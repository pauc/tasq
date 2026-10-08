//! [`NbStore`]: the nb notebook as a [`Store`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use tasq_core::clock::{Clock, SystemClock};
use tasq_core::config::StoreConfig;
use tasq_core::format::{self, Document, FormatError, Parsed};
use tasq_core::model::{Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::{IdScheme, Store, StoreError, StoreInfo};

use crate::bookkeeper::{Bookkeeper, NativeBookkeeper, RECONCILE_FIX, WriteLock};
use crate::create::{DEFAULT_NOTE, check_draft, free_file_name};
use crate::diff;
use crate::index::{Index, is_todo};
use crate::native::{JOB_FIX, markdown_files, verify_index};
use crate::resolve::{index_path, resolve_notebook};
use crate::revision::Revision;

/// Everything [`NbStore::open`] needs besides the config: the environment
/// to resolve `NB_DIR` in and run `git` with, the home directory, the config
/// file that set the notebook (for error messages), the status workflow and
/// how often writes sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbStoreOptions {
    /// The environment `git` runs with and `NB_DIR` is looked up in. Nothing
    /// outside this list reaches `git`.
    pub env: Vec<(String, String)>,
    /// The user's home directory, for the default `~/.nb`.
    pub home: Option<PathBuf>,
    /// The configuration file that set `store.notebook`, when known
    /// (`Loaded::file_for("store.notebook")`).
    pub config_file: Option<PathBuf>,
    /// Which `#tags` are statuses.
    pub workflow: Workflow,
    /// Seconds between automatic pulls and pushes after a write, `0` for
    /// never (`store.sync_interval`).
    pub sync_interval: u64,
}

impl NbStoreOptions {
    /// Options with an empty environment and no home directory.
    pub fn new(workflow: Workflow) -> Self {
        Self {
            env: Vec::new(),
            home: None,
            config_file: None,
            workflow,
            sync_interval: 0,
        }
    }

    /// Options from the current process: its whole environment and `HOME`.
    ///
    /// Reason: reads process state; nothing deterministic to assert.
    #[mutants::skip]
    pub fn from_process(workflow: Workflow) -> Self {
        let env: Vec<(String, String)> = std::env::vars().collect();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            env,
            home,
            config_file: None,
            workflow,
            sync_interval: 0,
        }
    }

    /// Replaces the environment.
    #[must_use]
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    /// Sets the home directory.
    #[must_use]
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    /// Records the config file that set the notebook.
    #[must_use]
    pub fn with_config_file(mut self, file: impl Into<PathBuf>) -> Self {
        self.config_file = Some(file.into());
        self
    }

    /// Sets how often writes sync (default: `0`, never).
    #[must_use]
    pub fn with_sync_interval(mut self, seconds: u64) -> Self {
        self.sync_interval = seconds;
        self
    }
}

/// Something the user should hear about that did not stop the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreWarning {
    /// `.index` was missing and was rebuilt from the notebook's files; ids
    /// may differ from the ones the user remembers.
    RebuiltIndex {
        /// The index file.
        path: PathBuf,
    },
    /// Bookkeeping after a successful write failed, possibly in the
    /// background job of an earlier command.
    Bookkeeping {
        /// What failed and the manual fix.
        message: String,
    },
}

impl fmt::Display for StoreWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RebuiltIndex { path } => write!(
                f,
                "rebuilt missing index at {} from the notebook's files (todo ids may have changed)",
                path.display()
            ),
            Self::Bookkeeping { message } => write!(f, "bookkeeping failed: {message}"),
        }
    }
}

/// An nb notebook on disk: `*.todo.md` files plus `.index`.
///
/// Reads never spawn a process. Writes rewrite one file atomically and then
/// hand over to the [`Bookkeeper`]. Each `get`/`list` remembers the
/// [`Revision`] of the file it read, keyed by id, so that
/// [`Store::update`] can refuse to overwrite a file somebody else changed in
/// between (`nb`, the old script, an editor).
pub struct NbStore {
    dir: PathBuf,
    workflow: Workflow,
    env: Vec<(String, String)>,
    sync_interval: u64,
    bookkeeper: Box<dyn Bookkeeper>,
    clock: Box<dyn Clock>,
    index: RefCell<Index>,
    revisions: RefCell<HashMap<TaskId, Revision>>,
    warnings: Vec<StoreWarning>,
}

