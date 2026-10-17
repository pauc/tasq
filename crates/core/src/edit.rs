//! The small edits of the original script, `set`, `log` and `done`, as
//! operations on a [`Store`], plus [`reopen`], the inverse of `done`,
//! [`revise`], the TUI form's save, and the one-field edits of the CLI
//! ([`rename`], [`due`], [`retag`], [`describe`]).
//!
//! The CLI (`tasq set/log/done`) and the TUI (`t`, `p`, `l`, `d`) both call
//! these, so the two front ends cannot drift (FR-10): a status change is
//! always "read the task, change the field, optionally log the note, write
//! the whole task back", and `done` is always "log the note, close the task
//! with its closing time, write it back".

use std::path::PathBuf;

use chrono::NaiveDate;
use thiserror::Error;

use crate::clock::Clock;
use crate::model::{ModelError, Priority, Status, Tag, Task, TaskDraft, TaskId, Workflow};
use crate::query::{self, Filter};
use crate::store::{BatchError, Store, StoreError};

/// What `set` changes: a workflow status or a priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A workflow status.
    Status(Status),
    /// `A`, `B` or `C`.
    Priority(Priority),
}

impl Value {
    /// Interprets the user's word: a priority letter (with or without `#`)
    /// first, then a status of `workflow` (with or without `#`). `None`
    /// when it is neither.
    pub fn parse(text: &str, workflow: &Workflow) -> Option<Self> {
        if let Ok(priority) = text.parse::<Priority>() {
            return Some(Self::Priority(priority));
        }
        workflow.parse_status(text).map(Self::Status)
    }

    /// The label the script printed after `->`: the status name, or
    /// `priority #A`.
    pub fn label(&self) -> String {
        match self {
            Self::Status(status) => status.to_string(),
            Self::Priority(priority) => format!("priority {}", priority.to_hash()),
        }
    }

    /// Which field of the task this value is for.
    pub fn field(&self) -> Field {
        match self {
            Self::Status(_) => Field::Status,
            Self::Priority(_) => Field::Priority,
        }
    }

    /// Changes the field on `task`.
    pub fn apply(&self, task: &mut Task) {
        match self {
            Self::Status(status) => task.set_status(status.clone()),
            Self::Priority(priority) => task.set_priority(*priority),
        }
    }
}

/// The task field a [`Value`] sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The workflow status.
    Status,
    /// The priority.
    Priority,
}

impl Field {
    /// The field's name, as [`FIELD_NAMES`] spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Priority => "priority",
        }
    }
}

impl std::fmt::Display for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The fields the TUI's edit form shows and writes back at once: what
/// [`Fields::of`] reads off a task and what [`revise`] puts on it. The
/// lists (links, worktrees, sessions, progress) are not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields {
    /// The title line.
    pub title: String,
    /// `## Description`, without blank lines at either end (the projection
    /// trims them); `None` removes the section.
    pub description: Option<String>,
    /// The workflow status; `None` is an open task without a status tag.
    pub status: Option<Status>,
    /// `A`, `B` or `C`.
    pub priority: Priority,
    /// `## Due`; `None` removes the section.
    pub due: Option<NaiveDate>,
    /// `## Project`; `None` removes the section.
    pub project: Option<PathBuf>,
    /// The topic tags, in file order.
    pub tags: Vec<Tag>,
}

/// The names of the [`Fields`], in form order (what [`Fields::changed`]
/// reports).
pub const FIELD_NAMES: [&str; 7] = [
    "title",
    "description",
    "status",
    "priority",
    "due",
    "project",
    "tags",
];

impl Fields {
    /// The fields as `task` has them.
    pub fn of(task: &Task) -> Self {
        Self {
            title: task.title.clone(),
            description: task.description.clone(),
            status: task.status.clone(),
            priority: task.priority,
            due: task.due,
            project: task.project.clone(),
            tags: task.tags.clone(),
        }
    }

    /// The names of the fields in which `self` differs from `task`, in
    /// [`FIELD_NAMES`] order.
    pub fn changed(&self, task: &Task) -> Vec<&'static str> {
        let differs = [
            self.title != task.title,
            self.description != task.description,
            self.status != task.status,
            self.priority != task.priority,
            self.due != task.due,
            self.project != task.project,
            self.tags != task.tags,
        ];
        FIELD_NAMES
            .into_iter()
            .zip(differs)
            .filter_map(|(name, differs)| differs.then_some(name))
            .collect()
    }

    /// Puts the fields on `task`.
    pub fn apply(&self, task: &mut Task) {
        task.title.clone_from(&self.title);
        task.description.clone_from(&self.description);
        match &self.status {
            Some(status) => task.set_status(status.clone()),
            None => task.clear_status(),
        }
        task.set_priority(self.priority);
        task.due = self.due;
        task.project.clone_from(&self.project);
        task.tags.clone_from(&self.tags);
    }

    /// A new task with these fields (the create form's save): no progress
    /// note, nothing else set.
    pub fn draft(&self) -> TaskDraft {
        TaskDraft {
            status: self.status.clone(),
            priority: self.priority,
            due: self.due,
            description: self.description.clone(),
            project: self.project.clone(),
            tags: self.tags.clone(),
            ..TaskDraft::new(self.title.clone())
        }
    }
}

/// What [`revise`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revised {
    /// The task as stored afterwards.
    pub task: Task,
    /// The fields that changed, in [`FIELD_NAMES`] order; empty when the
    /// form was saved as it was opened (nothing was written then).
    pub changed: Vec<&'static str>,
}

/// The edit form's save: puts `fields` on the task, writes it when anything
/// differs and returns it as stored with the names of the changed fields.
/// An empty title is refused before anything is read.
pub fn revise(store: &mut dyn Store, id: &TaskId, fields: &Fields) -> Result<Revised, EditError> {
    if fields.title.trim().is_empty() {
        return Err(EditError::EmptyTitle);
    }
    let mut task = store.get(id)?;
    let changed = fields.changed(&task);
    if !changed.is_empty() {
        fields.apply(&mut task);
        store.update(&task)?;
        task = store.get(id)?;
    }
    Ok(Revised { task, changed })
}

