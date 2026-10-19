//! [`TasqStore`]: a directory of `<ULID>.md` task files as a [`Store`]
//! (ADR-0026).
//!
//! Every command reads the directory afresh: the id map is built from the
//! frontmatter of each file, there is no index and no cache. Reads never
//! lock and never create the directory; one that does not exist is an
//! empty store. A write takes the store lock ([`crate::lock`]) from reading
//! the id map to the end of the write, and the first write creates the
//! directory.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use tasq_core::clock::{Clock, SystemClock, to_minute};
use tasq_core::doctor::Check;
use tasq_core::format::{self, Document, FormatError, Parsed};
use tasq_core::model::{Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::{Store, StoreError, StoreInfo};

use crate::lock::{STALE_AFTER, StoreLock, instant, is_stale, lock_path};
use crate::ulid::Ulid;
use crate::{Revision, diff, draft};

/// The name `describe()` reports.
pub const NAME: &str = "tasq";

/// How many ULIDs a create mints before giving up on a free file name. A
/// name is taken only when the random 80 bits repeat within one
/// millisecond; the bound is there so a broken random source cannot loop.
pub const MAX_NAME_ATTEMPTS: u32 = 8;

/// A source of the random part of a minted ULID: 80 bits, the rest is
/// masked off.
pub type RandomSource = Box<dyn FnMut() -> u128>;

/// A tasq-owned store: the directory `<data_dir>/<store.name>`.
///
/// `get` and `list` remember the [`Revision`] of each file they hand out,
/// keyed by id, so that [`Store::update`] can refuse to overwrite a file
/// somebody else changed in between (an editor, another process).
pub struct TasqStore {
    dir: PathBuf,
    workflow: Workflow,
    clock: Box<dyn Clock>,
    random: RandomSource,
    revisions: RefCell<HashMap<TaskId, Revision>>,
    warnings: RefCell<Vec<String>>,
}

/// Reason: the clock and the random source have no `Debug`; the fields
/// that do are printed.
impl Debug for TasqStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TasqStore")
            .field("dir", &self.dir)
            .field("workflow", &self.workflow)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

/// One task file the scan read.
#[derive(Debug)]
struct TaskFile {
    path: PathBuf,
    parsed: Parsed,
    revision: Revision,
    /// The bytes were not valid UTF-8 and were read with replacement
    /// characters; the file is readable but must not be written back.
    invalid_utf8: bool,
}

impl TaskFile {
    /// The error for a write to a file whose bytes are not valid UTF-8:
    /// writing the document back would replace the invalid bytes with
    /// U+FFFD.
    fn invalid_utf8_error(&self) -> StoreError {
        StoreError::Io {
            path: self.path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not valid UTF-8; a write would replace the invalid bytes",
            ),
        }
    }
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
    /// `status` values are statuses. Creates stamp the system clock and
    /// take the random part of the file name from the OS; see
    /// [`with_clock`](Self::with_clock) and [`with_random`](Self::with_random).
    pub fn new(dir: impl Into<PathBuf>, workflow: Workflow) -> Self {
        Self {
            dir: dir.into(),
            workflow,
            clock: Box::new(SystemClock),
            random: Box::new(os_random),
            revisions: RefCell::new(HashMap::new()),
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Replaces the clock that stamps new tasks and their notes, and whose
    /// time names their files.
    #[must_use]
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces the source of the random part of minted ULIDs (tests make
    /// file names predictable, or force a name collision).
    #[must_use]
    pub fn with_random(mut self, random: RandomSource) -> Self {
        self.random = random;
        self
    }

    /// Records the revision of `file` as the baseline of its task's next
    /// `update`.
    fn remember(&self, file: &TaskFile) {
        self.revisions
            .borrow_mut()
            .insert(file.parsed.task.id.clone(), file.revision);
    }

    /// Doctor's `lock` check: whether `.tasq/lock` exists and, by the
    /// store's clock, whether it is stale.
    fn lock_check(&self) -> Check {
        let path = lock_path(&self.dir);
        if !path.exists() {
            return Check::ok("lock", "no write in progress");
        }
        if is_stale(&path, instant(self.clock.as_ref())) {
            return Check::warn(
                "lock",
                format!(
                    "stale lock at {} (untouched for {} minutes or more: its writer died)",
                    path.display(),
                    STALE_AFTER.as_secs() / 60
                ),
                "remove the file, or ignore it: the next write removes it",
            );
        }
        Check::ok(
            "lock",
            format!("a write is in progress (lock at {})", path.display()),
        )
    }

    /// Takes the store lock, creating the directory when it does not
    /// exist. The error names the lock file.
    fn lock(&self) -> Result<StoreLock, StoreError> {
        StoreLock::acquire(&self.dir, self.clock.as_ref()).map_err(|source| StoreError::Io {
            path: lock_path(&self.dir),
            source,
        })
    }

    /// Writes `text` as a new `<ULID>.md` in the directory and returns its
    /// path: a temp file in the directory, renamed into place without
    /// overwriting. The ULID is minted at `millis` with a fresh random part
    /// for each attempt; a name that exists is tried again, up to
    /// [`MAX_NAME_ATTEMPTS`] times.
    fn write_new(&mut self, millis: u64, text: &str) -> Result<PathBuf, StoreError> {
        let io_error = |path: PathBuf, source| StoreError::Io { path, source };
        let mut tmp = tempfile::Builder::new()
            .prefix(".tasq-")
            .tempfile_in(&self.dir)
            .map_err(|e| io_error(self.dir.clone(), e))?;
        tmp.write_all(text.as_bytes())
            .map_err(|e| io_error(tmp.path().to_owned(), e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o644))
                .map_err(|e| io_error(tmp.path().to_owned(), e))?;
        }
        let mut attempts = 0;
        loop {
            attempts += 1;
            let ulid = Ulid::from_parts(millis, (self.random)());
            let path = self.dir.join(format!("{ulid}.md"));
            let Err(failed) = tmp.persist_noclobber(&path) else {
                return Ok(path);
            };
            if failed.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(io_error(path, failed.error));
            }
            if attempts >= MAX_NAME_ATTEMPTS {
                return Err(io_error(
                    path,
                    std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("no free file name after {MAX_NAME_ATTEMPTS} attempts"),
                    ),
                ));
            }
            tmp = failed.file;
        }
    }

    /// Writes `text` over the existing task file `path`: a temp file in the
    /// directory, given the file's permissions, renamed into place. Returns
    /// the file's new revision.
    fn write_over(&self, path: &Path, text: &str) -> Result<Revision, StoreError> {
        let io_error = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let permissions = std::fs::metadata(path).map_err(io_error)?.permissions();
        let mut tmp = tempfile::Builder::new()
            .prefix(".tasq-")
            .tempfile_in(&self.dir)
            .map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        tmp.as_file()
            .set_permissions(permissions)
            .map_err(io_error)?;
        tmp.persist(path).map_err(|e| io_error(e.error))?;
        Revision::of(path, text.as_bytes()).map_err(io_error)
    }

    /// Edits a copy of `file`'s document with `edit` and, when the rendered
    /// bytes changed, writes it over the file and returns the new revision;
    /// `None` when nothing changed and nothing was written. A file whose
    /// bytes are not valid UTF-8 is refused (`InvalidData`) only when it
    /// would be written.
    fn edit(
        &self,
        file: &TaskFile,
        edit: impl FnOnce(&mut Document) -> Result<(), StoreError>,
    ) -> Result<Option<Revision>, StoreError> {
        let mut document = file.parsed.document.clone();
        let before = format::render(&document);
        edit(&mut document)?;
        let after = format::render(&document);
        if after == before {
            return Ok(None);
        }
        if file.invalid_utf8 {
            return Err(file.invalid_utf8_error());
        }
        self.write_over(&file.path, &after).map(Some)
    }

    /// Reads every `.md` file of the directory (extension compared
    /// ignoring case). A directory that does not exist is empty. Dot files
    /// (editor locks), directories (`.tasq/`, a directory named `x.md`) and
    /// names with another extension are not read; a `.md` file that is not
    /// a task file is recorded as skipped; a file that vanished between
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
            let revision = Revision::of(&path, &bytes).map_err(|e| io_error(&path, e))?;
            let text = String::from_utf8_lossy(&bytes);
            let invalid_utf8 = matches!(text, std::borrow::Cow::Owned(_));
            match format::parse_v2(&text, created_at(&name), &self.workflow) {
                Ok(parsed) => scan.tasks.push(TaskFile {
                    path,
                    parsed,
                    revision,
                    invalid_utf8,
                }),
                Err(e) => scan.skipped.push((path, e)),
            }
        }
        Ok(scan)
    }
}