impl fmt::Debug for NbStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NbStore")
            .field("dir", &self.dir)
            .field("bookkeeper", &self.bookkeeper.name())
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

impl NbStore {
    /// Resolves `config.notebook` (see [`crate::resolve`]) and opens it,
    /// syncing every `config.sync_interval` seconds after a write.
    pub fn open(config: &StoreConfig, options: &NbStoreOptions) -> Result<Self, StoreError> {
        let dir = resolve_notebook(&config.notebook, options)?;
        let options = options.clone().with_sync_interval(config.sync_interval);
        Self::open_dir(dir, &options)
    }

    /// Opens the notebook at `dir` directly with a [`NativeBookkeeper`]. A
    /// missing `.index` is rebuilt from the notebook's `*.md` files in name
    /// order, which is recorded as [`StoreWarning::RebuiltIndex`]. A failure
    /// of the previous command's background job becomes a
    /// [`StoreWarning::Bookkeeping`].
    pub fn open_dir(dir: impl Into<PathBuf>, options: &NbStoreOptions) -> Result<Self, StoreError> {
        let dir = dir.into();
        let mut warnings = Vec::new();
        let index_file = index_path(&dir);
        if !index_file.exists() {
            Self::rebuild_index(&dir, &index_file)?;
            warnings.push(StoreWarning::RebuiltIndex { path: index_file });
        }
        let bookkeeper = NativeBookkeeper::new(&dir, options.env.clone())
            .with_sync_interval(options.sync_interval);
        let mut store = Self {
            dir,
            workflow: options.workflow.clone(),
            env: options.env.clone(),
            sync_interval: options.sync_interval,
            bookkeeper: Box::new(bookkeeper),
            clock: Box::new(SystemClock),
            index: RefCell::new(Index::default()),
            revisions: RefCell::new(HashMap::new()),
            warnings,
        };
        store.collect_job_failure();
        store.load_index()?;
        Ok(store)
    }

    /// Writes `index_file` listing the notebook's markdown files; never
    /// replaces an existing index.
    fn rebuild_index(dir: &Path, index_file: &Path) -> Result<(), StoreError> {
        let index_error = |e: std::io::Error| StoreError::Index {
            path: index_file.to_owned(),
            message: format!("index not found and could not be rebuilt: {e}"),
        };
        let names = markdown_files(dir).map_err(index_error)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(index_file)
            .map_err(index_error)?;
        file.write_all(Index::listing(&names).render().as_bytes())
            .map_err(index_error)
    }

    /// Replaces the bookkeeper chosen at `open` (tests use a recording or
    /// [`crate::NoopBookkeeper`]).
    #[must_use]
    pub fn with_bookkeeper(mut self, bookkeeper: Box<dyn Bookkeeper>) -> Self {
        self.bookkeeper = bookkeeper;
        self
    }

    /// Replaces the clock that stamps new files and notes (default: the
    /// system clock).
    #[must_use]
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The environment the store was opened with (`git` runs with it).
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// The notebook directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `<dir>/.index`.
    pub fn index_path(&self) -> PathBuf {
        index_path(&self.dir)
    }

    /// The status workflow used to read and write tags.
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// Seconds between automatic syncs after a write, `0` for never.
    pub fn sync_interval(&self) -> u64 {
        self.sync_interval
    }

    /// The bookkeeper in use.
    pub fn bookkeeper(&self) -> &dyn Bookkeeper {
        self.bookkeeper.as_ref()
    }

    /// Warnings accumulated since `open` (or the last [`take_warnings`](Self::take_warnings)).
    pub fn warnings(&self) -> &[StoreWarning] {
        &self.warnings
    }

    /// Hands over and clears the warnings.
    pub fn take_warnings(&mut self) -> Vec<StoreWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Whether `open` had to rebuild a missing `.index`.
    pub fn rebuilt_index(&self) -> bool {
        self.warnings
            .iter()
            .any(|w| matches!(w, StoreWarning::RebuiltIndex { .. }))
    }

    /// Waits until the background bookkeeping of earlier writes is done
    /// (tests, and `tasq store sync`, which must not race a job). A failed
    /// job becomes a warning.
    pub fn wait_for_bookkeeping(&mut self) -> Result<(), StoreError> {
        drop(self.bookkeeper.lock()?);
        self.collect_job_failure();
        Ok(())
    }

