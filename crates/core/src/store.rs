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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

    /// Marks the task done (`true`) or reopens it (`false`), with the store's
    /// own `done` semantics (nb: `# [x]` plus the status tag removed).
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError>;

    /// Location, size and id semantics.
    fn describe(&self) -> StoreInfo;
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
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            "{\"name\":\"nb\",\"location\":\"/nb/home\",\"task_count\":4,\"id_scheme\":\"positional\",\"ids_may_change_on_reconcile\":true}"
        );
        let back: StoreInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
        assert_eq!(
            serde_json::to_string(&IdScheme::Stable).unwrap(),
            "\"stable\""
        );
    }
}
