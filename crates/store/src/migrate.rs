//! Converting an nb notebook into a store directory (ADR-0026), in two
//! steps: [`plan`] reads the notebook and decides every file, writing
//! nothing; [`Plan::write`] writes the files into the target.
//!
//! Every task of the notebook, open and done, keeps its nb id: the line of
//! its file in `.index`. A todo file that `.index` does not list gets the
//! id nb's reconcile would have given it, the next line after the index, in
//! name order. The file name is a ULID minted from the nb file's second
//! (`YYYYMMDDHHMMSS` at the start of its name, local time), falling back to
//! the time of the first progress entry, then to the clock. The text is a
//! fresh v2 document of the task ([`Document::from_task_v2`]), so a done
//! task says `status: done` and keeps its `## Closed` time as `closed`; a
//! done task closed without one (by `nb todo do` or the original script,
//! which never wrote it) is closed at its last progress entry. Sections the
//! format does not know are carried over verbatim at the end; text the
//! projection does not read inside a known section, or between the title
//! and the first section, is not.
//! The notebook is only read; nothing runs `nb` or `git`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use tasq_core::clock::Clock;
use tasq_core::format::{self, Document, FormatError, section};
use tasq_core::model::{Task, TaskId, Workflow};
use thiserror::Error;

use crate::lock::instant_at;
use crate::store::{MAX_NAME_ATTEMPTS, unix_millis};
use crate::ulid::Ulid;

/// nb's index file, one file name per line, the id is the line number.
pub const INDEX: &str = ".index";

/// Suffix of a todo file in an nb notebook.
const TODO_SUFFIX: &str = ".todo.md";

/// Why a migration could not be planned or written.
#[derive(Debug, Error)]
pub enum MigrateError {
    /// The notebook directory has no `.index`.
    #[error("{}: not an nb notebook (no {INDEX})", path.display())]
    NotANotebook {
        /// The notebook directory.
        path: PathBuf,
    },
    /// The target exists and has entries in it.
    #[error("{}: the target exists and is not empty", path.display())]
    TargetNotEmpty {
        /// The target directory.
        path: PathBuf,
    },
    /// Reading the notebook or writing the target failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A todo file of the notebook could not be read as a task.
    #[error("{}: {source}", path.display())]
    Format {
        /// The nb file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: FormatError,
    },
    /// Every ULID minted for a task named a file the plan already holds.
    #[error("{}: no free file name after {MAX_NAME_ATTEMPTS} attempts", path.display())]
    NoFreeName {
        /// The nb file.
        path: PathBuf,
    },
}

/// One task of the migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The nb id, kept as the task's id.
    pub id: TaskId,
    /// The nb file it comes from.
    pub from: PathBuf,
    /// The `<ULID>.md` name it gets in the target.
    pub file: String,
    /// The task's title.
    pub title: String,
    /// The creation time the file name carries (local time).
    pub created: NaiveDateTime,
    /// The v2 file.
    text: String,
}

impl Entry {
    /// The v2 file this entry writes.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A migration decided and not written: the target and one entry per task,
/// in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    target: PathBuf,
    entries: Vec<Entry>,
}

impl Plan {
    /// The directory the files go to.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The tasks, in id order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Writes every file into the target, creating it. The target is
    /// checked again first (absent or empty), and no file is overwritten.
    /// Not atomic: a failure part way leaves the files written so far,
    /// and the target then has to be emptied before a second attempt.
    pub fn write(&self) -> Result<(), MigrateError> {
        check_target(&self.target)?;
        let io_error = |path: &Path| {
            let path = path.to_owned();
            move |source| MigrateError::Io { path, source }
        };
        std::fs::create_dir_all(&self.target).map_err(io_error(&self.target))?;
        for entry in &self.entries {
            let path = self.target.join(&entry.file);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| file.write_all(entry.text.as_bytes()))
                .map_err(io_error(&path))?;
        }
        Ok(())
    }
}