/// Why an edit was refused.
#[derive(Debug, Error)]
pub enum EditError {
    /// A note was given but holds no text.
    #[error("the note must not be empty")]
    EmptyNote,
    /// The form's title holds no text.
    #[error("the title must not be empty")]
    EmptyTitle,
    /// A new title holds a line break; the title is the first line of the
    /// file.
    #[error("the title must be a single line")]
    MultiLineTitle,
    /// A `tag` word names a status or a priority, which are fields of the
    /// task, not topic tags.
    #[error("'{word}' is a {field}, not a tag; use set to change it")]
    NotATag {
        /// The word, without sign or `#`.
        word: String,
        /// The field the word names.
        field: Field,
    },
    /// A `tag` word is not a valid tag.
    #[error(transparent)]
    InvalidTag(#[from] ModelError),
    /// A description line starts with `## `, which the file would read as
    /// the heading of a new section.
    #[error("a description line cannot start with '## ' (it would start a new section): {0}")]
    DescriptionHeading(String),
    /// A note came with an edit that changed nothing; nothing was
    /// written, so the note was not logged either. Holds the field's name.
    #[error("{0} unchanged; the note was not logged (use log to add it alone)")]
    NoteNotLogged(&'static str),
    /// `done` was asked for a task that is already done.
    #[error("task {0} is already done")]
    AlreadyDone(TaskId),
    /// `reopen` was asked for a task that is still open.
    #[error("task {0} is not done")]
    NotDone(TaskId),
    /// `move` was asked for a done task, which has no status group to be
    /// ranked in.
    #[error("task {0} is done")]
    IsDone(TaskId),
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A batch write stopped; the error names the tasks already written.
    #[error(transparent)]
    Batch(#[from] BatchError),
}

/// `tasq set <id> <value> [note]`: changes the status or priority, logs
/// `note` when given, writes the task and returns it as stored. A value
/// the task already has is [`change`]'s no change.
pub fn set(
    store: &mut dyn Store,
    id: &TaskId,
    value: &Value,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Revised, EditError> {
    change(store, id, value.field().as_str(), note, clock, |task| {
        value.apply(task);
    })
}

/// `tasq rename <id> <title>`: replaces the title (trimmed), logs `note`
/// when given and returns the task as stored. An empty or multi-line title
/// is refused before anything is read; the same title is [`change`]'s no
/// change.
pub fn rename(
    store: &mut dyn Store,
    id: &TaskId,
    title: &str,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Revised, EditError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(EditError::EmptyTitle);
    }
    if title.contains(['\n', '\r']) {
        return Err(EditError::MultiLineTitle);
    }
    change(store, id, "title", note, clock, |task| {
        title.clone_into(&mut task.title);
    })
}

/// `tasq due <id> <date|none>`: sets the due date, or removes it with
/// `None`, logs `note` when given and returns the task as stored. The
/// date the task already has is [`change`]'s no change.
pub fn due(
    store: &mut dyn Store,
    id: &TaskId,
    day: Option<NaiveDate>,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Revised, EditError> {
    change(store, id, "due", note, clock, |task| task.due = day)
}

/// One word of `tasq tag`: `+name` or a bare `name` adds the topic tag,
/// `-name` removes it. A `#` after the sign is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagChange {
    /// Add the tag when the task does not have it.
    Add(Tag),
    /// Remove the tag when the task has it.
    Remove(Tag),
}

impl TagChange {
    /// Interprets `word`. A name that is a priority letter or a status of
    /// `workflow` is refused: those are fields, changed with `set`.
    pub fn parse(word: &str, workflow: &Workflow) -> Result<Self, EditError> {
        let (add, name) = match word.strip_prefix('-') {
            Some(rest) => (false, rest),
            None => (true, word.strip_prefix('+').unwrap_or(word)),
        };
        if let Some(value) = Value::parse(name, workflow) {
            return Err(EditError::NotATag {
                word: name.strip_prefix('#').unwrap_or(name).to_owned(),
                field: value.field(),
            });
        }
        let tag = name.parse::<Tag>()?;
        Ok(if add {
            Self::Add(tag)
        } else {
            Self::Remove(tag)
        })
    }

    /// `+name` or `-name`.
    pub fn label(&self) -> String {
        match self {
            Self::Add(tag) => format!("+{tag}"),
            Self::Remove(tag) => format!("-{tag}"),
        }
    }

    /// Whether going from `before` to `after` made this change.
    fn took_effect(&self, before: &[Tag], after: &[Tag]) -> bool {
        match self {
            Self::Add(tag) => !before.contains(tag) && after.contains(tag),
            Self::Remove(tag) => before.contains(tag) && !after.contains(tag),
        }
    }

    /// Applies the change to `tags`; adding keeps file order and appends.
    fn apply(&self, tags: &mut Vec<Tag>) {
        match self {
            Self::Add(tag) => {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            Self::Remove(tag) => tags.retain(|t| t != tag),
        }
    }
}

/// What [`retag`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retagged {
    /// The task as stored afterwards (as read when nothing was written).
    pub task: Task,
    /// The requested changes that took effect, in request order, each
    /// once: adding a tag the task had, removing one it did not have, or
    /// adding and then removing the same tag is no change.
    pub applied: Vec<TagChange>,
}

/// `tasq tag <id> <change>...`: applies `changes` in order to the topic
/// tags. Tags that stay keep their place and new ones go at the end; the
/// others are left alone. Logs `note` when given and returns the task as
/// stored with the changes that took effect. When no tag changes nothing
/// is written: that is no error without a note, and
/// [`EditError::NoteNotLogged`] with one.
pub fn retag(
    store: &mut dyn Store,
    id: &TaskId,
    changes: &[TagChange],
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Retagged, EditError> {
    let note = checked_note(note)?;
    let task = store.get(id)?;
    let mut wanted = task.tags.clone();
    for change in changes {
        change.apply(&mut wanted);
    }
    let mut applied: Vec<TagChange> = Vec::new();
    for change in changes {
        if change.took_effect(&task.tags, &wanted) && !applied.contains(change) {
            applied.push(change.clone());
        }
    }
    if applied.is_empty() {
        return unchanged("tags", note).map(|()| Retagged { task, applied });
    }
    let kept = task.tags.iter().filter(|t| wanted.contains(t));
    let added = wanted.iter().filter(|t| !task.tags.contains(t));
    let tags: Vec<Tag> = kept.chain(added).cloned().collect();
    let mut task = task;
    task.tags = tags;
    let task = save(store, task, note, clock)?;
    Ok(Retagged { task, applied })
}

/// `text` as the file reads a description back: CRLF line ends become LF,
/// blank lines are dropped at both ends, and nothing left is `None` (no
/// `## Description` section).
pub fn description_text(text: &str) -> Option<String> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| !l.trim().is_empty())?;
    let end = lines.iter().rposition(|l| !l.trim().is_empty())?;
    Some(lines[start..=end].join("\n"))
}

/// `tasq desc <id> <text>`: [`describe_read`] on the task as it is now.
pub fn describe(
    store: &mut dyn Store,
    id: &TaskId,
    text: &str,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Revised, EditError> {
    checked_description(text)?;
    let task = store.get(id)?;
    describe_read(store, task, text, note, clock)
}

/// [`describe`] for a task the caller read earlier, such as before opening
/// an editor on its description. A store that checks for conflicts
/// compares against its own last read of the id (the nb store keeps one
/// revision per id), so the check covers the time since that read only
/// when the caller does not read the same id again in between.
///
/// Replaces the description with [`description_text`] of `text` (blank
/// text removes the section), logs `note` when given and returns the task
/// as stored with `["description"]` as the change. The same description
/// is [`change`]'s no change. A line starting with `## ` is refused.
pub fn describe_read(
    store: &mut dyn Store,
    task: Task,
    text: &str,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Revised, EditError> {
    checked_description(text)?;
    let note = checked_note(note)?;
    let description = description_text(text);
    edited(store, task, "description", note, clock, |task| {
        task.description = description;
    })
}

/// Refuses a description with a line the file would read as a section
/// heading.
fn checked_description(text: &str) -> Result<(), EditError> {
    match text.lines().find(|l| l.starts_with("## ")) {
        Some(line) => Err(EditError::DescriptionHeading(line.to_owned())),
        None => Ok(()),
    }
}

/// `tasq log <id> <note>`: appends a dated progress note and returns the
/// task as stored.
pub fn log(
    store: &mut dyn Store,
    id: &TaskId,
    note: &str,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(Some(note))?.ok_or(EditError::EmptyNote)?;
    let mut task = store.get(id)?;
    task.log(note, clock);
    store.update(&task)?;
    Ok(store.get(id)?)
}

/// `tasq done <id> [note]`: logs `note` when given and closes the task
/// (`# [x]`, status tag removed, `closed_at` stamped with `clock.now()`,
/// see [`Task::close`]) in one write, and returns it as stored. A task
/// that is already done is refused before anything is written.
pub fn done(
    store: &mut dyn Store,
    id: &TaskId,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?;
    let mut task = store.get(id)?;
    if task.done {
        return Err(EditError::AlreadyDone(id.clone()));
    }
    task.close(clock);
    save(store, task, note, clock)
}

/// The note [`reopen`] logs when the caller gives none.
pub const REOPENED_NOTE: &str = "reopened";

/// `tasq reopen <id> [status] [note]`: the inverse of [`done`]. Clears
/// `# [x]`, puts `status` back (the caller picks it, usually the given one or
/// `workflow.default_status`; the status the task had before `done` is gone
/// from the file), drops `closed_at` and logs `note`, or [`REOPENED_NOTE`],
/// in one write.
/// A task that is not done is refused before anything is written.
pub fn reopen(
    store: &mut dyn Store,
    id: &TaskId,
    status: &Status,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?.unwrap_or(REOPENED_NOTE);
    let mut task = store.get(id)?;
    if !task.done {
        return Err(EditError::NotDone(id.clone()));
    }
    task.done = false;
    task.closed_at = None;
    task.set_status(status.clone());
    task.log(note, clock);
    store.update(&task)?;
    Ok(store.get(id)?)
}

/// The distance between two ranks written by a move (ADR 0025).
pub const RANK_STEP: i64 = 1024;

/// Where `tasq move` puts a task within its status group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// First in the group: the group's lowest rank minus [`RANK_STEP`], or
    /// [`RANK_STEP`] when nothing else in the group is ranked.
    Top,
    /// No rank: back to the natural priority, due, id order.
    Unrank,
}

/// What [`move_task`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The task as stored afterwards (as read when nothing was written).
    pub task: Task,
    /// `["rank"]` when the task's rank was written, `[]` when the move left
    /// it where it was and nothing was written.
    pub changed: Vec<&'static str>,
    /// The other tasks of the group whose rank was written for this move,
    /// in batch order; empty for a move that touched the task alone.
    pub ranked: Vec<TaskId>,
}

/// `tasq move <id> <target>`: sets the task's rank so that it sits at
/// `target` within its status group (the open tasks sharing its status,
/// the no-status group included) and writes every task whose rank changed
/// as one batch. A done task is refused before anything is read further;
/// a move that leaves the task where it is writes nothing.
pub fn move_task(store: &mut dyn Store, id: &TaskId, target: &Target) -> Result<Moved, EditError> {
    let task = store.get(id)?;
    if task.done {
        return Err(EditError::IsDone(id.clone()));
    }
    let group: Vec<Task> = store
        .list(&Filter::new())?
        .into_iter()
        .filter(|t| t.status == task.status)
        .collect();
    let rank = match target {
        Target::Top => top_rank(&group, id),
        Target::Unrank => None,
    };
    if rank == task.rank {
        return Ok(Moved {
            task,
            changed: Vec::new(),
            ranked: Vec::new(),
        });
    }
    let mut moved = task;
    moved.rank = rank;
    store.update_many(std::slice::from_ref(&moved))?;
    Ok(Moved {
        task: store.get(id)?,
        changed: vec!["rank"],
        ranked: Vec::new(),
    })
}

/// The rank that puts `id` first in `group`: its current rank when it is
/// already first, else the lowest rank of the others minus [`RANK_STEP`],
/// or [`RANK_STEP`] when none of them is ranked.
fn top_rank(group: &[Task], id: &TaskId) -> Option<i64> {
    let ordered = query::sort(group);
    let first = ordered.first().expect("the group holds the moved task");
    if first.id == *id {
        return first.rank;
    }
    let lowest = group
        .iter()
        .filter(|t| t.id != *id)
        .filter_map(|t| t.rank)
        .min();
    Some(lowest.map_or(RANK_STEP, |rank| rank - RANK_STEP))
}

/// The shape of every one-field edit: checks `note`, reads the task and
/// hands it to [`edited`].
fn change(
    store: &mut dyn Store,
    id: &TaskId,
    field: &'static str,
    note: Option<&str>,
    clock: &dyn Clock,
    edit: impl FnOnce(&mut Task),
) -> Result<Revised, EditError> {
    let note = checked_note(note)?;
    let task = store.get(id)?;
    edited(store, task, field, note, clock, edit)
}

/// The second half of [`change`], for a task the caller already read:
/// lets `edit` change `field`, then, when it differs, logs the checked
/// `note`, writes and returns the task as stored with `[field]` as the
/// change. When `field` stays the same nothing is written: see
/// [`unchanged`].
fn edited(
    store: &mut dyn Store,
    task: Task,
    field: &'static str,
    note: Option<&str>,
    clock: &dyn Clock,
    edit: impl FnOnce(&mut Task),
) -> Result<Revised, EditError> {
    let mut after = task.clone();
    edit(&mut after);
    if Fields::of(&after) == Fields::of(&task) {
        return unchanged(field, note).map(|()| Revised {
            task,
            changed: Vec::new(),
        });
    }
    let task = save(store, after, note, clock)?;
    Ok(Revised {
        task,
        changed: vec![field],
    })
}

/// An edit that changed nothing writes nothing: no error without a note,
/// [`EditError::NoteNotLogged`] with one, rather than a note logged alone.
fn unchanged(field: &'static str, note: Option<&str>) -> Result<(), EditError> {
    match note {
        Some(_) => Err(EditError::NoteNotLogged(field)),
        None => Ok(()),
    }
}

/// Logs the checked `note` on `task`, writes it and reads it back.
fn save(
    store: &mut dyn Store,
    mut task: Task,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    if let Some(note) = note {
        task.log(note, clock);
    }
    store.update(&task)?;
    Ok(store.get(&task.id)?)
}

/// A given note must hold text; `None` passes through.
fn checked_note(note: Option<&str>) -> Result<Option<&str>, EditError> {
    match note {
        Some(text) if text.trim().is_empty() => Err(EditError::EmptyNote),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::ProgressEntry;
    use crate::store::MemoryStore;

    fn store() -> MemoryStore {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        let b = Task::new(TaskId::from(2), "B");
        MemoryStore::new([a, b])
    }

    fn clock() -> FixedClock {
        FixedClock::at("2026-10-04 10:15")
    }

    fn entry(note: &str) -> ProgressEntry {
        ProgressEntry::new(clock().0, note)
    }

    #[test]
    fn value_parsing_prefers_priority_then_status() {
        let wf = Workflow::default();
        assert_eq!(Value::parse("A", &wf), Some(Value::Priority(Priority::A)));
        assert_eq!(Value::parse("#C", &wf), Some(Value::Priority(Priority::C)));
        assert_eq!(
            Value::parse("#ready", &wf),
            Some(Value::Status(Status::READY))
        );
        assert_eq!(
            Value::parse("blocked", &wf),
            Some(Value::Status(Status::BLOCKED))
        );
        assert_eq!(Value::parse("nope", &wf), None);
        // Lowercase letters are tags in the file, so they are not priorities.
        assert_eq!(Value::parse("a", &wf), None);
    }

    #[test]
    fn value_labels_and_apply() {
        assert_eq!(Value::Status(Status::BLOCKED).label(), "blocked");
        assert_eq!(Value::Priority(Priority::A).label(), "priority #A");
        let mut task = Task::new(TaskId::from(1), "x");
        Value::Status(Status::WAITING).apply(&mut task);
        assert_eq!(task.status, Some(Status::WAITING));
        assert_eq!(task.priority, Priority::B);
        Value::Priority(Priority::C).apply(&mut task);
        assert_eq!(task.priority, Priority::C);
        assert_eq!(task.status, Some(Status::WAITING));
    }

    #[test]
    fn set_status_with_and_without_note() {
        let mut store = store();
        let id = TaskId::from(1);
        let task = set(
            &mut store,
            &id,
            &Value::Status(Status::IN_PROGRESS),
            None,
            &clock(),
        )
        .unwrap()
        .task;
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), task);

        let task = set(
            &mut store,
            &id,
            &Value::Priority(Priority::A),
            Some("urgent"),
            &clock(),
        )
        .unwrap()
        .task;
        assert_eq!(task.priority, Priority::A);
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, vec![entry("urgent")]);
        assert_eq!(store.get(&id).unwrap(), task);
    }

