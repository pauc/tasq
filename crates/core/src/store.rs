//! The [`Store`] extension trait: where tasks live.
//!
//! A store maps opaque [`TaskId`]s to [`Task`]s and persists whole tasks.
//! The first implementation is the nb-compatible notebook store in
//! `tasq-store-nb`; the CLI and TUI only ever talk to this trait, so a
//! database-backed store can be added without touching them (ADR-0002,
//! ADR-0003).
//!
//! Reads are infallible with respect to concurrency: a store may be read by
//! several processes at once. Writes are whole-task: [`Store::update`] takes
//! the complete task as the caller wants it on disk, and the store works out
//! which edits produce it. A store that detects the task changed since it
//! was read fails with [`StoreError::Conflict`] instead of overwriting.
//! [`Store::update_many`] writes several tasks as one batch (the nb store
//! makes it one commit) and stops at the first failure, reporting which
//! tasks were already written.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::clock::FixedClock;
use crate::doctor::Check;
use crate::format::FormatError;
use crate::model::{Task, TaskDraft, TaskId};
use crate::query::Filter;

/// Why a store operation failed.
#[derive(Debug, Error)]
pub enum StoreError {
    /// No task has this id (or the id points at something that is not a task).
    #[error("no task with id {0}")]
    NotFound(TaskId),
    /// The task changed on disk since it was read; nothing was written.
    #[error("task {id} changed since it was read ({}); re-read it and retry", path.display())]
    Conflict {
        /// The id of the task.
        id: TaskId,
        /// The file that changed.
        path: PathBuf,
    },
    /// A filesystem error on `path`.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A task file could not be parsed.
    #[error("{}: {source}", path.display())]
    Format {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: FormatError,
    },
    /// A configuration value could not be resolved (a notebook that does not
    /// exist, no home directory, ...).
    #[error("{setting}{}: {message}", file.as_deref().map(|f| format!(" (set in {})", f.display())).unwrap_or_default())]
    Config {
        /// The dotted config key, for example `store.notebook`.
        setting: &'static str,
        /// The configuration file that set it, when known.
        file: Option<PathBuf>,
        /// What went wrong.
        message: String,
    },
    /// The store's index is missing or unusable.
    #[error("{}: {message}", path.display())]
    Index {
        /// The index file.
        path: PathBuf,
        /// What went wrong and how to fix it.
        message: String,
    },
    /// The task was written but the bookkeeping that follows a write (index
    /// registration, commit) failed. The data is on disk; the message says
    /// how to repair the bookkeeping by hand.
    #[error("bookkeeping failed after the write: {message}")]
    Bookkeeping {
        /// What failed and the manual fix.
        message: String,
    },
    /// The store cannot perform this operation (yet or at all).
    #[error("unsupported: {operation}")]
    Unsupported {
        /// What was asked.
        operation: String,
    },
}

/// Why a batch write ([`Store::update_many`]) stopped, and how far it got.
///
/// The tasks in `written` are on disk as requested; the task the error is
/// about and every task after it were left untouched.
#[derive(Debug, Error)]
#[error("{error}{}", written_suffix(written))]
pub struct BatchError {
    /// The ids written before the failure, in batch order.
    pub written: Vec<TaskId>,
    /// What stopped the batch.
    #[source]
    pub error: StoreError,
}

/// The ` (tasks already written: 1, 4)` part of a [`BatchError`] message,
/// empty when nothing was written.
fn written_suffix(written: &[TaskId]) -> String {
    if written.is_empty() {
        return String::new();
    }
    let ids: Vec<String> = written.iter().map(ToString::to_string).collect();
    format!(" (tasks already written: {})", ids.join(", "))
}

/// How a store assigns ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdScheme {
    /// An id names the same task forever.
    Stable,
    /// An id is a position (nb: the line number in `.index`) and can move
    /// when the store is reorganised.
    Positional,
}

