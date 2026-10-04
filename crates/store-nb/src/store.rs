//! [`NbStore`]: the nb notebook as a [`Store`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use tasq_core::config::StoreConfig;
use tasq_core::format::{self, FormatError, Parsed};
use tasq_core::model::{Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::{IdScheme, Store, StoreError, StoreInfo};

use crate::bookkeeper::{Bookkeeper, NoopBookkeeper};
use crate::diff;
use crate::index::{Index, is_todo};
use crate::nb::Nb;
use crate::resolve::{index_path, resolve_notebook};
use crate::revision::Revision;

/// Everything [`NbStore::open`] needs besides the config: the environment
/// to resolve `NB_DIR` in and run `nb` with, the home directory, the config
/// file that set the notebook (for error messages) and the status workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbStoreOptions {
    /// The environment `nb` runs with and `NB_DIR` is looked up in. Nothing
    /// outside this list reaches `nb`.
    pub env: Vec<(String, String)>,
    /// The user's home directory, for the default `~/.nb`.
    pub home: Option<PathBuf>,
    /// The configuration file that set `store.notebook`, when known
    /// (`Loaded::file_for("store.notebook")`).
    pub config_file: Option<PathBuf>,
    /// Which `#tags` are statuses.
    pub workflow: Workflow,
}

impl NbStoreOptions {
    /// Options with an empty environment and no home directory.
    pub fn new(workflow: Workflow) -> Self {
        Self {
            env: Vec::new(),
            home: None,
            config_file: None,
            workflow,
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
}

/// Something the user should hear about that did not stop the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreWarning {
    /// `.index` was missing and `nb index reconcile` rebuilt it; ids may
    /// differ from the ones the user remembers.
    RebuiltIndex {
        /// The index file.
        path: PathBuf,
    },
    /// Bookkeeping after a successful write failed.
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
                "rebuilt missing nb index at {} (todo ids may have changed)",
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
    nb: Option<Nb>,
    bookkeeper: Box<dyn Bookkeeper>,
    index: RefCell<Index>,
    revisions: RefCell<HashMap<TaskId, Revision>>,
    warnings: Vec<StoreWarning>,
}

impl fmt::Debug for NbStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NbStore")
            .field("dir", &self.dir)
            .field("nb", &self.nb.as_ref().map(Nb::program))
            .field("bookkeeper", &self.bookkeeper.name())
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

impl NbStore {
    /// Resolves `config.notebook` (see [`crate::resolve`]) and opens it.
    pub fn open(config: &StoreConfig, options: &NbStoreOptions) -> Result<Self, StoreError> {
        let nb = Nb::locate(&options.env);
        let dir = resolve_notebook(&config.notebook, options, nb.as_ref())?;
        Self::open_dir(dir, options)
    }

    /// Opens the notebook at `dir` directly. A missing `.index` is rebuilt
    /// with `nb index reconcile` when `nb` is available, which is recorded
    /// as [`StoreWarning::RebuiltIndex`]; without `nb` it is an error.
    pub fn open_dir(dir: impl Into<PathBuf>, options: &NbStoreOptions) -> Result<Self, StoreError> {
        let dir = dir.into();
        let nb = Nb::locate(&options.env);
        let mut warnings = Vec::new();
        let index_file = index_path(&dir);
        if !index_file.exists() {
            let rebuilt = nb.as_ref().is_some_and(|nb| {
                nb.run(&["index", "reconcile", &dir.display().to_string()])
                    .is_ok()
            }) && index_file.is_file();
            if !rebuilt {
                let fix = if nb.is_some() {
                    "'nb index reconcile' could not rebuild it"
                } else {
                    "nb is not on PATH to rebuild it (run 'nb index reconcile' in the notebook)"
                };
                return Err(StoreError::Index {
                    path: index_file,
                    message: format!("nb index not found, and {fix}"),
                });
            }
            warnings.push(StoreWarning::RebuiltIndex { path: index_file });
        }
        let store = Self {
            dir,
            workflow: options.workflow.clone(),
            nb,
            bookkeeper: Box::new(NoopBookkeeper),
            index: RefCell::new(Index::default()),
            revisions: RefCell::new(HashMap::new()),
            warnings,
        };
        store.load_index()?;
        Ok(store)
    }

    /// Replaces the bookkeeper (default: [`NoopBookkeeper`]).
    #[must_use]
    pub fn with_bookkeeper(mut self, bookkeeper: Box<dyn Bookkeeper>) -> Self {
        self.bookkeeper = bookkeeper;
        self
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

    /// The `nb` executable the store would use, when one was found.
    pub fn nb(&self) -> Option<&Nb> {
        self.nb.as_ref()
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

    /// Runs the bookkeeper's checkpoint, turning a failure into a warning:
    /// the file is already written, so this is never data loss.
    fn checkpoint(&mut self, message: &str) {
        if let Err(e) = self.bookkeeper.checkpoint(message) {
            self.warnings.push(StoreWarning::Bookkeeping {
                message: e.to_string(),
            });
        }
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

    /// Not available yet: creation needs nb's filename rule and index
    /// registration (plan T-203, T-206).
    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        Err(StoreError::Unsupported {
            operation: format!("creating task {:?}: not implemented yet", draft.title),
        })
    }

    /// Rewrites the task's file with the operations that bring it in line
    /// with `task` (see [`crate::diff`]). Nothing is written when the file
    /// already describes `task`, when it changed since `task` was read
    /// ([`StoreError::Conflict`]) or when a change has no operation
    /// ([`StoreError::Unsupported`]).
    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
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
        self.checkpoint(&format!("[tasq] Update: {}", Self::file_name(&path)));
        Ok(())
    }

    /// `done`: what `nb todo do` writes (`# [x]`) plus the status tag
    /// removed, as `tasks done` did. `!done`: `nb todo undo` (`# [ ]`); no
    /// status is restored. The file is left alone when already in that state.
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        let path = self.path_of(id)?;
        let (mut parsed, _) = self.read(id, &path)?;
        let before = format::render(&parsed.document);
        if done {
            format::ops::set_done(&mut parsed.document, &self.workflow);
        } else {
            format::ops::set_open(&mut parsed.document);
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
        self.checkpoint(&format!("[tasq] {verb}: {}", Self::file_name(&path)));
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
            "rebuilt missing nb index at /nb/home/.index (todo ids may have changed)"
        );
        assert_eq!(
            StoreWarning::Bookkeeping {
                message: "nb git checkpoint failed".into()
            }
            .to_string(),
            "bookkeeping failed: nb git checkpoint failed"
        );
    }

    #[test]
    fn options_builders() {
        let o = NbStoreOptions::new(Workflow::default())
            .with_env(vec![("NB_DIR".into(), "/d".into())])
            .with_home("/h")
            .with_config_file("/c.toml");
        assert_eq!(o.env, vec![("NB_DIR".to_owned(), "/d".to_owned())]);
        assert_eq!(o.home, Some(PathBuf::from("/h")));
        assert_eq!(o.config_file, Some(PathBuf::from("/c.toml")));
        assert_eq!(o.workflow, Workflow::default());
        let plain = NbStoreOptions::new(Workflow::default());
        assert_eq!(plain.env, Vec::new());
        assert_eq!((plain.home, plain.config_file), (None, None));
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
