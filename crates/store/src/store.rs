//! [`TasqStore`]: a directory of `<ULID>.md` task files as a [`Store`]
//! (ADR-0026).
//!
//! Every command reads the directory afresh: the id map is built from the
//! frontmatter of each file, there is no index and no cache. Reads never
//! lock and never create the directory; one that does not exist is an
//! empty store.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use tasq_core::doctor::Check;
use tasq_core::format::{self, FormatError, Parsed};
use tasq_core::model::{Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::{IdScheme, Store, StoreError, StoreInfo};

use crate::ulid::Ulid;

/// The name `describe()` reports.
pub const NAME: &str = "tasq";

/// A tasq-owned store: the directory `<data_dir>/<store.name>`.
#[derive(Debug)]
pub struct TasqStore {
    dir: PathBuf,
    workflow: Workflow,
    warnings: RefCell<Vec<String>>,
}

/// One task file the scan read.
#[derive(Debug)]
struct TaskFile {
    path: PathBuf,
    parsed: Parsed,
}

/// What the directory holds: whether it exists, the task files in name
/// order, and the markdown files that are not task files.
#[derive(Debug, Default)]
struct Scan {
    exists: bool,
    tasks: Vec<TaskFile>,
    skipped: Vec<(PathBuf, FormatError)>,
}

impl Scan {
    /// The task files claiming `id`, in name order.
    fn claiming(&self, id: &TaskId) -> Vec<&TaskFile> {
        self.tasks
            .iter()
            .filter(|f| f.parsed.task.id == *id)
            .collect()
    }

    /// The one file claiming `id`: [`StoreError::NotFound`] when none does,
    /// [`StoreError::AmbiguousId`] naming them all when several do.
    fn single(&self, id: &TaskId) -> Result<&TaskFile, StoreError> {
        match self.claiming(id).as_slice() {
            [] => Err(StoreError::NotFound(id.clone())),
            [one] => Ok(one),
            many => Err(StoreError::AmbiguousId {
                id: id.clone(),
                paths: many.iter().map(|f| f.path.clone()).collect(),
            }),
        }
    }

    /// One [`StoreError::AmbiguousId`] per id claimed by more than one
    /// file, in id order.
    fn ambiguous(&self) -> Vec<StoreError> {
        let mut ids: Vec<&TaskId> = self.tasks.iter().map(|f| &f.parsed.task.id).collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| self.single(id).err())
            .collect()
    }
}

impl TasqStore {
    /// The store over `dir`, which need not exist. `workflow` says which
    /// `status` values are statuses.
    pub fn new(dir: impl Into<PathBuf>, workflow: Workflow) -> Self {
        Self {
            dir: dir.into(),
            workflow,
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Reads every `.md` file of the directory (extension compared
    /// ignoring case). A directory that does not exist is empty. Dot files
    /// (editor locks), directories (`.tasq/`, a directory named `x.md`) and
    /// names with another extension are not read; a `.md` file that is not
    /// a v2 task file is recorded as skipped; a file that vanished between
    /// the listing and the read (or a dangling symlink) is dropped; any
    /// other I/O failure is the error.
    fn scan(&self) -> Result<Scan, StoreError> {
        let io_error = |path: &Path, source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let listing = match std::fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Scan::default()),
            Err(e) => return Err(io_error(&self.dir, e)),
        };
        let mut names = Vec::new();
        for entry in listing {
            let entry = entry.map_err(|e| io_error(&self.dir, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_markdown = Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
            if !name.starts_with('.') && is_markdown {
                names.push(name);
            }
        }
        names.sort();
        let mut scan = Scan {
            exists: true,
            ..Scan::default()
        };
        for name in names {
            let path = self.dir.join(&name);
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
                Err(e) => return Err(io_error(&path, e)),
            };
            let text = String::from_utf8_lossy(&bytes);
            match format::parse_v2(&text, created_at(&name), &self.workflow) {
                Ok(parsed) => scan.tasks.push(TaskFile { path, parsed }),
                Err(e) => scan.skipped.push((path, e)),
            }
        }
        Ok(scan)
    }
}

/// What to do about a `.md` file that did not parse as a v2 task file.
fn task_file_fix(error: &FormatError) -> &'static str {
    match error {
        FormatError::NotFrontmatter { .. } | FormatError::NotATask { .. } => {
            "add a frontmatter block with an id, or move the file out of the store"
        }
        FormatError::UnterminatedFrontmatter => "close the frontmatter block with a '---' line",
        FormatError::MissingId { .. } => "add an 'id: <number>' line to the frontmatter",
        FormatError::InvalidId { .. } => "make the id a positive integer",
        FormatError::MissingTitle { .. } => "put a '# Title' line right after the frontmatter",
    }
}

/// The creation time a file name carries: the ULID's timestamp in local
/// time, `None` for a name that is not `<ULID>.md`.
fn created_at(name: &str) -> Option<chrono::NaiveDateTime> {
    let stem = Path::new(name).file_stem()?.to_str()?;
    let ulid = Ulid::parse(stem)?;
    Some(local_time(ulid.timestamp()))
}

/// `utc` as the machine's local wall-clock time, the shape every timestamp
/// in the model has (see `tasq_core::clock`).
fn local_time(utc: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDateTime {
    utc.with_timezone(&chrono::Local).naive_local()
}

impl Store for TasqStore {
    /// Every task file, in file name order (creation order for ULID
    /// names), filtered by `filter` (which defaults to open tasks only). An
    /// id claimed by more than one file is a warning; both files are listed.
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        let scan = self.scan()?;
        for ambiguous in scan.ambiguous() {
            self.warnings
                .borrow_mut()
                .push(format!("{ambiguous}; see 'tasq doctor'"));
        }
        Ok(scan
            .tasks
            .into_iter()
            .map(|f| f.parsed.task)
            .filter(|t| filter.matches(t))
            .collect())
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        Ok(self.scan()?.single(id)?.parsed.task.clone())
    }

    fn create(&mut self, _draft: TaskDraft) -> Result<Task, StoreError> {
        Err(StoreError::Unsupported {
            operation: "create (the tasq store does not write yet)".to_owned(),
        })
    }

    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        Err(StoreError::Unsupported {
            operation: format!(
                "update of task {} (the tasq store does not write yet)",
                task.id
            ),
        })
    }