/// What to do about a `.md` file that did not parse as a task file.
fn task_file_fix(error: &FormatError) -> &'static str {
    match error {
        FormatError::NotFrontmatter { .. } => {
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
/// in the model has (see `tasq_core::clock`); [`instant`] is the inverse.
fn local_time(utc: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDateTime {
    utc.with_timezone(&Local).naive_local()
}

/// `at` in milliseconds since the Unix epoch, `0` for an instant before it
/// (the ULID timestamp is unsigned).
pub(crate) fn unix_millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The id a new task gets: the highest id in the store plus one, `1` when
/// there is none. Open and done tasks count, as do the files of an
/// ambiguous id.
fn next_id(scan: &Scan) -> TaskId {
    let highest = scan
        .tasks
        .iter()
        .filter_map(|f| f.parsed.task.id.as_str().parse::<u64>().ok())
        .max();
    TaskId::from(highest.map_or(1, |h| h + 1))
}

/// 128 random bits from the OS; [`Ulid::from_parts`] keeps 80. The
/// default random source of [`TasqStore`].
/// Reason: reads the OS entropy source; nothing deterministic to assert.
#[mutants::skip]
pub fn os_random() -> u128 {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS entropy source is available");
    u128::from_le_bytes(bytes)
}

impl Store for TasqStore {
    /// Every task file, in file name order (creation order for ULID
    /// names), filtered by `filter` (which defaults to open tasks only). An
    /// id claimed by more than one file is a warning; both files are listed.
    /// The revision of every file read, listed or not, is remembered.
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        let scan = self.scan()?;
        for ambiguous in scan.ambiguous() {
            self.warnings
                .borrow_mut()
                .push(format!("{ambiguous}; see 'tasq doctor'"));
        }
        for file in &scan.tasks {
            self.remember(file);
        }
        Ok(scan
            .tasks
            .into_iter()
            .map(|f| f.parsed.task)
            .filter(|t| filter.matches(t))
            .collect())
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        let scan = self.scan()?;
        let file = scan.single(id)?;
        self.remember(file);
        Ok(file.parsed.task.clone())
    }

    /// Under the store lock: reads the id map, takes the highest id plus
    /// one, writes the draft as a fresh file named by a ULID minted from
    /// the clock, releases the lock, and reads the task back (so the
    /// returned task has its `created_at`). The first write creates the
    /// directory.
    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        draft::check_draft(&draft)?;
        let lock = self.lock()?;
        let id = next_id(&self.scan()?);
        let task = draft::materialize(draft, id.clone(), self.clock.as_ref());
        let text = format::render(&Document::from_task_v2(&task));
        self.write_new(unix_millis(instant(self.clock.as_ref())), &text)?;
        drop(lock);
        self.get(&id)
    }

    /// Under the store lock: checks the file against the revision
    /// remembered when the task was read, then rewrites it with the
    /// operations that bring it in line with `task` (see [`crate::diff`]).
    /// Nothing is written when the file changed since the read
    /// ([`StoreError::Conflict`]), when it already describes `task`, or
    /// when a change has no operation ([`StoreError::Unsupported`]). The
    /// written revision becomes the baseline, so the same reader can keep
    /// writing.
    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        let _lock = self.lock()?;
        let scan = self.scan()?;
        let file = scan.single(&task.id)?;
        let baseline = self.revisions.borrow().get(&task.id).copied();
        if let Some(baseline) = baseline
            && baseline != file.revision
        {
            return Err(StoreError::Conflict {
                id: task.id.clone(),
                path: file.path.clone(),
            });
        }
        let written = self.edit(file, |document| {
            diff::apply(document, task, &self.workflow).map_err(|fields| {
                StoreError::Unsupported {
                    operation: format!(
                        "changing {} of task {} (the tasq store cannot remove or rewrite related links, merge requests, worktrees, sessions or progress entries)",
                        fields.join(", "),
                        task.id
                    ),
                }
            })
        })?;
        if let Some(revision) = written {
            self.revisions
                .borrow_mut()
                .insert(task.id.clone(), revision);
        }
        Ok(())
    }

    /// Under the store lock. `done`: `status: done` (any status is
    /// replaced) and `closed` stamped with the clock, as closing through
    /// `update` does; a task already done keeps its `closed`. `!done`:
    /// `status: done` and `closed` are dropped; no status is restored. The
    /// file is left alone when already in that state. The remembered
    /// revision is not touched: a task read before this write no longer
    /// matches the file, so an `update` with it is a conflict until it is
    /// re-read.
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        let _lock = self.lock()?;
        let scan = self.scan()?;
        let file = scan.single(id)?;
        self.edit(file, |document| {
            if done {
                format::ops::set_done(document);
                if !file.parsed.task.done {
                    format::ops::set_closed(document, to_minute(self.clock.now()));
                }
            } else {
                format::ops::set_open(document);
                format::ops::clear_closed(document);
            }
            Ok(())
        })?;
        Ok(())
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
            bookkeeper: None,
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(self.warnings.get_mut())
    }

    /// The checks, in display order: `store` (the directory: `Ok` and
    /// nothing else when it does not exist yet, `Fail` naming the error and
    /// nothing else when it cannot be read, otherwise `Ok` with its task
    /// file count), one `task-file` `Fail` per `.md` file that is not a
    /// task file (or `task-files` `Ok`), one `ids` `Fail` per id claimed
    /// by more than one file (or `ids` `Ok`), and `lock`: `Ok` with no lock
    /// file or a live one (a write in progress), `Warn` naming a stale one
    /// (its writer died; the next write removes it, doctor does not).
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
            checks.push(Check::ok("task-files", "every .md file is a task file"));
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
        checks.push(self.lock_check());
        checks
    }

    /// The file claiming `id`.
    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        Ok(Some(self.scan()?.single(id)?.path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;

    #[test]
    fn a_clock_before_the_epoch_is_millisecond_zero() {
        assert_eq!(unix_millis(instant(&FixedClock::at("1960-01-01 00:00"))), 0);
        assert_eq!(
            unix_millis(UNIX_EPOCH + std::time::Duration::from_millis(1_469_922_850_259)),
            1_469_922_850_259
        );
    }
}