    #[test]
    fn status_change_done_and_reopen_keep_the_rank() {
        let mut store = store();
        let id = TaskId::from(1);
        let mut ranked = store.get(&id).unwrap();
        ranked.rank = Some(1024);
        store.update(&ranked).unwrap();
        let task = set(
            &mut store,
            &id,
            &Value::Status(Status::IN_PROGRESS),
            None,
            &clock(),
        )
        .unwrap()
        .task;
        assert_eq!(task.rank, Some(1024));
        let task = done(&mut store, &id, None, &clock()).unwrap();
        assert_eq!(task.rank, Some(1024));
        let task = reopen(&mut store, &id, &Status::READY, None, &clock()).unwrap();
        assert_eq!(task.rank, Some(1024));
        assert_eq!(store.get(&id).unwrap().rank, Some(1024));
    }

    /// Three ready tasks: 1 (`#A`), 3 (`#B`), 4 (`#C`), plus 2 without a
    /// status and 5 done.
    fn group() -> MemoryStore {
        let mut urgent = Task::new(TaskId::from(1), "A");
        urgent.set_status(Status::READY);
        urgent.set_priority(Priority::A);
        let no_status = Task::new(TaskId::from(2), "B");
        let mut normal = Task::new(TaskId::from(3), "C");
        normal.set_status(Status::READY);
        let mut low = Task::new(TaskId::from(4), "D");
        low.set_status(Status::READY);
        low.set_priority(Priority::C);
        let mut done = Task::new(TaskId::from(5), "E");
        done.mark_done();
        MemoryStore::new([urgent, no_status, normal, low, done])
    }

