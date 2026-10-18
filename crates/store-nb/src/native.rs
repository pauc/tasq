//! [`NativeBookkeeper`]: the bookkeeping after a write (ADR-0023). Appends to
//! `.index` exactly as `nb index add` does, commits with `git` in a
//! background job when the notebook is a repository (see [`crate::job`]),
//! pulls and pushes in the same job every `store.sync_interval` seconds,
//! verifies the index against the directory and syncs on demand with
//! `git pull --rebase && git push`.

use std::io::Write;
use std::path::{Path, PathBuf};

use tasq_core::clock::Clock;
use tasq_core::store::StoreError;

use crate::bookkeeper::{Bookkeeper, SyncOutcome, Verification, WriteLock, failure};
use crate::git::Git;
use crate::index::Index;
use crate::job::{self, State};
use crate::resolve::index_path;

/// The fix suggested when the background job failed.
pub const JOB_FIX: &str = "check 'git status' in the notebook and commit by hand";

/// Bookkeeping done by `tasq` itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeBookkeeper {
    git: Git,
    state: State,
    sync_interval: u64,
}

impl NativeBookkeeper {
    /// Keeps the notebook at `dir`; `git` runs with `env`. Writes never
    /// sync until [`with_sync_interval`](Self::with_sync_interval) says how
    /// often.
    pub fn new(dir: impl Into<PathBuf>, env: Vec<(String, String)>) -> Self {
        let git = Git::new(dir, env);
        let state = State::new(git.dir());
        Self {
            git,
            state,
            sync_interval: 0,
        }
    }

    /// Pull and push after a write when the last sync is at least
    /// `seconds` old; `0` never does.
    #[must_use]
    pub fn with_sync_interval(mut self, seconds: u64) -> Self {
        self.sync_interval = seconds;
        self
    }

    /// The notebook directory.
    pub fn dir(&self) -> &Path {
        self.git.dir()
    }

    /// Seconds between automatic syncs, `0` for never.
    pub fn sync_interval(&self) -> u64 {
        self.sync_interval
    }

    /// The job's coordination files.
    pub fn state(&self) -> &State {
        &self.state
    }
}

fn io(path: PathBuf, source: std::io::Error) -> StoreError {
    StoreError::Io { path, source }
}

/// The bytes `nb index add <name>` appends to `.index`: the name and a
/// newline, nothing else (nb does not repair a missing trailing newline on
/// the previous line either).
pub fn index_entry(name: &str) -> String {
    format!("{name}\n")
}

/// The non-hidden `*.md` files directly in `dir`, sorted by name (which is
/// creation order for nb's timestamped names). Hidden files and
/// non-markdown files are ignored, as nb's own patterns do for `.index` and
/// `.git`.
pub fn markdown_files(dir: &Path) -> Result<Vec<String>, std::io::Error> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_markdown = Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !name.starts_with('.') && is_markdown && entry.file_type()?.is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Compares `index` with the `*.md` files in `dir` (see
/// [`markdown_files`]): every non-empty index line must name an existing
/// file and every markdown file must be listed.
pub fn verify_index(dir: &Path, index: &Index) -> Result<Verification, std::io::Error> {
    let mut missing_files: Vec<String> = index
        .entries()
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty() && !dir.join(name).is_file())
        .map(str::to_owned)
        .collect();
    missing_files.sort();
    missing_files.dedup();
    let unindexed_files: Vec<String> = markdown_files(dir)?
        .into_iter()
        .filter(|name| !index.entries().any(|(_, listed)| listed == name))
        .collect();
    let consistent = missing_files.is_empty() && unindexed_files.is_empty();
    let detail = if consistent {
        "index matches the files".to_owned()
    } else {
        format!(
            "index out of date: {} listed file(s) missing, {} file(s) not listed",
            missing_files.len(),
            unindexed_files.len()
        )
    };
    Ok(Verification {
        consistent,
        detail,
        missing_files,
        unindexed_files,
        raw_output: None,
    })
}

