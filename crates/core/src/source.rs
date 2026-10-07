//! External sources and reconciliation (plan T-501).
//!
//! A [`Source`] turns something outside the notebook (merge requests
//! awaiting review, assigned issues, an LLM's reading of an inbox) into
//! [`SourceItem`]s. How it does that is its own business (ADR-0004):
//! deterministic API calls, an LLM, or both. The core only reconciles:
//! [`reconcile`] compares the items with the existing tasks and produces
//! [`Change`]s, which [`apply`] writes through a [`Store`]. Both are pure
//! with respect to I/O, so the rules are unit tested and mutation tested.
//!
//! Matching uses the task's `## Source` section ([`Origin`]: source name
//! plus external id) and, for tasks that predate `tasq sync`, a URL found
//! in `## Related` or `### Merge requests`.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::clock::Clock;
use crate::model::{Link, Origin, Priority, Status, Tag, Task, TaskDraft, TaskId};
use crate::store::{Store, StoreError};

/// What the source says about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemState {
    /// Still something to do.
    Open,
    /// Finished elsewhere (merged, closed, reassigned, approved).
    Done,
    /// Still open but something happened the user should look at.
    NeedsAttention,
}

/// One thing a source found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItem {
    /// Stable id within the source (a URL, `group/project!123`, a ticket number).
    pub external_id: String,
    /// Where to look, recorded in `## Source` and `## Related`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Task title to create.
    pub title: String,
    /// `## Description` text for a new task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Open, done or needs attention.
    #[serde(default = "default_state")]
    pub state: ItemState,
    /// Status for a new task; `None` means the source's configured default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// Priority for a new task; `None` means the source's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Extra topic tags on top of the source's configured ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// Due date for a new task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    /// Why the item is done or needs attention (`MR merged`), logged as a note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn default_state() -> ItemState {
    ItemState::Open
}

impl SourceItem {
    /// An open item with only an id and a title.
    pub fn new(external_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            external_id: external_id.into(),
            url: None,
            title: title.into(),
            body: None,
            state: ItemState::Open,
            status: None,
            priority: None,
            tags: Vec::new(),
            due: None,
            note: None,
        }
    }

    /// Sets the URL.
    #[must_use]
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Sets the state and its note.
    #[must_use]
    pub fn with_state(mut self, state: ItemState, note: Option<&str>) -> Self {
        self.state = state;
        self.note = note.map(str::to_owned);
        self
    }
}

/// The state of a tracked item, as `Source::check` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItemState {
    /// Which task's origin this answers for.
    pub origin: Origin,
    /// Its state now.
    pub state: ItemState,
    /// Why (`merged`, `approved by you`, `reassigned`), for the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What a source gets for a sweep.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncContext {
    /// Origins the notebook already tracks for this source.
    pub known: Vec<Origin>,
}

/// Why a source failed. One failing source never stops the others.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SourceError {
    /// Credentials missing or rejected.
    #[error("authentication: {0}")]
    Auth(String),
    /// The remote or the command failed.
    #[error("{0}")]
    Unavailable(String),
    /// The remote or the command answered something unusable.
    #[error("invalid output: {message} (starts with {excerpt:?})")]
    InvalidOutput {
        /// What is wrong.
        message: String,
        /// The first part of the output, for the user to recognise it.
        excerpt: String,
    },
}

/// An external source of tasks.
pub trait Source {
    /// The name from `[[source]] name`, recorded in `## Source`.
    fn name(&self) -> &str;

    /// Everything currently relevant (a full sweep).
    fn fetch(&self, ctx: &SyncContext) -> Result<Vec<SourceItem>, SourceError>;

    /// The current state of specific tracked items.
    fn check(&self, origins: &[Origin]) -> Result<Vec<SourceItemState>, SourceError>;
}