    #[test]
    fn move_top_ranks_1024_when_nothing_in_the_group_is_ranked() {
        let mut store = group();
        let moved = move_task(&mut store, &TaskId::from(4), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(1024));
        assert_eq!(moved.changed, vec!["rank"]);
        assert_eq!(moved.ranked, Vec::<TaskId>::new());
        assert_eq!(store.get(&TaskId::from(4)).unwrap(), moved.task);
        let ready = crate::query::sort(
            store
                .tasks()
                .iter()
                .filter(|t| t.status == Some(Status::READY)),
        );
        let ids: Vec<&TaskId> = ready.iter().map(|t| &t.id).collect();
        assert_eq!(ids, [&TaskId::from(4), &TaskId::from(1), &TaskId::from(3)]);
    }

    fn ranked(store: &mut MemoryStore, id: u64, rank: i64) {
        let mut task = store.get(&TaskId::from(id)).unwrap();
        task.rank = Some(rank);
        store.update(&task).unwrap();
    }

    fn ready_ids(store: &MemoryStore) -> Vec<TaskId> {
        crate::query::sort(
            store
                .tasks()
                .iter()
                .filter(|t| t.status == Some(Status::READY)),
        )
        .into_iter()
        .map(|t| t.id.clone())
        .collect()
    }

    #[test]
    fn move_top_goes_one_step_below_the_lowest_rank_of_the_others() {
        let mut store = group();
        ranked(&mut store, 1, 1024);
        ranked(&mut store, 3, 2048);
        let moved = move_task(&mut store, &TaskId::from(4), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(0));
        assert_eq!(ready_ids(&store), [4, 1, 3].map(TaskId::from));
        // A ranked task that is not first moves above the lowest rank,
        // its own rank left out of the minimum.
        let moved = move_task(&mut store, &TaskId::from(3), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(-1024));
        assert_eq!(moved.changed, vec!["rank"]);
        assert_eq!(ready_ids(&store), [3, 4, 1].map(TaskId::from));
    }