    /// Repairs `.index` without renumbering (ADR-0023): lines naming a file
    /// that no longer exists become empty, unlisted markdown files are
    /// appended in name order, and the change is committed as
    /// `[tasq] Reconcile: .index`. Returns what was found; nothing is
    /// written when the index was already consistent.
    pub fn reconcile(&mut self) -> Result<crate::bookkeeper::Verification, StoreError> {
        let lock = self.lock()?;
        let index = self.load_index()?;
        let path = self.index_path();
        let found = verify_index(&self.dir, &index).map_err(|source| StoreError::Io {
            path: self.dir.clone(),
            source,
        })?;
        if found.consistent {
            return Ok(found);
        }
        let fixed = index.reconciled(&found.missing_files, &found.unindexed_files);
        Self::write_atomic(&path, &fixed.render())?;
        self.load_index()?;
        self.checkpoint(lock, "[tasq] Reconcile: .index");
        Ok(found)
    }

    /// Takes the notebook's write lock; see [`Bookkeeper::lock`]. A failure
    /// of the previous job is left for the next [`NbStore::open`] to report:
    /// the TUI writes through `dyn Store` and has nowhere to show it.
    fn lock(&self) -> Result<WriteLock, StoreError> {
        self.bookkeeper.lock()
    }

    /// Turns the last background job's failure, if any, into a warning.
    fn collect_job_failure(&mut self) {
        if let Some(failure) = self.bookkeeper.take_failure() {
            self.warnings.push(StoreWarning::Bookkeeping {
                message: format!("the background commit failed: {failure}; {JOB_FIX}"),
            });
        }
    }

    /// Re-reads `.index` and returns it.
    fn load_index(&self) -> Result<Index, StoreError> {
        let path = self.index_path();
        let text = std::fs::read_to_string(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        let index = Index::parse(&text);
        *self.index.borrow_mut() = index.clone();
        Ok(index)
    }

    /// The file behind `id`, as `file_for_id` resolved it: the index line
    /// must name a todo file that exists.
    pub fn path_of(&self, id: &TaskId) -> Result<PathBuf, StoreError> {
        let index = self.load_index()?;
        let name = index
            .file_for(id)
            .ok_or_else(|| StoreError::NotFound(id.clone()))?;
        let path = self.dir.join(name);
        if is_todo(name) && path.is_file() {
            Ok(path)
        } else {
            Err(StoreError::NotFound(id.clone()))
        }
    }

    /// Reads and parses one file, recording its revision under `id`.
    fn read(&self, id: &TaskId, path: &Path) -> Result<(Parsed, Revision), StoreError> {
        let io_error = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let bytes = std::fs::read(path).map_err(io_error)?;
        let revision = Revision::of(path, &bytes).map_err(io_error)?;
        let text = String::from_utf8_lossy(&bytes);
        let parsed = format::parse(&text, id.clone(), &self.workflow).map_err(|source| {
            StoreError::Format {
                path: path.to_owned(),
                source,
            }
        })?;
        self.revisions.borrow_mut().insert(id.clone(), revision);
        Ok((parsed, revision))
    }

    /// Writes `text` over `path` atomically (temp file in the same directory,
    /// then rename), keeping the file's permissions, and returns the new
    /// revision.
    fn write_atomic(path: &Path, text: &str) -> Result<Revision, StoreError> {
        let io_error = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let dir = path.parent().ok_or_else(|| {
            io_error(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "task file has no parent directory",
            ))
        })?;
        let permissions = std::fs::metadata(path).map_err(io_error)?.permissions();
        let mut tmp = tempfile::Builder::new()
            .prefix(".tasq-")
            .tempfile_in(dir)
            .map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        tmp.as_file()
            .set_permissions(permissions)
            .map_err(io_error)?;
        tmp.persist(path).map_err(|e| io_error(e.error))?;
        Revision::of(path, text.as_bytes()).map_err(io_error)
    }