/// What reconciliation is allowed to do for a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Create tasks for new open items.
    pub create_new: bool,
    /// Log a note and mark the task done when its item is done.
    pub close_when_done: bool,
    /// Tag added to a matched open task that lacks it (`review-request`).
    pub flag: Option<Tag>,
}

impl Default for Policy {
    /// Create and close, no flag.
    fn default() -> Self {
        Self {
            create_new: true,
            close_when_done: true,
            flag: None,
        }
    }
}

/// Defaults a source gives new tasks when the item does not say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Defaults {
    /// `[[source]] status`, else `workflow.default_status` (the caller decides).
    pub status: Option<Status>,
    /// `[[source]] tags`.
    pub tags: Vec<Tag>,
}

/// One edit reconciliation asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A new task for a new open item.
    Create {
        /// What to create (origin and `## Related` set). Boxed: a draft is
        /// far larger than the other variants.
        draft: Box<TaskDraft>,
    },
    /// The item is done: log `note` and mark the task done.
    Close {
        /// The task.
        id: TaskId,
        /// The note to log first.
        note: String,
    },
    /// Add `tag` to a matched open task.
    Flag {
        /// The task.
        id: TaskId,
        /// The tag.
        tag: Tag,
    },
    /// The item needs attention: log `note`.
    Note {
        /// The task.
        id: TaskId,
        /// The note.
        note: String,
    },
}

impl Change {
    /// One line for `--dry-run` and the summary.
    pub fn describe(&self) -> String {
        match self {
            Self::Create { draft } => format!("create: {}", draft.title),
            Self::Close { id, note } => format!("[{id}] done: {note}"),
            Self::Flag { id, tag } => format!("[{id}] tag {}", tag.to_hash()),
            Self::Note { id, note } => format!("[{id}] note: {note}"),
        }
    }

    /// The task the change touches, if it exists already.
    pub fn task_id(&self) -> Option<&TaskId> {
        match self {
            Self::Create { .. } => None,
            Self::Close { id, .. } | Self::Flag { id, .. } | Self::Note { id, .. } => Some(id),
        }
    }
}

/// The note prefix every sync edit carries: `sync(<source>): ...`.
pub fn sync_note(source: &str, text: &str) -> String {
    format!("sync({source}): {text}")
}

/// The task tracking `item` for `source`: by origin, else by a URL in
/// `## Related` or `### Merge requests` (tasks older than `tasq sync`).
pub fn find_task<'a>(
    tasks: &'a [Task],
    source: &str,
    external_id: &str,
    url: Option<&str>,
) -> Option<&'a Task> {
    tasks
        .iter()
        .find(|t| {
            t.origin
                .as_ref()
                .is_some_and(|o| o.source == source && o.external_id == external_id)
        })
        .or_else(|| {
            let url = url?;
            tasks.iter().find(|t| {
                t.related
                    .iter()
                    .chain(&t.merge_requests)
                    .any(|link| link.url == url)
            })
        })
}

/// Compares what `source` reports with the existing tasks.
///
/// - A new open item becomes a `Create` (with `policy.create_new`); a new
///   done item is ignored.
/// - A matched open task whose item is done becomes a `Close` (with
///   `policy.close_when_done`); a matched task that is already done gets
///   nothing.
/// - A matched open task gets a `Flag` when `policy.flag` is set and the
///   tag is missing, and a `Note` when the item needs attention.
/// - `states` (from `Source::check`) are matched by origin only and follow
///   the same done / needs-attention rules.
///
/// Items that repeat an external id are taken once, first occurrence wins.
pub fn reconcile(
    source: &str,
    tasks: &[Task],
    items: &[SourceItem],
    states: &[SourceItemState],
    policy: &Policy,
    defaults: &Defaults,
) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for item in items {
        if seen.contains(&item.external_id.as_str()) {
            continue;
        }
        seen.push(&item.external_id);
        match find_task(tasks, source, &item.external_id, item.url.as_deref()) {
            None => {
                if item.state != ItemState::Done && policy.create_new {
                    changes.push(Change::Create {
                        draft: Box::new(draft_for(source, item, defaults)),
                    });
                }
            }
            Some(task) => changes.extend(changes_for_known(
                source,
                task,
                item.state,
                item.note.as_deref(),
                policy,
            )),
        }
    }
    for state in states {
        if state.origin.source != source {
            continue;
        }
        let Some(task) = find_task(
            tasks,
            source,
            &state.origin.external_id,
            state.origin.url.as_deref(),
        ) else {
            continue;
        };
        changes.extend(changes_for_known(
            source,
            task,
            state.state,
            state.note.as_deref(),
            policy,
        ));
    }
    changes
}