    #[test]
    fn move_top_only_looks_at_the_tasks_own_status_group() {
        let mut store = group();
        // Task 2 has no status: its rank is not this group's lowest.
        ranked(&mut store, 2, -5000);
        let moved = move_task(&mut store, &TaskId::from(4), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(1024));
        // The no-status tasks are a group of their own and move like any other.
        let draft = TaskDraft {
            status: None,
            ..TaskDraft::new("F")
        };
        assert_eq!(store.create(draft).unwrap().id, TaskId::from(6));
        let moved = move_task(&mut store, &TaskId::from(6), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(-6024));
        assert_eq!(moved.task.status, None);
    }

    #[test]
    fn move_top_on_the_first_task_writes_nothing() {
        let mut store = group();
        let first = store.get(&TaskId::from(1)).unwrap();
        // First by the natural order, nothing ranked.
        let mut no_write = NoWrite(store.clone());
        let moved = move_task(&mut no_write, &TaskId::from(1), &Target::Top).unwrap();
        assert_eq!(
            moved,
            Moved {
                task: first,
                changed: Vec::new(),
                ranked: Vec::new()
            }
        );
        // First by rank.
        ranked(&mut store, 4, 1024);
        ranked(&mut store, 3, 2048);
        let mut no_write = NoWrite(store);
        let moved = move_task(&mut no_write, &TaskId::from(4), &Target::Top).unwrap();
        assert_eq!(moved.task.rank, Some(1024));
        assert_eq!(moved.changed, Vec::<&str>::new());
    }

    #[test]
    fn move_unrank_removes_the_rank_and_is_a_no_op_without_one() {
        let mut store = group();
        ranked(&mut store, 4, 1024);
        assert_eq!(ready_ids(&store), [4, 1, 3].map(TaskId::from));
        let moved = move_task(&mut store, &TaskId::from(4), &Target::Unrank).unwrap();
        assert_eq!(moved.task.rank, None);
        assert_eq!(moved.changed, vec!["rank"]);
        assert_eq!(moved.ranked, Vec::<TaskId>::new());
        assert_eq!(store.get(&TaskId::from(4)).unwrap(), moved.task);
        assert_eq!(ready_ids(&store), [1, 3, 4].map(TaskId::from));
        let mut no_write = NoWrite(store);
        let moved = move_task(&mut no_write, &TaskId::from(4), &Target::Unrank).unwrap();
        assert_eq!(moved.task.rank, None);
        assert_eq!(moved.changed, Vec::<&str>::new());
    }