    fn set_done(&mut self, id: &TaskId, _done: bool) -> Result<(), StoreError> {
        Err(StoreError::Unsupported {
            operation: format!("set_done of task {id} (the tasq store does not write yet)"),
        })
    }

    /// `task_count` is the number of task files, open and done, read now;
    /// `0`, with the error queued as a warning, when the directory cannot
    /// be read (so `tasq store info` does not pass it off as empty).
    fn describe(&self) -> StoreInfo {
        let task_count = match self.scan() {
            Ok(scan) => scan.tasks.len(),
            Err(e) => {
                self.warnings
                    .borrow_mut()
                    .push(format!("the store cannot be read: {e}"));
                0
            }
        };
        StoreInfo {
            name: NAME.to_owned(),
            location: self.dir.clone(),
            task_count,
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            bookkeeper: None,
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(self.warnings.get_mut())
    }

    /// The checks, in display order: `store` (the directory: `Ok` and
    /// nothing else when it does not exist yet, `Fail` naming the error and
    /// nothing else when it cannot be read, otherwise `Ok` with its task
    /// file count), one `task-file` `Fail` per `.md` file that is not a v2
    /// task file (or `task-files` `Ok`), and one `ids` `Fail` per id claimed
    /// by more than one file (or `ids` `Ok`).
    fn doctor(&self) -> Vec<Check> {
        let scan = match self.scan() {
            Ok(scan) => scan,
            Err(e) => {
                return vec![Check::fail(
                    "store",
                    format!("store at {} cannot be read: {e}", self.dir.display()),
                    "check the directory's permissions",
                )];
            }
        };
        let dir = self.dir.display();
        if !scan.exists {
            return vec![Check::ok(
                "store",
                format!("store at {dir} does not exist yet (created on the first write)"),
            )];
        }
        let mut checks = vec![Check::ok(
            "store",
            format!("store at {dir}, {} task file(s)", scan.tasks.len()),
        )];
        if scan.skipped.is_empty() {
            checks.push(Check::ok("task-files", "every .md file is a v2 task file"));
        }
        for (path, error) in &scan.skipped {
            checks.push(Check::fail(
                "task-file",
                format!("{}: {error}", path.display()),
                task_file_fix(error),
            ));
        }
        let ambiguous = scan.ambiguous();
        if ambiguous.is_empty() {
            checks.push(Check::ok(
                "ids",
                "ids are stable and no id is claimed twice",
            ));
        }
        for ambiguous in ambiguous {
            checks.push(Check::fail(
                "ids",
                ambiguous.to_string(),
                "give one of the files another id, or remove the copy",
            ));
        }
        checks
    }

    /// The file claiming `id`.
    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        Ok(Some(self.scan()?.single(id)?.path.clone()))
    }
}