/// Plans the migration of `notebook` into `target`, writing nothing.
/// `target` must not exist or be an empty directory. `workflow` says which
/// tags are statuses; `clock` is the creation time of a task whose time
/// cannot be told otherwise; `random` gives the random part of each ULID.
/// An index line whose file is gone, or that names a file that is not a
/// todo or whose first line is not a task title, is skipped, as the nb
/// store skips it.
pub fn plan(
    notebook: &Path,
    target: &Path,
    workflow: &Workflow,
    clock: &dyn Clock,
    random: &mut dyn FnMut() -> u128,
) -> Result<Plan, MigrateError> {
    check_target(target)?;
    let mut entries: Vec<Entry> = Vec::new();
    for (id, name) in todo_files(notebook)? {
        let path = notebook.join(&name);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::IsADirectory
                ) =>
            {
                continue;
            }
            Err(source) => return Err(MigrateError::Io { path, source }),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return Err(MigrateError::Io {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "not valid UTF-8; the migration would replace the invalid bytes",
                ),
            });
        };
        let mut parsed = match format::parse(&text, id, workflow) {
            Ok(parsed) => parsed,
            Err(FormatError::NotATask { .. }) => continue,
            Err(source) => return Err(MigrateError::Format { path, source }),
        };
        if parsed.task.done && parsed.task.closed_at.is_none() {
            parsed.task.closed_at = parsed.task.progress.last().map(|e| e.at.date_time());
        }
        let created = created_at(&name, &parsed.task).unwrap_or_else(|| clock.now());
        let millis = unix_millis(instant_at(created));
        let file = std::iter::repeat_with(|| format!("{}.md", Ulid::from_parts(millis, random())))
            .take(MAX_NAME_ATTEMPTS as usize)
            .find(|file| entries.iter().all(|e| e.file != *file))
            .ok_or_else(|| MigrateError::NoFreeName { path: path.clone() })?;
        entries.push(Entry {
            text: v2_text(&parsed.document, &parsed.task),
            id: parsed.task.id,
            from: path,
            file,
            title: parsed.task.title,
            created,
        });
    }
    Ok(Plan {
        target: target.to_owned(),
        entries,
    })
}

/// `Ok` when `target` does not exist or is an empty directory.
fn check_target(target: &Path) -> Result<(), MigrateError> {
    match std::fs::read_dir(target).map(|mut listing| listing.next().is_some()) {
        Ok(true) => Err(MigrateError::TargetNotEmpty {
            path: target.to_owned(),
        }),
        Ok(false) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(MigrateError::Io {
            path: target.to_owned(),
            source,
        }),
    }
}

/// `(id, name)` of every todo file of the notebook: the `.index` lines that
/// name one, then the todo files `.index` does not list, in name order,
/// numbered after its last line.
fn todo_files(notebook: &Path) -> Result<Vec<(TaskId, String)>, MigrateError> {
    let index_path = notebook.join(INDEX);
    let index = match std::fs::read_to_string(&index_path) {
        Ok(index) => index,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(MigrateError::NotANotebook {
                path: notebook.to_owned(),
            });
        }
        Err(source) => {
            return Err(MigrateError::Io {
                path: index_path,
                source,
            });
        }
    };
    let listed: Vec<&str> = index
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let io_error = |source| MigrateError::Io {
        path: notebook.to_owned(),
        source,
    };
    let mut unlisted = Vec::new();
    for entry in std::fs::read_dir(notebook).map_err(io_error)? {
        let name = entry
            .map_err(io_error)?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if is_todo(&name) && !listed.contains(&name.as_str()) {
            unlisted.push(name);
        }
    }
    unlisted.sort();
    Ok(listed
        .iter()
        .map(|name| (*name).to_owned())
        .chain(unlisted)
        .zip(1u64..)
        .filter(|(name, _)| is_todo(name))
        .map(|(name, line)| (TaskId::from(line), name))
        .collect())
}

/// Whether `name` is a todo file (`*.todo.md`).
fn is_todo(name: &str) -> bool {
    name.len() > TODO_SUFFIX.len() && name.ends_with(TODO_SUFFIX)
}

/// The creation time of the task in nb file `name`: the `YYYYMMDDHHMMSS`
/// its name starts with, else the time of its first progress entry (`00:00`
/// for a date-only one), else `None`.
fn created_at(name: &str, task: &Task) -> Option<NaiveDateTime> {
    name.get(..14)
        .filter(|stamp| stamp.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|stamp| NaiveDateTime::parse_from_str(stamp, "%Y%m%d%H%M%S").ok())
        .or_else(|| task.progress.first().map(|entry| entry.at.date_time()))
}

/// The v2 file of `task`, read from the v1 `document`: the canonical v2
/// document, then every section of `document` the format does not know,
/// verbatim, each after a blank line.
fn v2_text(document: &Document, task: &Task) -> String {
    let mut text = format::render(&Document::from_task_v2(task));
    for unknown in document
        .sections()
        .into_iter()
        .filter(|s| !section::ALL.contains(&s.name))
    {
        let end = unknown
            .body
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .map_or(0, |i| i + 1);
        text.push('\n');
        text.push_str("## ");
        text.push_str(unknown.name);
        text.push('\n');
        for line in &unknown.body[..end] {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}