fn changes_for_known(
    source: &str,
    task: &Task,
    state: ItemState,
    note: Option<&str>,
    policy: &Policy,
) -> Vec<Change> {
    if task.done {
        return Vec::new();
    }
    let mut out = Vec::new();
    match state {
        ItemState::Done => {
            if policy.close_when_done {
                out.push(Change::Close {
                    id: task.id.clone(),
                    note: sync_note(source, note.unwrap_or("done")),
                });
            }
        }
        ItemState::Open | ItemState::NeedsAttention => {
            if let Some(tag) = &policy.flag
                && !task.has_tag(tag)
            {
                out.push(Change::Flag {
                    id: task.id.clone(),
                    tag: tag.clone(),
                });
            }
            if state == ItemState::NeedsAttention {
                out.push(Change::Note {
                    id: task.id.clone(),
                    note: sync_note(source, note.unwrap_or("needs attention")),
                });
            }
        }
    }
    out
}

/// The draft for a new item: title, body, origin, URL in `## Related`,
/// the source's tags plus the item's, the item's status or the default.
pub fn draft_for(source: &str, item: &SourceItem, defaults: &Defaults) -> TaskDraft {
    let mut draft = TaskDraft::new(item.title.trim())
        .with_status(item.status.clone().or_else(|| defaults.status.clone()))
        .with_note(sync_note(source, "created"))
        .with_origin(Origin {
            source: source.to_owned(),
            external_id: item.external_id.clone(),
            url: item.url.clone(),
        });
    if let Some(priority) = item.priority {
        draft = draft.with_priority(priority);
    }
    if let Some(body) = item
        .body
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        draft = draft.with_description(body);
    }
    if let Some(due) = item.due {
        draft = draft.with_due(due);
    }
    if let Some(url) = &item.url {
        draft = draft.with_related(Link::new(url.as_str()));
    }
    for tag in defaults.tags.iter().chain(&item.tags) {
        draft = draft.with_tag(tag.clone());
    }
    draft
}

/// A change that was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// The task, as created or edited.
    pub id: TaskId,
    /// What happened (`Change::describe`, with the id for creations).
    pub description: String,
}

/// Writes `changes` through `store`. Stops at the first store error; the
/// changes written so far are returned with it.
pub fn apply(
    store: &mut dyn Store,
    changes: &[Change],
    clock: &dyn Clock,
) -> (Vec<Applied>, Option<StoreError>) {
    let mut applied = Vec::new();
    for change in changes {
        match apply_one(store, change, clock) {
            Ok(done) => applied.push(done),
            Err(e) => return (applied, Some(e)),
        }
    }
    (applied, None)
}

