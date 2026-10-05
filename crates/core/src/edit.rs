//! The small edits of the original script, `set`, `log` and `done`, as
//! operations on a [`Store`], plus [`revise`], the TUI form's save.
//!
//! The CLI (`tasq set/log/done`) and the TUI (`t`, `p`, `l`, `d`) both call
//! these, so the two front ends cannot drift (FR-10): a status change is
//! always "read the task, change the field, optionally log the note, write
//! the whole task back", and `done` is always "log the note first, then
//! close with the store's own `done` semantics".

use std::path::PathBuf;

use chrono::NaiveDate;
use thiserror::Error;

use crate::clock::Clock;
use crate::model::{Priority, Status, Tag, Task, TaskId, Workflow};
use crate::store::{Store, StoreError};

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

    /// Changes the field on `task`.
    pub fn apply(&self, task: &mut Task) {
        match self {
            Self::Status(status) => task.set_status(status.clone()),
            Self::Priority(priority) => task.set_priority(*priority),
        }
    }
}

/// The fields the TUI's edit form shows and writes back at once: what
/// [`Fields::of`] reads off a task and what [`revise`] puts on it. The
/// description (multi-line, the editor's job) and the lists are not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields {
    /// The title line.
    pub title: String,
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
pub const FIELD_NAMES: [&str; 6] = ["title", "status", "priority", "due", "project", "tags"];

impl Fields {
    /// The fields as `task` has them.
    pub fn of(task: &Task) -> Self {
        Self {
            title: task.title.clone(),
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
        match &self.status {
            Some(status) => task.set_status(status.clone()),
            None => task.clear_status(),
        }
        task.set_priority(self.priority);
        task.due = self.due;
        task.project.clone_from(&self.project);
        task.tags.clone_from(&self.tags);
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
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `tasq set <id> <value> [note]`: changes the status or priority, logs
/// `note` when given, writes the task and returns it as stored.
pub fn set(
    store: &mut dyn Store,
    id: &TaskId,
    value: &Value,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?;
    let mut task = store.get(id)?;
    value.apply(&mut task);
    if let Some(note) = note {
        task.log(note, clock);
    }
    store.update(&task)?;
    Ok(store.get(id)?)
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

/// `tasq done <id> [note]`: logs `note` when given, then closes the task
/// (`# [x]`, status tag removed) and returns it as stored.
pub fn done(
    store: &mut dyn Store,
    id: &TaskId,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?;
    let mut task = store.get(id)?;
    if let Some(note) = note {
        task.log(note, clock);
        store.update(&task)?;
    }
    store.set_done(id, true)?;
    Ok(store.get(id)?)
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
        .unwrap();
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
        .unwrap();
        assert_eq!(task.priority, Priority::A);
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, vec![entry("urgent")]);
        assert_eq!(store.get(&id).unwrap(), task);
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
        assert_eq!(task.progress, vec![entry("merged")]);
        assert_eq!(store.get(&id).unwrap(), task);

        let task = done(&mut store, &TaskId::from(2), None, &clock()).unwrap();
        assert!(task.done);
        assert_eq!(task.progress, Vec::new());
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
    fn fields_read_off_a_task_and_name_what_differs() {
        let mut task = Task::new(TaskId::from(1), "A");
        task.set_status(Status::READY);
        task.add_tag(Tag::new("gitlab").unwrap());
        let fields = Fields::of(&task);
        assert_eq!(
            fields,
            Fields {
                title: "A".into(),
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
    fn checked_note_passes_text_through() {
        assert_eq!(checked_note(None).unwrap(), None);
        assert_eq!(checked_note(Some("x")).unwrap(), Some("x"));
        assert!(checked_note(Some("\t")).is_err());
    }
}