    #[test]
    fn move_refuses_done_and_missing_tasks_before_writing() {
        let mut store = NoWrite(group());
        let err = move_task(&mut store, &TaskId::from(5), &Target::Top).unwrap_err();
        assert!(matches!(err, EditError::IsDone(ref id) if *id == TaskId::from(5)));
        assert_eq!(err.to_string(), "task 5 is done");
        let err = move_task(&mut store, &TaskId::from(5), &Target::Unrank).unwrap_err();
        assert_eq!(err.to_string(), "task 5 is done");
        let err = move_task(&mut store, &TaskId::from(9), &Target::Top).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn set_refuses_an_empty_note_before_writing() {
        let mut store = store();
        let before = store.get(&TaskId::from(1)).unwrap();
        let err = set(
            &mut store,
            &TaskId::from(1),
            &Value::Status(Status::LATER),
            Some("  "),
            &clock(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "the note must not be empty");
        assert_eq!(store.get(&TaskId::from(1)).unwrap(), before);
    }

    #[test]
    fn set_on_a_missing_task_is_not_found() {
        let mut store = store();
        let err = set(
            &mut store,
            &TaskId::from(9),
            &Value::Priority(Priority::A),
            None,
            &clock(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
        assert!(matches!(err, EditError::Store(StoreError::NotFound(_))));
    }

    #[test]
    fn log_appends_a_dated_note() {
        let mut store = store();
        let id = TaskId::from(2);
        let task = log(&mut store, &id, "found the cause", &clock()).unwrap();
        assert_eq!(task.progress, vec![entry("found the cause")]);
        let task = log(&mut store, &id, "fixed", &clock()).unwrap();
        assert_eq!(
            task.progress,
            vec![entry("found the cause"), entry("fixed")]
        );
        assert_eq!(store.get(&id).unwrap(), task);
        assert_eq!(
            log(&mut store, &id, " \n", &clock())
                .unwrap_err()
                .to_string(),
            "the note must not be empty"
        );
        assert_eq!(store.get(&id).unwrap().progress.len(), 2);
        assert_eq!(
            log(&mut store, &TaskId::from(9), "x", &clock())
                .unwrap_err()
                .to_string(),
            "no task with id 9"
        );
    }

    #[test]
    fn done_logs_then_closes() {
        let mut store = store();
        let id = TaskId::from(1);
        let task = done(&mut store, &id, Some("merged"), &clock()).unwrap();
        assert!(task.done);
        assert_eq!(task.status, None);
        assert_eq!(task.closed_at, Some(clock().now()));
        assert_eq!(task.progress, vec![entry("merged")]);
        assert_eq!(store.get(&id).unwrap(), task);

        let task = done(&mut store, &TaskId::from(2), None, &clock()).unwrap();
        assert!(task.done);
        assert_eq!(task.closed_at, Some(clock().now()));
        assert_eq!(task.progress, Vec::new());

        // Done again: refused, nothing written.
        let before = store.get(&id).unwrap();
        let later = FixedClock::at("2026-10-09 08:00");
        let err = done(&mut store, &id, Some("follow-up"), &later).unwrap_err();
        assert!(matches!(err, EditError::AlreadyDone(ref i) if *i == id));
        assert_eq!(err.to_string(), "task 1 is already done");
        let err = done(&mut store, &id, None, &later).unwrap_err();
        assert!(matches!(err, EditError::AlreadyDone(_)));
        assert_eq!(store.get(&id).unwrap(), before);
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing() {
        let mut store = store();
        let id = TaskId::from(1);
        let day = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap();
        due(&mut store, &id, Some(day), None, &clock()).unwrap();
        retag(
            &mut store,
            &id,
            &[TagChange::Add(Tag::new("x").unwrap())],
            None,
            &clock(),
        )
        .unwrap();
        let before = store.get(&id).unwrap();

        let ready = Value::Status(Status::READY);
        let revised = set(&mut store, &id, &ready, None, &clock()).unwrap();
        assert_eq!(
            revised,
            Revised {
                task: before.clone(),
                changed: Vec::new()
            }
        );
        let revised = rename(&mut store, &id, " A ", None, &clock()).unwrap();
        assert_eq!(revised.changed, Vec::<&str>::new());
        let revised = due(&mut store, &id, Some(day), None, &clock()).unwrap();
        assert_eq!(revised.changed, Vec::<&str>::new());

        let note = Some("why");
        let message = |err: EditError| err.to_string();
        let err = set(&mut store, &id, &ready, note, &clock()).unwrap_err();
        assert!(matches!(err, EditError::NoteNotLogged("status")));
        let normal = Value::Priority(Priority::default());
        let err = set(&mut store, &id, &normal, note, &clock()).unwrap_err();
        assert_eq!(
            message(err),
            "priority unchanged; the note was not logged (use log to add it alone)"
        );
        let err = rename(&mut store, &id, "A", note, &clock()).unwrap_err();
        assert!(matches!(err, EditError::NoteNotLogged("title")));
        let err = due(&mut store, &id, Some(day), note, &clock()).unwrap_err();
        assert!(matches!(err, EditError::NoteNotLogged("due")));
        let add_x = [TagChange::Add(Tag::new("x").unwrap())];
        let err = retag(&mut store, &id, &add_x, note, &clock()).unwrap_err();
        assert!(matches!(err, EditError::NoteNotLogged("tags")));
        assert_eq!(store.get(&id).unwrap(), before);

        // A change names its field.
        let later = Value::Status(Status::LATER);
        let revised = set(&mut store, &id, &later, note, &clock()).unwrap();
        assert_eq!(revised.changed, vec!["status"]);
        let revised = set(
            &mut store,
            &id,
            &Value::Priority(Priority::A),
            None,
            &clock(),
        )
        .unwrap();
        assert_eq!(revised.changed, vec!["priority"]);
        assert_eq!(
            rename(&mut store, &id, "B", None, &clock())
                .unwrap()
                .changed,
            vec!["title"]
        );
        assert_eq!(
            due(&mut store, &id, None, None, &clock()).unwrap().changed,
            vec!["due"]
        );
        assert_eq!(store.get(&id).unwrap().progress, vec![entry("why")]);
    }

    #[test]
    fn done_refuses_an_empty_note_and_missing_tasks() {
        let mut store = store();
        let err = done(&mut store, &TaskId::from(1), Some(""), &clock()).unwrap_err();
        assert_eq!(err.to_string(), "the note must not be empty");
        assert!(!store.get(&TaskId::from(1)).unwrap().done);
        let err = done(&mut store, &TaskId::from(9), None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn reopen_clears_done_sets_the_status_and_logs() {
        let mut store = store();
        let id = TaskId::from(1);
        done(&mut store, &id, None, &clock()).unwrap();
        let task = reopen(&mut store, &id, &Status::IN_PROGRESS, None, &clock()).unwrap();
        assert!(!task.done);
        assert_eq!(task.closed_at, None);
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, vec![entry(REOPENED_NOTE)]);
        assert_eq!(REOPENED_NOTE, "reopened");
        assert_eq!(store.get(&id).unwrap(), task);

        done(&mut store, &id, None, &clock()).unwrap();
        let task = reopen(
            &mut store,
            &id,
            &Status::READY,
            Some("not merged"),
            &clock(),
        )
        .unwrap();
        assert_eq!(task.status, Some(Status::READY));
        assert_eq!(
            task.progress,
            vec![entry(REOPENED_NOTE), entry("not merged")]
        );
        assert_eq!(store.get(&id).unwrap(), task);
    }

    #[test]
    fn reopen_refuses_open_tasks_empty_notes_and_missing_tasks() {
        let mut store = store();
        let id = TaskId::from(1);
        let mut read_only = NoWrite(store.clone());
        let err = reopen(&mut read_only, &id, &Status::READY, None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "task 1 is not done");
        assert!(matches!(err, EditError::NotDone(ref i) if *i == id));

        done(&mut store, &id, None, &clock()).unwrap();
        let before = store.get(&id).unwrap();
        let err = reopen(&mut store, &id, &Status::READY, Some(" "), &clock()).unwrap_err();
        assert_eq!(err.to_string(), "the note must not be empty");
        assert_eq!(store.get(&id).unwrap(), before);
        let err = reopen(&mut store, &TaskId::from(9), &Status::READY, None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn fields_make_a_draft() {
        let fields = Fields {
            title: "T".into(),
            description: Some("D".into()),
            status: None,
            priority: Priority::A,
            due: NaiveDate::from_ymd_opt(2026, 10, 9),
            project: Some(PathBuf::from("/p")),
            tags: vec![Tag::new("x").unwrap()],
        };
        let mut expected = TaskDraft::new("T")
            .with_status(None)
            .with_priority(Priority::A)
            .with_due(NaiveDate::from_ymd_opt(2026, 10, 9).unwrap())
            .with_description("D")
            .with_project("/p")
            .with_tag(Tag::new("x").unwrap());
        assert_eq!(fields.draft(), expected);
        let ready = Fields {
            status: Some(Status::WAITING),
            ..fields
        };
        expected.status = Some(Status::WAITING);
        assert_eq!(ready.draft(), expected);
    }

    #[test]
    fn fields_read_off_a_task_and_name_what_differs() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.set_status(Status::READY);
        task.add_tag(Tag::new("gitlab").unwrap());
        task.description = Some("Why.".into());
        let fields = Fields::of(&task);
        assert_eq!(
            fields,
            Fields {
                title: "A".into(),
                description: Some("Why.".into()),
                status: Some(Status::READY),
                priority: Priority::B,
                due: None,
                project: None,
                tags: vec![Tag::new("gitlab").unwrap()],
            }
        );
        assert_eq!(fields.changed(&task), Vec::<&str>::new());
        let all = Fields {
            title: "B".into(),
            description: None,
            status: None,
            priority: Priority::A,
            due: Some(clock().today()),
            project: Some("/p".into()),
            tags: Vec::new(),
        };
        assert_eq!(all.changed(&task), FIELD_NAMES);
        let mut applied = task.clone();
        all.apply(&mut applied);
        assert_eq!(Fields::of(&applied), all);
        assert_eq!(applied.status, None);
        assert_eq!(applied.progress, task.progress, "only the fields change");
        let mut back = applied.clone();
        fields.apply(&mut back);
        assert_eq!(back, task);
        let one = Fields {
            due: None,
            ..all.clone()
        };
        assert_eq!(one.changed(&applied), vec!["due"]);
    }

    #[test]
    fn revise_writes_only_when_something_differs() {
        let mut store = store();
        let id = TaskId::from(1);
        let before = store.get(&id).unwrap();
        let mut read_only = NoWrite(store.clone());
        let same = revise(&mut read_only, &id, &Fields::of(&before)).unwrap();
        assert_eq!(same.changed, Vec::<&str>::new());
        assert_eq!(same.task, before);

        let fields = Fields {
            title: "Renamed".into(),
            status: Some(Status::BLOCKED),
            due: Some(clock().today()),
            ..Fields::of(&before)
        };
        let revised = revise(&mut store, &id, &fields).unwrap();
        assert_eq!(revised.changed, vec!["title", "status", "due"]);
        assert_eq!(revised.task.title, "Renamed");
        assert_eq!(revised.task.status, Some(Status::BLOCKED));
        assert_eq!(revised.task.due, Some(clock().today()));
        assert_eq!(store.get(&id).unwrap(), revised.task);
    }

    /// A store that reads like a [`MemoryStore`] and panics on any write.
    struct NoWrite(MemoryStore);

    impl Store for NoWrite {
        fn list(&self, filter: &crate::query::Filter) -> Result<Vec<Task>, StoreError> {
            self.0.list(filter)
        }

        fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
            self.0.get(id)
        }

        fn create(&mut self, _draft: crate::model::TaskDraft) -> Result<Task, StoreError> {
            panic!("no write expected")
        }

        fn update(&mut self, _task: &Task) -> Result<(), StoreError> {
            panic!("no write expected")
        }

        fn set_done(&mut self, _id: &TaskId, _done: bool) -> Result<(), StoreError> {
            panic!("no write expected")
        }

        fn describe(&self) -> crate::store::StoreInfo {
            self.0.describe()
        }
    }

    #[test]
    fn revise_refuses_an_empty_title_and_missing_tasks() {
        let mut store = store();
        let id = TaskId::from(1);
        let before = store.get(&id).unwrap();
        let blank = Fields {
            title: " \t".into(),
            ..Fields::of(&before)
        };
        let err = revise(&mut store, &id, &blank).unwrap_err();
        assert_eq!(err.to_string(), "the title must not be empty");
        assert!(matches!(err, EditError::EmptyTitle));
        assert_eq!(store.get(&id).unwrap(), before);
        let err = revise(&mut store, &TaskId::from(9), &Fields::of(&before)).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn rename_trims_the_title_and_logs_the_note() {
        let mut store = store();
        let id = TaskId::from(1);
        let task = rename(&mut store, &id, "  Renamed  ", None, &clock())
            .unwrap()
            .task;
        assert_eq!(task.title, "Renamed");
        assert_eq!(task.progress, Vec::new());
        assert_eq!(task.status, Some(Status::READY));
        assert_eq!(store.get(&id).unwrap(), task);
        let task = rename(&mut store, &id, "Again", Some("clearer"), &clock())
            .unwrap()
            .task;
        assert_eq!(task.title, "Again");
        assert_eq!(task.progress, vec![entry("clearer")]);
    }

    #[test]
    fn rename_refuses_empty_and_multi_line_titles_before_reading() {
        let mut store = store();
        let id = TaskId::from(1);
        let before = store.get(&id).unwrap();
        let err = rename(&mut store, &id, " \t", None, &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyTitle));
        let err = rename(&mut store, &id, "one\ntwo", None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "the title must be a single line");
        let err = rename(&mut store, &id, "one\rtwo", None, &clock()).unwrap_err();
        assert!(matches!(err, EditError::MultiLineTitle));
        let err = rename(&mut store, &id, "x", Some(" "), &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyNote));
        assert_eq!(store.get(&id).unwrap(), before);
        let err = rename(&mut store, &TaskId::from(9), "x", None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn due_sets_and_clears_the_date() {
        let mut store = store();
        let id = TaskId::from(2);
        let day = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap();
        let task = due(&mut store, &id, Some(day), None, &clock())
            .unwrap()
            .task;
        assert_eq!(task.due, Some(day));
        assert_eq!(task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), task);
        let task = due(&mut store, &id, None, Some("no deadline"), &clock())
            .unwrap()
            .task;
        assert_eq!(task.due, None);
        assert_eq!(task.progress, vec![entry("no deadline")]);
        assert_eq!(store.get(&id).unwrap(), task);
        let err = due(&mut store, &id, Some(day), Some(""), &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyNote));
        assert_eq!(store.get(&id).unwrap().due, None);
    }

    fn tags(names: &[&str]) -> Vec<Tag> {
        names.iter().map(|n| Tag::new(*n).unwrap()).collect()
    }

    #[test]
    fn tag_change_parsing() {
        let wf = Workflow::default();
        let parse = |w: &str| TagChange::parse(w, &wf);
        assert_eq!(parse("+a").unwrap(), TagChange::Add(Tag::new("a").unwrap()));
        assert_eq!(parse("a").unwrap(), TagChange::Add(Tag::new("a").unwrap()));
        assert_eq!(parse("#a").unwrap(), TagChange::Add(Tag::new("a").unwrap()));
        assert_eq!(
            parse("+#a").unwrap(),
            TagChange::Add(Tag::new("a").unwrap())
        );
        assert_eq!(
            parse("-b").unwrap(),
            TagChange::Remove(Tag::new("b").unwrap())
        );
        assert_eq!(
            parse("-#b").unwrap(),
            TagChange::Remove(Tag::new("b").unwrap())
        );
        // Lowercase letters are tags, as for `set`.
        assert_eq!(parse("+a").unwrap().label(), "+a");
        assert_eq!(parse("-b").unwrap().label(), "-b");
        assert_eq!(
            parse("+ready").unwrap_err().to_string(),
            "'ready' is a status, not a tag; use set to change it"
        );
        assert_eq!(
            parse("-#blocked").unwrap_err().to_string(),
            "'blocked' is a status, not a tag; use set to change it"
        );
        assert_eq!(
            parse("A").unwrap_err().to_string(),
            "'A' is a priority, not a tag; use set to change it"
        );
        assert_eq!(
            parse("-#C").unwrap_err().to_string(),
            "'C' is a priority, not a tag; use set to change it"
        );
        assert_eq!(
            parse("+").unwrap_err().to_string(),
            "invalid tag \"\": must not be empty"
        );
        assert_eq!(
            parse("-##x").unwrap_err().to_string(),
            "invalid tag \"#x\": must not start with '#'"
        );
    }

    #[test]
    fn retag_adds_and_removes_only_the_named_tags() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.set_status(Status::READY);
        task.tags = tags(&["gitlab", "needs-triage", "ux"]);
        let mut store = MemoryStore::new([task]);
        let id = TaskId::from(1);
        let wf = Workflow::default();
        let changes: Vec<TagChange> = ["+needs-info", "-needs-triage", "ux", "-absent"]
            .iter()
            .map(|w| TagChange::parse(w, &wf).unwrap())
            .collect();
        let done = retag(&mut store, &id, &changes, None, &clock()).unwrap();
        let task = done.task;
        assert_eq!(task.tags, tags(&["gitlab", "ux", "needs-info"]));
        assert_eq!(task.status, Some(Status::READY));
        assert_eq!(task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), task);
        // `ux` was there and `absent` was not: only the real changes count.
        assert_eq!(labels(&done.applied), ["+needs-info", "-needs-triage"]);
        // Later words win: add then remove leaves it out, and is no change.
        let changes = [
            TagChange::parse("x", &wf).unwrap(),
            TagChange::parse("-x", &wf).unwrap(),
            TagChange::parse("-gitlab", &wf).unwrap(),
            TagChange::parse("+gitlab", &wf).unwrap(),
            TagChange::parse("-ux", &wf).unwrap(),
            TagChange::parse("-ux", &wf).unwrap(),
        ];
        let done = retag(&mut store, &id, &changes, Some("triaged"), &clock()).unwrap();
        assert_eq!(
            done.task.tags,
            tags(&["gitlab", "needs-info"]),
            "order kept"
        );
        assert_eq!(done.task.progress, vec![entry("triaged")]);
        assert_eq!(labels(&done.applied), ["-ux"]);
        let err = retag(&mut store, &id, &changes, Some(" "), &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyNote));
    }

    fn labels(changes: &[TagChange]) -> Vec<String> {
        changes.iter().map(TagChange::label).collect()
    }

    #[test]
    fn retag_without_a_change_or_a_note_writes_nothing() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.tags = tags(&["ux"]);
        let mut store = NoWrite(MemoryStore::new([task]));
        let wf = Workflow::default();
        let changes = [
            TagChange::parse("ux", &wf).unwrap(),
            TagChange::parse("-absent", &wf).unwrap(),
        ];
        let done = retag(&mut store, &TaskId::from(1), &changes, None, &clock()).unwrap();
        assert_eq!(done.applied, Vec::new());
        assert_eq!(done.task.tags, tags(&["ux"]));
    }

    #[test]
    fn description_text_is_what_the_file_reads_back() {
        assert_eq!(description_text(""), None);
        assert_eq!(description_text(" \n\t\n"), None);
        assert_eq!(description_text("x"), Some("x".into()));
        assert_eq!(description_text("x\n"), Some("x".into()));
        assert_eq!(
            description_text("\n \n  kept\n\nalso kept  \n\n"),
            Some("  kept\n\nalso kept  ".into())
        );
        assert_eq!(description_text("a\r\nb\r\n"), Some("a\nb".into()));
        assert_eq!(description_text("-"), Some("-".into()));
    }

    #[test]
    fn describe_sets_and_clears_the_description() {
        let mut store = store();
        let id = TaskId::from(2);
        let done = describe(&mut store, &id, "Why.\n\nHow.\n", None, &clock()).unwrap();
        assert_eq!(done.changed, ["description"]);
        assert_eq!(done.task.description.as_deref(), Some("Why.\n\nHow."));
        assert_eq!(done.task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), done.task);
        let done = describe(&mut store, &id, "\n", Some("dropped"), &clock()).unwrap();
        assert_eq!(done.task.description, None);
        assert_eq!(done.task.progress, vec![entry("dropped")]);
        assert_eq!(store.get(&id).unwrap(), done.task);
        let err = describe(&mut store, &id, "x", Some(""), &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyNote));
        assert_eq!(store.get(&id).unwrap().description, None);
        let err = describe(&mut store, &TaskId::from(9), "x", None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn describe_refuses_a_line_that_would_start_a_section() {
        let mut store = store();
        let id = TaskId::from(2);
        let err = describe(&mut store, &id, "Intro\n## Steps\n1. x", None, &clock()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "a description line cannot start with '## ' (it would start a new section): ## Steps"
        );
        assert_eq!(store.get(&id).unwrap().description, None);
        // Deeper headings and a bare `##` are plain text.
        let done = describe(&mut store, &id, "### Steps\n##\n #", None, &clock()).unwrap();
        assert_eq!(done.task.description.as_deref(), Some("### Steps\n##\n #"));
    }

    #[test]
    fn describe_read_writes_the_task_it_was_given() {
        let mut store = store();
        let id = TaskId::from(2);
        let read = store.get(&id).unwrap();
        let done = describe_read(&mut store, read.clone(), "Edited.\n", None, &clock()).unwrap();
        assert_eq!(done.changed, ["description"]);
        assert_eq!(done.task.description.as_deref(), Some("Edited."));
        assert_eq!(done.task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), done.task);
        let done = describe_read(&mut store, done.task, "", Some("cleared"), &clock()).unwrap();
        assert_eq!(done.changed, ["description"]);
        assert_eq!(done.task.description, None);
        assert_eq!(done.task.progress, vec![entry("cleared")]);
        assert_eq!(store.get(&id).unwrap(), done.task);
        let err = describe_read(&mut store, read.clone(), "x", Some(" "), &clock()).unwrap_err();
        assert!(matches!(err, EditError::EmptyNote));
        let err = describe_read(&mut store, read, "## x", None, &clock()).unwrap_err();
        assert!(matches!(err, EditError::DescriptionHeading(ref l) if l == "## x"));
        assert_eq!(store.get(&id).unwrap().progress, vec![entry("cleared")]);
    }