fn apply_one(
    store: &mut dyn Store,
    change: &Change,
    clock: &dyn Clock,
) -> Result<Applied, StoreError> {
    match change {
        Change::Create { draft } => {
            let task = store.create((**draft).clone())?;
            Ok(Applied {
                description: format!("[{}] created: {}", task.id, task.title),
                id: task.id,
            })
        }
        Change::Close { id, note } => {
            let mut task = store.get(id)?;
            task.log(note.as_str(), clock);
            task.close(clock);
            store.update(&task)?;
            Ok(Applied {
                id: id.clone(),
                description: change.describe(),
            })
        }
        Change::Flag { id, tag } => {
            let mut task = store.get(id)?;
            task.add_tag(tag.clone());
            store.update(&task)?;
            Ok(Applied {
                id: id.clone(),
                description: change.describe(),
            })
        }
        Change::Note { id, note } => {
            let mut task = store.get(id)?;
            task.log(note.as_str(), clock);
            store.update(&task)?;
            Ok(Applied {
                id: id.clone(),
                description: change.describe(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::Workflow;
    use crate::query::Filter;
    use crate::store::{IdScheme, StoreInfo};

    const SRC: &str = "gitlab-review-requests";

    fn tag(s: &str) -> Tag {
        Tag::new(s).unwrap()
    }

    fn tracked(id: u64, external_id: &str) -> Task {
        let mut t = Task::new(TaskId::from(id), format!("Tracked {external_id}"));
        t.set_status(Status::READY);
        t.origin = Some(Origin {
            source: SRC.into(),
            external_id: external_id.into(),
            url: Some(format!("https://gl/{external_id}")),
        });
        t
    }

    fn defaults() -> Defaults {
        Defaults {
            status: Some(Status::READY),
            tags: vec![tag("gitlab")],
        }
    }

    #[test]
    fn new_open_item_is_created_with_defaults_and_origin() {
        let item = SourceItem::new("g/p!1", "  Review MR !1: Add parser ")
            .with_url("https://gl/g/p/-/merge_requests/1");
        let changes = reconcile(SRC, &[], &[item], &[], &Policy::default(), &defaults());
        let Change::Create { draft } = &changes[0] else {
            panic!("{changes:?}");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(draft.title, "Review MR !1: Add parser");
        assert_eq!(draft.status, Some(Status::READY));
        assert_eq!(draft.priority, Priority::B);
        assert_eq!(draft.tags, vec![tag("gitlab")]);
        assert_eq!(
            draft.related,
            vec![Link::new("https://gl/g/p/-/merge_requests/1")]
        );
        assert_eq!(
            draft.note.as_deref(),
            Some("sync(gitlab-review-requests): created")
        );
        assert_eq!(
            draft.origin,
            Some(Origin {
                source: SRC.into(),
                external_id: "g/p!1".into(),
                url: Some("https://gl/g/p/-/merge_requests/1".into())
            })
        );
        assert_eq!(changes[0].describe(), "create: Review MR !1: Add parser");
        assert_eq!(changes[0].task_id(), None);
    }

    #[test]
    fn item_fields_override_the_defaults() {
        let mut item = SourceItem::new("x", "T");
        item.status = Some(Status::LATER);
        item.priority = Some(Priority::A);
        item.tags = vec![tag("urgent"), tag("gitlab")];
        item.body = Some("  details \n".into());
        item.due = NaiveDate::from_ymd_opt(2026, 12, 1);
        let draft = draft_for(SRC, &item, &defaults());
        assert_eq!(draft.status, Some(Status::LATER));
        assert_eq!(draft.priority, Priority::A);
        assert_eq!(
            draft.tags,
            vec![tag("gitlab"), tag("urgent")],
            "deduplicated"
        );
        assert_eq!(draft.description.as_deref(), Some("details"));
        assert_eq!(draft.due, NaiveDate::from_ymd_opt(2026, 12, 1));
        assert_eq!(draft.related, Vec::new());
        let blank_body = SourceItem {
            body: Some("  ".into()),
            ..item
        };
        assert_eq!(draft_for(SRC, &blank_body, &defaults()).description, None);
    }

    #[test]
    fn done_or_duplicate_new_items_and_create_new_off() {
        let done = SourceItem::new("x", "T").with_state(ItemState::Done, Some("merged"));
        assert_eq!(
            reconcile(SRC, &[], &[done], &[], &Policy::default(), &defaults()),
            Vec::new()
        );
        let twice = [
            SourceItem::new("x", "First"),
            SourceItem::new("x", "Second"),
        ];
        let changes = reconcile(SRC, &[], &twice, &[], &Policy::default(), &defaults());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].describe(), "create: First");
        let flag_only = Policy {
            create_new: false,
            ..Policy::default()
        };
        assert_eq!(
            reconcile(
                SRC,
                &[],
                &[SourceItem::new("x", "T")],
                &[],
                &flag_only,
                &defaults()
            ),
            Vec::new()
        );
    }

    #[test]
    fn known_items_close_flag_and_note() {
        let tasks = vec![tracked(1, "a"), tracked(2, "b"), tracked(3, "c")];
        let items = vec![
            SourceItem::new("a", "A").with_state(ItemState::Done, Some("merged")),
            SourceItem::new("b", "B"),
            SourceItem::new("c", "C").with_state(ItemState::NeedsAttention, None),
        ];
        let policy = Policy {
            flag: Some(tag("review-request")),
            ..Policy::default()
        };
        let changes = reconcile(SRC, &tasks, &items, &[], &policy, &defaults());
        assert_eq!(
            changes,
            vec![
                Change::Close {
                    id: TaskId::from(1),
                    note: "sync(gitlab-review-requests): merged".into()
                },
                Change::Flag {
                    id: TaskId::from(2),
                    tag: tag("review-request")
                },
                Change::Flag {
                    id: TaskId::from(3),
                    tag: tag("review-request")
                },
                Change::Note {
                    id: TaskId::from(3),
                    note: "sync(gitlab-review-requests): needs attention".into()
                },
            ]
        );
        assert_eq!(
            changes[0].describe(),
            "[1] done: sync(gitlab-review-requests): merged"
        );
        assert_eq!(changes[1].describe(), "[2] tag #review-request");
        assert_eq!(
            changes[3].describe(),
            "[3] note: sync(gitlab-review-requests): needs attention"
        );
        assert_eq!(changes[1].task_id(), Some(&TaskId::from(2)));
        // No flag configured and close disabled: an open item changes nothing,
        // a done one neither.
        let passive = Policy {
            close_when_done: false,
            ..Policy::default()
        };
        assert_eq!(
            reconcile(SRC, &tasks, &items[..2], &[], &passive, &defaults()),
            Vec::new()
        );
        // A task that already carries the flag is left alone.
        let mut flagged = tracked(4, "d");
        flagged.add_tag(tag("review-request"));
        assert_eq!(
            reconcile(
                SRC,
                &[flagged],
                &[SourceItem::new("d", "D")],
                &[],
                &policy,
                &defaults()
            ),
            Vec::new()
        );
    }

    #[test]
    fn done_tasks_are_never_touched() {
        let mut t = tracked(1, "a");
        t.mark_done();
        let items = vec![SourceItem::new("a", "A").with_state(ItemState::Done, None)];
        let policy = Policy {
            flag: Some(tag("f")),
            ..Policy::default()
        };
        assert_eq!(
            reconcile(SRC, &[t], &items, &[], &policy, &defaults()),
            Vec::new()
        );
    }

    #[test]
    fn legacy_tasks_match_by_url() {
        let mut legacy = Task::new(TaskId::from(9), "Old");
        legacy.add_merge_request(Link::labelled("https://gl/g/p/-/merge_requests/5", "MR"));
        let mut related_only = Task::new(TaskId::from(10), "Older");
        related_only.add_related(Link::new("https://gl/g/p/-/issues/6"));
        let tasks = vec![legacy, related_only];
        let items = vec![
            SourceItem::new("g/p!5", "Review").with_url("https://gl/g/p/-/merge_requests/5"),
            SourceItem::new("g/p#6", "Issue")
                .with_url("https://gl/g/p/-/issues/6")
                .with_state(ItemState::Done, Some("closed")),
            SourceItem::new("g/p!7", "New").with_url("https://gl/g/p/-/merge_requests/7"),
        ];
        let changes = reconcile(SRC, &tasks, &items, &[], &Policy::default(), &defaults());
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert_eq!(
            changes[0],
            Change::Close {
                id: TaskId::from(10),
                note: "sync(gitlab-review-requests): closed".into()
            }
        );
        assert!(matches!(&changes[1], Change::Create { draft } if draft.title == "New"));
        assert!(find_task(&tasks, SRC, "nope", None).is_none());
        assert!(find_task(&tasks, "other", "g/p!5", None).is_none());
    }

    #[test]
    fn check_states_follow_the_same_rules_and_ignore_other_sources() {
        let tasks = vec![tracked(1, "a"), tracked(2, "b")];
        let states = vec![
            SourceItemState {
                origin: tasks[0].origin.clone().unwrap(),
                state: ItemState::Done,
                note: Some("approved by you".into()),
            },
            SourceItemState {
                origin: Origin {
                    source: "other".into(),
                    external_id: "b".into(),
                    url: None,
                },
                state: ItemState::Done,
                note: None,
            },
            SourceItemState {
                origin: Origin {
                    source: SRC.into(),
                    external_id: "unknown".into(),
                    url: None,
                },
                state: ItemState::Done,
                note: None,
            },
            SourceItemState {
                origin: tasks[1].origin.clone().unwrap(),
                state: ItemState::Open,
                note: None,
            },
        ];
        let changes = reconcile(SRC, &tasks, &[], &states, &Policy::default(), &defaults());
        assert_eq!(
            changes,
            vec![Change::Close {
                id: TaskId::from(1),
                note: "sync(gitlab-review-requests): approved by you".into()
            }]
        );
    }

    #[test]
    fn item_serde_shape() {
        let item = SourceItem::new("x", "T").with_url("https://x");
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(
            json,
            "{\"external_id\":\"x\",\"url\":\"https://x\",\"title\":\"T\",\"state\":\"open\"}"
        );
        let back: SourceItem =
            serde_json::from_str("{\"external_id\":\"y\",\"title\":\"U\"}").unwrap();
        assert_eq!(back.state, ItemState::Open);
        let needs: SourceItem =
            serde_json::from_str("{\"external_id\":\"y\",\"title\":\"U\",\"state\":\"needs-attention\",\"tags\":[\"a\"]}")
                .unwrap();
        assert_eq!(needs.state, ItemState::NeedsAttention);
        assert_eq!(needs.tags, vec![tag("a")]);
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            SourceError::Auth("no token".into()).to_string(),
            "authentication: no token"
        );
        assert_eq!(
            SourceError::Unavailable("timeout".into()).to_string(),
            "timeout"
        );
        assert_eq!(
            SourceError::InvalidOutput {
                message: "expected a JSON array".into(),
                excerpt: "oops".into()
            }
            .to_string(),
            "invalid output: expected a JSON array (starts with \"oops\")"
        );
    }

    // ---- apply, against an in-memory store ----

    #[derive(Default)]
    struct MemStore {
        tasks: Vec<Task>,
        fail_update: bool,
    }

    impl Store for MemStore {
        fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
            Ok(self
                .tasks
                .iter()
                .filter(|t| filter.matches(t))
                .cloned()
                .collect())
        }
        fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
            self.tasks
                .iter()
                .find(|t| &t.id == id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(id.clone()))
        }
        fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
            let id = TaskId::from(self.tasks.len() as u64 + 1);
            let task = draft.into_task(id, &FixedClock::at("2026-10-07 09:30"));
            self.tasks.push(task.clone());
            Ok(task)
        }
        fn update(&mut self, task: &Task) -> Result<(), StoreError> {
            if self.fail_update {
                return Err(StoreError::Unsupported {
                    operation: "update".into(),
                });
            }
            let slot = self
                .tasks
                .iter_mut()
                .find(|t| t.id == task.id)
                .ok_or_else(|| StoreError::NotFound(task.id.clone()))?;
            *slot = task.clone();
            Ok(())
        }
        fn set_done(&mut self, _id: &TaskId, _done: bool) -> Result<(), StoreError> {
            unreachable!("apply goes through update")
        }
        fn describe(&self) -> StoreInfo {
            StoreInfo {
                name: "mem".into(),
                location: "/".into(),
                task_count: self.tasks.len(),
                id_scheme: IdScheme::Stable,
                ids_may_change_on_reconcile: false,
            }
        }
    }

    #[test]
    fn apply_writes_every_change_kind() {
        let mut store = MemStore::default();
        store.tasks.push(tracked(1, "a"));
        store.tasks.push(tracked(2, "b"));
        store.tasks.push(tracked(3, "c"));
        let clock = FixedClock::at("2026-10-07 09:30");
        let changes = vec![
            Change::Create {
                draft: Box::new(draft_for(
                    SRC,
                    &SourceItem::new("n", "New one"),
                    &defaults(),
                )),
            },
            Change::Close {
                id: TaskId::from(1),
                note: "sync(x): merged".into(),
            },
            Change::Flag {
                id: TaskId::from(2),
                tag: tag("review-request"),
            },
            Change::Note {
                id: TaskId::from(3),
                note: "sync(x): look".into(),
            },
        ];
        let (applied, err) = apply(&mut store, &changes, &clock);
        assert!(err.is_none(), "{err:?}");
        assert_eq!(
            applied
                .iter()
                .map(|a| a.description.as_str())
                .collect::<Vec<_>>(),
            vec![
                "[4] created: New one",
                "[1] done: sync(x): merged",
                "[2] tag #review-request",
                "[3] note: sync(x): look"
            ]
        );
        assert_eq!(applied[0].id, TaskId::from(4));
        let closed = store.get(&TaskId::from(1)).unwrap();
        assert!(closed.done);
        assert_eq!(closed.status, None);
        assert_eq!(closed.closed_at, Some(clock.now()));
        assert_eq!(closed.progress.last().unwrap().note, "sync(x): merged");
        assert!(
            store
                .get(&TaskId::from(2))
                .unwrap()
                .has_tag(&tag("review-request"))
        );
        assert_eq!(
            store
                .get(&TaskId::from(3))
                .unwrap()
                .progress
                .last()
                .unwrap()
                .note,
            "sync(x): look"
        );
        assert_eq!(
            store
                .get(&TaskId::from(4))
                .unwrap()
                .origin
                .as_ref()
                .unwrap()
                .source,
            SRC
        );
        assert_eq!(Workflow::default().statuses.len(), 5);
    }

    #[test]
    fn apply_stops_at_the_first_error_and_reports_progress() {
        let mut store = MemStore {
            tasks: vec![tracked(1, "a")],
            fail_update: true,
        };
        let clock = FixedClock::at("2026-10-07 09:30");
        let changes = vec![
            Change::Create {
                draft: Box::new(draft_for(SRC, &SourceItem::new("n", "New"), &defaults())),
            },
            Change::Note {
                id: TaskId::from(1),
                note: "n".into(),
            },
            Change::Note {
                id: TaskId::from(1),
                note: "never".into(),
            },
        ];
        let (applied, err) = apply(&mut store, &changes, &clock);
        assert_eq!(applied.len(), 1);
        assert!(matches!(err, Some(StoreError::Unsupported { .. })));
        let (applied, err) = apply(
            &mut store,
            &[Change::Flag {
                id: TaskId::from(99),
                tag: tag("x"),
            }],
            &clock,
        );
        assert_eq!(applied, Vec::new());
        assert!(matches!(err, Some(StoreError::NotFound(_))));
    }
}