/// What a store says about itself (`tasq store info`, `tasq doctor`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreInfo {
    /// Implementation name (`nb`).
    pub name: String,
    /// Where the tasks live.
    pub location: PathBuf,
    /// Number of tasks, open and done.
    pub task_count: usize,
    /// How ids behave.
    pub id_scheme: IdScheme,
    /// Whether ids can change when the store's index is rebuilt; the CLI
    /// warns about it.
    pub ids_may_change_on_reconcile: bool,
    /// The strategy that does the bookkeeping after a write (nb: `native`,
    /// or `none` in tests), for stores that have one. Left out of the JSON
    /// when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookkeeper: Option<String>,
}

/// Task persistence.
pub trait Store {
    /// The tasks matching `filter`, in store order.
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError>;

    /// One task by id.
    fn get(&self, id: &TaskId) -> Result<Task, StoreError>;

    /// Creates a task from `draft`, assigning the id, and returns it.
    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError>;

    /// Writes `task` as a whole: after a successful call, reading `task.id`
    /// back gives exactly `task`. The store diffs it against what is stored
    /// and applies the edits it knows how to make; a change it cannot express
    /// fails with [`StoreError::Unsupported`] and writes nothing. Fails with
    /// [`StoreError::Conflict`] when the stored task changed since it was read.
    fn update(&mut self, task: &Task) -> Result<(), StoreError>;

    /// Writes every task in `tasks`, in order, as one batch: the same
    /// contract as [`update`](Self::update) per task, including the
    /// [`StoreError::Conflict`] check. The first failure stops the batch; the
    /// error carries the ids already written so the caller knows the state
    /// of the store. An empty batch writes nothing.
    ///
    /// The default implementation calls `update` once per task. A store
    /// with per-write bookkeeping overrides it to do that bookkeeping once
    /// (nb: one lock, one commit).
    fn update_many(&mut self, tasks: &[Task]) -> Result<(), BatchError> {
        let mut written = Vec::with_capacity(tasks.len());
        for task in tasks {
            if let Err(error) = self.update(task) {
                return Err(BatchError { written, error });
            }
            written.push(task.id.clone());
        }
        Ok(())
    }

    /// Marks the task done (`true`) or reopens it (`false`), with the store's
    /// own `done` semantics (nb: `# [x]` plus the status tag removed).
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError>;

    /// Location, size and id semantics.
    fn describe(&self) -> StoreInfo;

    /// Hands over, and clears, what the user should hear about that did not
    /// stop a command (nb: an index rebuilt on open, a background commit
    /// that failed). The CLI prints each one as a warning.
    fn take_warnings(&mut self) -> Vec<String>;

    /// The store's part of `tasq doctor`: one [`Check`] per thing worth
    /// verifying about the store itself (nb: the notebook, its index, the
    /// bookkeeper, the git identity), in display order.
    fn doctor(&self) -> Vec<Check>;

    /// The file holding `id`, for stores that keep one file per task (the
    /// TUI opens it in `$EDITOR`). `None` when the store has no such file;
    /// [`StoreError::NotFound`] when there is no such task.
    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        self.get(id).map(|_| None)
    }
}

/// A boxed store is a store: the CLI opens its store as a `Box<dyn Store>`
/// and hands it to functions taking `&mut dyn Store` or `&dyn Store`.
impl<S: Store + ?Sized> Store for Box<S> {
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        (**self).list(filter)
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        (**self).get(id)
    }

    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        (**self).create(draft)
    }

    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        (**self).update(task)
    }

    fn update_many(&mut self, tasks: &[Task]) -> Result<(), BatchError> {
        (**self).update_many(tasks)
    }

    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        (**self).set_done(id, done)
    }

    fn describe(&self) -> StoreInfo {
        (**self).describe()
    }

    fn take_warnings(&mut self) -> Vec<String> {
        (**self).take_warnings()
    }

    fn doctor(&self) -> Vec<Check> {
        (**self).doctor()
    }

    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        (**self).file_of(id)
    }
}

/// A [`Store`] that keeps its tasks in memory: the test double for every
/// front end, and a scratch store for tools that assemble tasks without a
/// notebook. Ids are stable and assigned on creation as `1`, `2`, ...
/// after the highest numeric id present. New tasks' notes are stamped with
/// the injected clock. Warnings are whatever a test queued with
/// [`MemoryStore::warn`].
#[derive(Debug, Clone)]
pub struct MemoryStore {
    tasks: Vec<Task>,
    clock: FixedClock,
    warnings: Vec<String>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new([])
    }
}