    /// Writes `text` to the new file `path`, which must not exist yet, via a
    /// temp file in the same directory; mode 0644 like a file nb creates.
    fn create_atomic(path: &Path, text: &str) -> Result<(), StoreError> {
        let io_error = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let dir = path.parent().ok_or_else(|| {
            io_error(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "task file has no parent directory",
            ))
        })?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".tasq-")
            .tempfile_in(dir)
            .map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o644))
                .map_err(io_error)?;
        }
        tmp.persist_noclobber(path).map_err(|e| io_error(e.error))?;
        Ok(())
    }

    /// Hands `lock` to the bookkeeper's checkpoint, turning a failure into a
    /// warning: the file is already written, so this is never data loss.
    fn checkpoint(&mut self, lock: WriteLock, message: &str) {
        if let Err(e) = self.bookkeeper.checkpoint(lock, message) {
            self.warnings.push(StoreWarning::Bookkeeping {
                message: e.to_string(),
            });
        }
    }

    /// The text [`Store::create`] writes for `draft`: the draft as a task
    /// with the id still unknown, its note (or [`DEFAULT_NOTE`]) stamped
    /// at `now`, rendered by [`Document::from_task`].
    fn render_draft(&self, draft: TaskDraft) -> String {
        let mut draft = draft;
        if draft.note.is_none() {
            draft.note = Some(DEFAULT_NOTE.to_owned());
        }
        let task = draft.into_task(TaskId::from(0), self.clock.as_ref());
        format::render(&Document::from_task(&task, &self.workflow))
    }

    fn file_name(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl Store for NbStore {
    /// Every todo in the index whose file exists and parses, like the
    /// script's `all_open`, filtered by `filter` (which defaults to open
    /// tasks only). Index lines that are not `*.todo.md`, whose file is gone
    /// or whose first line is not a task title are skipped.
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        let index = self.load_index()?;
        let mut tasks = Vec::new();
        for (id, name) in index.entries().filter(|(_, name)| is_todo(name)) {
            let path = self.dir.join(name);
            if !path.is_file() {
                continue;
            }
            match self.read(&id, &path) {
                Ok((parsed, _)) => {
                    if filter.matches(&parsed.task) {
                        tasks.push(parsed.task);
                    }
                }
                Err(StoreError::Format {
                    source: FormatError::NotATask { .. },
                    ..
                }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(tasks)
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        let path = self.path_of(id)?;
        let (parsed, _) = self.read(id, &path)?;
        Ok(parsed.task)
    }

    /// Writes `draft` as a new `YYYYMMDDHHMMSS.todo.md` named after the
    /// clock (next free second when taken, like nb), registers it through
    /// the bookkeeper, reads the id back as the last index line naming the
    /// file (the script's `grep -nxF | tail -1`), commits as
    /// `[tasq] Add: <file>` and returns the task as stored.
    ///
    /// A draft without a note gets `created via tasq create`; a `done` draft
    /// is written as `# [x]` without a status tag. Merge requests need a
    /// label ([`StoreError::Unsupported`] otherwise). When the file is
    /// written but cannot be registered, the error is
    /// [`StoreError::Bookkeeping`] and names the file.
    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        check_draft(&draft)?;
        let lock = self.lock()?;
        let now = self.clock.now();
        let name = free_file_name(now, |candidate| self.dir.join(candidate).exists()).ok_or_else(
            || StoreError::Io {
                path: self.dir.join(crate::create::file_name_at(now)),
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "no free todo filename within {} seconds",
                        crate::create::MAX_ATTEMPTS
                    ),
                ),
            },
        )?;
        let path = self.dir.join(&name);
        let text = self.render_draft(draft);
        Self::create_atomic(&path, &text)?;
        self.bookkeeper
            .register(&path)
            .map_err(|e| StoreError::Bookkeeping {
                message: format!("{name} was written but not added to .index: {e}"),
            })?;
        let id = self
            .load_index()?
            .last_id_of(&name)
            .ok_or_else(|| StoreError::Bookkeeping {
                message: format!("{name} was written but .index does not list it; {RECONCILE_FIX}"),
            })?;
        self.checkpoint(lock, &format!("[tasq] Add: {name}"));
        self.get(&id)
    }

    /// Rewrites the task's file with the operations that bring it in line
    /// with `task` (see [`crate::diff`]). Nothing is written when the file
    /// already describes `task`, when it changed since `task` was read
    /// ([`StoreError::Conflict`]) or when a change has no operation
    /// ([`StoreError::Unsupported`]).
    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        let lock = self.lock()?;
        let path = self.path_of(&task.id)?;
        let baseline = self.revisions.borrow().get(&task.id).copied();
        let (mut parsed, current) = self.read(&task.id, &path)?;
        if let Some(baseline) = baseline
            && baseline != current
        {
            // Put the baseline back: the caller's task is still the stale one.
            self.revisions
                .borrow_mut()
                .insert(task.id.clone(), baseline);
            return Err(StoreError::Conflict {
                id: task.id.clone(),
                path,
            });
        }
        let before = format::render(&parsed.document);
        if let Err(fields) = diff::apply(&mut parsed.document, task, &self.workflow) {
            return Err(StoreError::Unsupported {
                operation: format!(
                    "changing {} of task {} (the nb store can only set status, priority, project and done, and append progress, worktrees, sessions, related links and merge requests)",
                    fields.join(", "),
                    task.id
                ),
            });
        }
        let after = format::render(&parsed.document);
        if after == before {
            return Ok(());
        }
        let revision = Self::write_atomic(&path, &after)?;
        self.revisions
            .borrow_mut()
            .insert(task.id.clone(), revision);
        self.checkpoint(lock, &format!("[tasq] Update: {}", Self::file_name(&path)));
        Ok(())
    }

    /// `done`: what `nb todo do` writes (`# [x]`) plus the status tag
    /// removed, as `tasks done` did. `!done`: `nb todo undo` (`# [ ]`); no
    /// status is restored. The file is left alone when already in that state.
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        let lock = self.lock()?;
        let path = self.path_of(id)?;
        let (mut parsed, _) = self.read(id, &path)?;
        let before = format::render(&parsed.document);
        if done {
            format::ops::set_done(&mut parsed.document, &self.workflow);
        } else {
            format::ops::set_open(&mut parsed.document);
            format::ops::clear_closed(&mut parsed.document);
        }
        let after = format::render(&parsed.document);
        if after == before {
            return Ok(());
        }
        Self::write_atomic(&path, &after)?;
        // The caller holds no task for this id that matches the file now;
        // drop the baseline so the next `get` starts fresh.
        self.revisions.borrow_mut().remove(id);
        let verb = if done { "Done" } else { "Undone" };
        self.checkpoint(lock, &format!("[tasq] {verb}: {}", Self::file_name(&path)));
        Ok(())
    }

    /// `task_count` is the number of index lines naming a todo file that
    /// exists, open or done, as of the last index read.
    fn describe(&self) -> StoreInfo {
        let index = self.index.borrow();
        let task_count = index
            .entries()
            .filter(|(_, name)| is_todo(name) && self.dir.join(name).is_file())
            .count();
        StoreInfo {
            name: "nb".to_owned(),
            location: self.dir.clone(),
            task_count,
            id_scheme: IdScheme::Positional,
            ids_may_change_on_reconcile: true,
        }
    }

    /// The todo file behind `id` ([`NbStore::path_of`]).
    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        self.path_of(id).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_messages() {
        assert_eq!(
            StoreWarning::RebuiltIndex {
                path: "/nb/home/.index".into()
            }
            .to_string(),
            "rebuilt missing index at /nb/home/.index from the notebook's files (todo ids may have changed)"
        );
        assert_eq!(
            StoreWarning::Bookkeeping {
                message: "the background commit failed".into()
            }
            .to_string(),
            "bookkeeping failed: the background commit failed"
        );
    }

    #[test]
    fn options_builders() {
        let o = NbStoreOptions::new(Workflow::default())
            .with_env(vec![("NB_DIR".into(), "/d".into())])
            .with_home("/h")
            .with_config_file("/c.toml")
            .with_sync_interval(60);
        assert_eq!(o.env, vec![("NB_DIR".to_owned(), "/d".to_owned())]);
        assert_eq!(o.home, Some(PathBuf::from("/h")));
        assert_eq!(o.config_file, Some(PathBuf::from("/c.toml")));
        assert_eq!(o.workflow, Workflow::default());
        assert_eq!(o.sync_interval, 60);
        let plain = NbStoreOptions::new(Workflow::default());
        assert_eq!(plain.env, Vec::new());
        assert_eq!((plain.home, plain.config_file), (None, None));
        assert_eq!(plain.sync_interval, 0);
    }

    #[test]
    fn file_name_of_a_path() {
        assert_eq!(
            NbStore::file_name(Path::new("/nb/home/x.todo.md")),
            "x.todo.md"
        );
        assert_eq!(NbStore::file_name(Path::new("/")), "");
    }
}