impl Bookkeeper for NativeBookkeeper {
    fn name(&self) -> &'static str {
        "native"
    }

    /// Appends the file's basename to `.index` (creating the index when it
    /// does not exist, as nb's `>>` would).
    fn register(&self, file: &Path) -> Result<(), StoreError> {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = index_path(self.dir());
        let mut index = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| io(path.clone(), e))?;
        index
            .write_all(index_entry(&name).as_bytes())
            .map_err(|e| io(path, e))
    }

    /// Waits for the previous job and takes `.git/tasq/lock`, breaking a
    /// lock that is stale by `clock`. A notebook that is not a repository
    /// has no lock.
    fn lock(&self, clock: &dyn Clock) -> Result<WriteLock, StoreError> {
        if !self.git.is_repository() {
            return Ok(WriteLock::none());
        }
        self.state
            .acquire(clock)
            .map(WriteLock::held)
            .map_err(|e| failure("taking the notebook lock", &e, JOB_FIX))
    }

    /// Starts the background job (see [`crate::job::SCRIPT`]): `git add -A`,
    /// `git commit -q -m <message>`, then a pull and push when
    /// [`job::sync_due`] at `clock`'s time, which the job then records. The
    /// job releases `lock`. Nothing is started when the notebook is not a
    /// repository.
    fn checkpoint(
        &self,
        lock: WriteLock,
        message: &str,
        clock: &dyn Clock,
    ) -> Result<bool, StoreError> {
        if !self.git.is_repository() {
            return Ok(false);
        }
        let now = job::unix_time(clock);
        let sync = job::sync_due(self.state.last_sync(), now, self.sync_interval);
        let lock_path = lock.path().map(Path::to_path_buf).unwrap_or_default();
        job::spawn(
            self.dir(),
            self.git.env(),
            &self.state,
            &lock_path,
            message,
            sync,
            now,
        )
        .map_err(|e| failure("starting the commit", &e, JOB_FIX))?;
        lock.hand_over();
        Ok(true)
    }

    fn take_failure(&self) -> Option<String> {
        self.state.take_failure()
    }

    fn verify(&self) -> Result<Verification, StoreError> {
        let path = index_path(self.dir());
        let text = std::fs::read_to_string(&path).map_err(|e| io(path, e))?;
        verify_index(self.dir(), &Index::parse(&text)).map_err(|e| io(self.dir().to_owned(), e))
    }

    /// `git pull --rebase` then `git push` when the notebook is a repository
    /// with a remote, recorded at `clock`'s time; otherwise nothing, with a
    /// `detail` saying why.
    fn sync(&self, clock: &dyn Clock) -> Result<SyncOutcome, StoreError> {
        if !self.git.is_repository() {
            return Ok(SyncOutcome::skipped(
                "not a git repository; nothing to sync",
            ));
        }
        let _lock = self.lock(clock)?;
        let fix = "sync by hand with 'git pull --rebase && git push'";
        let remotes = self
            .git
            .remotes()
            .map_err(|e| failure("listing remotes", &e, fix))?;
        if remotes.is_empty() {
            return Ok(SyncOutcome::skipped(
                "no remote configured; nothing to sync",
            ));
        }
        let _ = self.state.record_sync(job::unix_time(clock));
        let pulled = self
            .git
            .run(&["pull", "--rebase", "-q"])
            .map_err(|e| failure("pulling", &e, fix))?;
        let pushed = self
            .git
            .run(&["push", "-q"])
            .map_err(|e| failure("pushing", &e, fix))?;
        let raw = format!("{pulled}{pushed}").trim().to_owned();
        Ok(SyncOutcome {
            synced: true,
            detail: format!("pulled and pushed ({})", remotes.join(", ")),
            raw_output: (!raw.is_empty()).then_some(raw),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookkeeper::RECONCILE_FIX;

    #[test]
    fn index_entry_is_name_plus_newline() {
        assert_eq!(
            index_entry("20261004120000.todo.md"),
            "20261004120000.todo.md\n"
        );
        assert_eq!(index_entry(""), "\n");
    }

    #[test]
    fn verify_index_reports_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.todo.md", "b.todo.md", "notes.md", ".index", "image.png"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub.md")).unwrap();
        let index = Index::parse("a.todo.md\ngone.todo.md\n\nnotes.md\n");
        let v = verify_index(dir.path(), &index).unwrap();
        assert!(!v.consistent);
        assert_eq!(v.missing_files, vec!["gone.todo.md"]);
        assert_eq!(v.unindexed_files, vec!["b.todo.md"]);
        assert_eq!(
            v.detail,
            "index out of date: 1 listed file(s) missing, 1 file(s) not listed"
        );
        assert_eq!(v.raw_output, None);
        assert_eq!(v.fix(), Some(RECONCILE_FIX));

        let index = Index::parse("a.todo.md\nb.todo.md\nnotes.md\n");
        let v = verify_index(dir.path(), &index).unwrap();
        assert!(v.consistent);
        assert_eq!(v.detail, "index matches the files");
        assert_eq!(v.missing_files, Vec::<String>::new());
        assert_eq!(v.unindexed_files, Vec::<String>::new());

        assert!(verify_index(&dir.path().join("nope"), &index).is_err());
    }

    #[test]
    fn verify_index_sorts_its_lists() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["z.md", "y.md"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let v = verify_index(dir.path(), &Index::parse("q.md\nb.md\n")).unwrap();
        assert_eq!(v.missing_files, vec!["b.md", "q.md"]);
        assert_eq!(v.unindexed_files, vec!["y.md", "z.md"]);
        assert_eq!(
            v.detail,
            "index out of date: 2 listed file(s) missing, 2 file(s) not listed"
        );
    }

    #[test]
    fn name_and_dir() {
        let b = NativeBookkeeper::new("/nb/home", Vec::new());
        assert_eq!(b.name(), "native");
        assert_eq!(b.dir(), Path::new("/nb/home"));
        assert_eq!(b.sync_interval(), 0);
        assert_eq!(b.state(), &State::new(Path::new("/nb/home")));
        assert_eq!(b.with_sync_interval(60).sync_interval(), 60);
    }
}