impl MemoryStore {
    /// A store holding `tasks`, with its clock at 2026-01-01 09:00.
    pub fn new(tasks: impl IntoIterator<Item = Task>) -> Self {
        Self {
            tasks: tasks.into_iter().collect(),
            clock: FixedClock::at("2026-01-01 09:00"),
            warnings: Vec::new(),
        }
    }

    /// Queues `message` for the next [`Store::take_warnings`], so a front
    /// end's warning output can be tested.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    /// Uses `clock` to stamp the first note of created tasks.
    #[must_use]
    pub fn with_clock(mut self, clock: FixedClock) -> Self {
        self.clock = clock;
        self
    }

    /// Every task, open and done, in store order.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    fn position(&self, id: &TaskId) -> Result<usize, StoreError> {
        self.tasks
            .iter()
            .position(|t| t.id == *id)
            .ok_or_else(|| StoreError::NotFound(id.clone()))
    }

    fn next_id(&self) -> TaskId {
        let highest = self
            .tasks
            .iter()
            .filter_map(|t| t.id.as_str().parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        TaskId::from(highest + 1)
    }
}

impl Store for MemoryStore {
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        Ok(self
            .tasks
            .iter()
            .filter(|t| filter.matches(t))
            .cloned()
            .collect())
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        Ok(self.tasks[self.position(id)?].clone())
    }

    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        let task = draft.into_task(self.next_id(), &self.clock);
        self.tasks.push(task.clone());
        Ok(task)
    }

    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        let at = self.position(&task.id)?;
        self.tasks[at] = task.clone();
        Ok(())
    }

    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        let at = self.position(id)?;
        if done {
            self.tasks[at].mark_done();
        } else {
            self.tasks[at].done = false;
            self.tasks[at].closed_at = None;
        }
        Ok(())
    }

    fn describe(&self) -> StoreInfo {
        StoreInfo {
            name: "memory".to_owned(),
            location: PathBuf::from("memory"),
            task_count: self.tasks.len(),
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            bookkeeper: None,
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// One check: how many tasks are held.
    fn doctor(&self) -> Vec<Check> {
        vec![Check::ok(
            "memory",
            format!("{} task(s) held in memory", self.tasks.len()),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io(kind: std::io::ErrorKind) -> std::io::Error {
        std::io::Error::new(kind, "boom")
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            StoreError::NotFound(TaskId::from(3)).to_string(),
            "no task with id 3"
        );
        assert_eq!(
            StoreError::Conflict {
                id: TaskId::from(3),
                path: "/nb/home/x.todo.md".into(),
            }
            .to_string(),
            "task 3 changed since it was read (/nb/home/x.todo.md); re-read it and retry"
        );
        assert_eq!(
            StoreError::Io {
                path: "/nb/.index".into(),
                source: io(std::io::ErrorKind::NotFound),
            }
            .to_string(),
            "/nb/.index: boom"
        );
        assert_eq!(
            StoreError::Format {
                path: "/nb/notes.md".into(),
                source: FormatError::NotATask {
                    first_line: "# Notes".into(),
                },
            }
            .to_string(),
            "/nb/notes.md: not a task: first line \"# Notes\" is not '# [ ] Title' or '# [x] Title'"
        );
        assert_eq!(
            StoreError::Config {
                setting: "store.notebook",
                file: Some("/etc/tasq.toml".into()),
                message: "notebook \"work\" not found".into(),
            }
            .to_string(),
            "store.notebook (set in /etc/tasq.toml): notebook \"work\" not found"
        );
        assert_eq!(
            StoreError::Config {
                setting: "store.notebook",
                file: None,
                message: "m".into(),
            }
            .to_string(),
            "store.notebook: m"
        );
        assert_eq!(
            StoreError::Index {
                path: "/nb/.index".into(),
                message: "missing".into(),
            }
            .to_string(),
            "/nb/.index: missing"
        );
        assert_eq!(
            StoreError::Bookkeeping {
                message: "run nb index reconcile".into(),
            }
            .to_string(),
            "bookkeeping failed after the write: run nb index reconcile"
        );
        assert_eq!(
            StoreError::Unsupported {
                operation: "create".into(),
            }
            .to_string(),
            "unsupported: create"
        );
    }

    #[test]
    fn errors_expose_their_sources() {
        use std::error::Error;
        let e = StoreError::Io {
            path: "/p".into(),
            source: io(std::io::ErrorKind::Other),
        };
        assert!(e.source().is_some());
        let e = StoreError::Format {
            path: "/p".into(),
            source: FormatError::NotATask {
                first_line: String::new(),
            },
        };
        assert!(e.source().is_some());
        assert!(StoreError::NotFound(TaskId::from(1)).source().is_none());
    }

    #[test]
    fn store_info_serde() {
        let info = StoreInfo {
            name: "nb".into(),
            location: "/nb/home".into(),
            task_count: 4,
            id_scheme: IdScheme::Positional,
            ids_may_change_on_reconcile: true,
            bookkeeper: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            "{\"name\":\"nb\",\"location\":\"/nb/home\",\"task_count\":4,\"id_scheme\":\"positional\",\"ids_may_change_on_reconcile\":true}",
            "no bookkeeper: the key is left out"
        );
        let back: StoreInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
        let with_bookkeeper = StoreInfo {
            bookkeeper: Some("native".to_owned()),
            ..info
        };
        let json = serde_json::to_string(&with_bookkeeper).unwrap();
        assert!(json.ends_with(",\"bookkeeper\":\"native\"}"), "{json}");
        let back: StoreInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, with_bookkeeper);
        assert_eq!(
            serde_json::to_string(&IdScheme::Stable).unwrap(),
            "\"stable\""
        );
    }

    mod memory {
        use super::*;
        use crate::model::{Priority, Status};

        fn store() -> MemoryStore {
            let mut a = Task::new(TaskId::from(1), "A");
            a.set_status(Status::READY);
            let mut done = Task::new(TaskId::from(3), "Old");
            done.mark_done();
            MemoryStore::new([a, done])
        }

        #[test]
        fn list_applies_the_filter_in_store_order() {
            let store = store();
            let open = store.list(&Filter::default()).unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(open[0].id, TaskId::from(1));
            let all = store.list(&Filter::default().any_done()).unwrap();
            assert_eq!(all, store.tasks().to_vec());
            assert_eq!(all[1].id, TaskId::from(3));
            assert_eq!(store.list(&Filter::default().done(true)).unwrap().len(), 1);
            assert_eq!(MemoryStore::default().tasks(), &[] as &[Task]);
        }

        #[test]
        fn get_finds_by_id() {
            let store = store();
            assert_eq!(store.get(&TaskId::from(3)).unwrap().title, "Old");
            assert_eq!(
                store.get(&TaskId::from(2)).unwrap_err().to_string(),
                "no task with id 2"
            );
        }

        #[test]
        fn create_assigns_the_next_numeric_id_and_stamps_the_note() {
            let mut store = store().with_clock(FixedClock::at("2026-10-04 10:15"));
            let task = store
                .create(TaskDraft::new("New").with_note("first"))
                .unwrap();
            assert_eq!(task.id, TaskId::from(4));
            assert_eq!(task.title, "New");
            assert_eq!(task.progress.len(), 1);
            assert_eq!(
                crate::clock::format_timestamp(task.progress[0].at.date_time()),
                "2026-10-04 10:15"
            );
            assert_eq!(store.get(&TaskId::from(4)).unwrap(), task);
            assert_eq!(store.tasks().len(), 3);
            let second = store.create(TaskDraft::new("Again")).unwrap();
            assert_eq!(second.id, TaskId::from(5));
            assert_eq!(second.progress, Vec::new());

            let mut text_ids = MemoryStore::new([Task::new(TaskId::new("abc").unwrap(), "x")]);
            assert_eq!(
                text_ids.create(TaskDraft::new("y")).unwrap().id,
                TaskId::from(1)
            );
            let mut empty = MemoryStore::default();
            assert_eq!(
                empty.create(TaskDraft::new("y")).unwrap().id,
                TaskId::from(1)
            );
            assert_eq!(
                crate::clock::format_timestamp(
                    empty
                        .create(TaskDraft::new("z").with_note("n"))
                        .unwrap()
                        .progress[0]
                        .at
                        .date_time()
                ),
                "2026-01-01 09:00"
            );
        }

        #[test]
        fn update_replaces_the_whole_task() {
            let mut store = store();
            let mut task = store.get(&TaskId::from(1)).unwrap();
            task.set_priority(Priority::A);
            task.title = "Renamed".to_owned();
            store.update(&task).unwrap();
            assert_eq!(store.get(&TaskId::from(1)).unwrap(), task);
            assert_eq!(store.tasks().len(), 2);
            assert_eq!(store.tasks()[1].title, "Old");
            let stranger = Task::new(TaskId::from(7), "?");
            assert_eq!(
                store.update(&stranger).unwrap_err().to_string(),
                "no task with id 7"
            );
            assert_eq!(store.tasks().len(), 2);
        }

        #[test]
        fn update_many_writes_every_task_in_order() {
            let mut store = store();
            let mut a = store.get(&TaskId::from(1)).unwrap();
            a.title = "A, renamed".to_owned();
            let mut old = store.get(&TaskId::from(3)).unwrap();
            old.set_priority(Priority::C);
            store.update_many(&[a.clone(), old.clone()]).unwrap();
            assert_eq!(store.tasks(), &[a.clone(), old.clone()]);
            store.update_many(&[]).unwrap();
            assert_eq!(store.tasks(), &[a, old]);
        }

        #[test]
        fn update_many_stops_at_the_first_error_and_reports_what_it_wrote() {
            let mut store = store();
            let mut a = store.get(&TaskId::from(1)).unwrap();
            a.title = "A, renamed".to_owned();
            let mut old = store.get(&TaskId::from(3)).unwrap();
            old.title = "never written".to_owned();
            let stranger = Task::new(TaskId::from(7), "?");
            let err = store.update_many(&[a.clone(), stranger, old]).unwrap_err();
            assert_eq!(err.written, vec![TaskId::from(1)]);
            assert!(matches!(&err.error, StoreError::NotFound(id) if *id == TaskId::from(7)));
            assert_eq!(
                err.to_string(),
                "no task with id 7 (tasks already written: 1)"
            );
            assert_eq!(store.tasks()[0], a);
            assert_eq!(store.tasks()[1].title, "Old");

            let first = store
                .update_many(&[Task::new(TaskId::from(9), "?")])
                .unwrap_err();
            assert_eq!(first.written, Vec::new());
            assert_eq!(first.to_string(), "no task with id 9");
        }

        #[test]
        fn batch_error_exposes_its_source() {
            use std::error::Error;
            let err = BatchError {
                written: vec![TaskId::from(1), TaskId::from(4)],
                error: StoreError::NotFound(TaskId::from(2)),
            };
            assert_eq!(
                err.to_string(),
                "no task with id 2 (tasks already written: 1, 4)"
            );
            assert_eq!(err.source().unwrap().to_string(), "no task with id 2");
        }

        #[test]
        fn set_done_closes_and_reopens() {
            let mut store = store();
            let mut closed = store.get(&TaskId::from(1)).unwrap();
            closed.close(&crate::clock::FixedClock::at("2026-10-07 14:32"));
            store.update(&closed).unwrap();
            store.set_done(&TaskId::from(1), true).unwrap();
            let task = store.get(&TaskId::from(1)).unwrap();
            assert!(task.done);
            assert_eq!(task.status, None);
            store.set_done(&TaskId::from(1), false).unwrap();
            let task = store.get(&TaskId::from(1)).unwrap();
            assert!(!task.done);
            assert_eq!(task.status, None);
            assert_eq!(task.closed_at, None, "an open task has no closing time");
            assert_eq!(
                store
                    .set_done(&TaskId::from(9), true)
                    .unwrap_err()
                    .to_string(),
                "no task with id 9"
            );
        }

        #[test]
        fn file_of_is_none_for_existing_tasks_only() {
            let store = store();
            assert_eq!(store.file_of(&TaskId::from(1)).unwrap(), None);
            assert_eq!(
                store.file_of(&TaskId::from(2)).unwrap_err().to_string(),
                "no task with id 2"
            );
        }

        #[test]
        fn describe() {
            let info = store().describe();
            assert_eq!(
                info,
                StoreInfo {
                    name: "memory".to_owned(),
                    location: PathBuf::from("memory"),
                    task_count: 2,
                    id_scheme: IdScheme::Stable,
                    ids_may_change_on_reconcile: false,
                    bookkeeper: None,
                }
            );
        }

        #[test]
        fn warnings_are_queued_until_taken() {
            let mut store = store();
            assert_eq!(store.take_warnings(), Vec::<String>::new());
            store.warn("first");
            store.warn("second");
            assert_eq!(
                store.take_warnings(),
                vec!["first".to_owned(), "second".to_owned()]
            );
            assert_eq!(
                store.take_warnings(),
                Vec::<String>::new(),
                "taken means cleared"
            );
        }

        #[test]
        fn doctor_counts_the_tasks() {
            assert_eq!(
                store().doctor(),
                vec![Check::ok("memory", "2 task(s) held in memory")]
            );
            assert_eq!(
                MemoryStore::default().doctor(),
                vec![Check::ok("memory", "0 task(s) held in memory")]
            );
        }

        /// A recording store: every call is logged, so the boxed forwarding
        /// is checked call by call, with the arguments it passed on.
        #[derive(Default)]
        struct Recording {
            inner: MemoryStore,
            calls: std::cell::RefCell<Vec<String>>,
        }

        impl Recording {
            fn note(&self, call: String) {
                self.calls.borrow_mut().push(call);
            }
        }

        impl Store for Recording {
            fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
                self.note("list".to_owned());
                self.inner.list(filter)
            }
            fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
                self.note(format!("get {id}"));
                self.inner.get(id)
            }
            fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
                self.note("create".to_owned());
                self.inner.create(draft)
            }
            fn update(&mut self, task: &Task) -> Result<(), StoreError> {
                self.note(format!("update {}", task.id));
                self.inner.update(task)
            }
            fn update_many(&mut self, tasks: &[Task]) -> Result<(), BatchError> {
                self.note(format!("update_many {}", tasks.len()));
                self.inner.update_many(tasks)
            }
            fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
                self.note(format!("set_done {id} {done}"));
                self.inner.set_done(id, done)
            }
            fn describe(&self) -> StoreInfo {
                self.note("describe".to_owned());
                StoreInfo {
                    name: "recording".to_owned(),
                    ..self.inner.describe()
                }
            }
            fn take_warnings(&mut self) -> Vec<String> {
                self.note("take_warnings".to_owned());
                vec!["w".to_owned()]
            }
            fn doctor(&self) -> Vec<Check> {
                self.note("doctor".to_owned());
                vec![Check::ok("c", "fine")]
            }
            fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
                self.note(format!("file_of {id}"));
                Ok(Some(PathBuf::from(format!("/{id}.md"))))
            }
        }

        #[test]
        fn a_box_forwards_every_call() {
            let mut boxed = Box::new(Recording::default());
            let created = boxed.create(TaskDraft::new("Boxed")).unwrap();
            assert_eq!(created.id, TaskId::from(1));
            assert_eq!(boxed.get(&created.id).unwrap().title, "Boxed");
            assert_eq!(boxed.list(&Filter::new()).unwrap().len(), 1);
            let mut renamed = created.clone();
            renamed.title = "Renamed".to_owned();
            boxed.update(&renamed).unwrap();
            assert_eq!(boxed.get(&created.id).unwrap().title, "Renamed");
            boxed.update_many(&[renamed.clone(), renamed]).unwrap();
            boxed.set_done(&created.id, true).unwrap();
            assert!(boxed.get(&created.id).unwrap().done);
            assert_eq!(boxed.describe().name, "recording");
            assert_eq!(boxed.take_warnings(), vec!["w".to_owned()]);
            assert_eq!(boxed.doctor(), vec![Check::ok("c", "fine")]);
            assert_eq!(
                boxed.file_of(&created.id).unwrap(),
                Some(PathBuf::from("/1.md"))
            );
            assert_eq!(
                boxed.calls.borrow().join("; "),
                "create; get 1; list; update 1; get 1; update_many 2; set_done 1 true; get 1; \
                 describe; take_warnings; doctor; file_of 1"
            );
        }
    }
}