    #[test]
    fn describe_read_without_a_change_or_a_note_writes_nothing() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.description = Some("Same.".into());
        let mut store = NoWrite(MemoryStore::new([task.clone()]));
        let done = describe_read(&mut store, task.clone(), "\nSame.\n\n", None, &clock()).unwrap();
        assert_eq!(done.changed, Vec::<&str>::new());
        assert_eq!(done.task, task);
    }

    #[test]
    fn describe_read_refuses_a_note_when_nothing_changed() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.description = Some("Same.".into());
        let mut store = NoWrite(MemoryStore::new([task.clone()]));
        let err =
            describe_read(&mut store, task, "Same.\n", Some("checked"), &clock()).unwrap_err();
        assert!(matches!(err, EditError::NoteNotLogged("description")));
        assert_eq!(
            err.to_string(),
            "description unchanged; the note was not logged (use log to add it alone)"
        );
    }

    #[test]
    fn values_name_their_field() {
        assert_eq!(Value::Status(Status::READY).field(), Field::Status);
        assert_eq!(Value::Priority(Priority::A).field(), Field::Priority);
        assert_eq!(Field::Status.to_string(), "status");
        assert_eq!(Field::Priority.to_string(), "priority");
    }

    #[test]
    fn checked_note_passes_text_through() {
        assert_eq!(checked_note(None).unwrap(), None);
        assert_eq!(checked_note(Some("x")).unwrap(), Some("x"));
        assert!(checked_note(Some("\t")).is_err());
    }
}
