//! The bookkeeping that follows a write: registering a new file in `.index`,
//! committing, verifying the index and syncing with a remote (ADR-0023).
//!
//! [`NativeBookkeeper`] does all of it with `git` and plain file appends;
//! `nb` is never run. [`NoopBookkeeper`] is for tests. A write goes
//! [`Bookkeeper::lock`], write the file, [`Bookkeeper::register`] (creates
//! only), [`Bookkeeper::checkpoint`], which hands the lock to a background
//! job so the caller never waits for a commit or a push.
//!
//! Raw `git` output never reaches the user through error messages: it is
//! returned in the `raw_output` field of the result structs for the CLI to
//! show at verbose level.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tasq_core::clock::Clock;
use tasq_core::store::StoreError;

pub use crate::native::NativeBookkeeper;

/// The manual repair for an index that no longer matches the files: what
/// `tasq store reconcile` did before it went with git sync (ADR-0026),
/// keeping every id. A missing index is rebuilt by the next
/// [`crate::NbStore::open`].
pub const RECONCILE_FIX: &str =
    "edit .index by hand: blank the line of a file that is gone, append a file that is not listed";

/// The notebook's write lock, from before a write until its background job
/// ends. Dropping it releases it; [`Bookkeeper::checkpoint`] takes it over
/// instead. Notebooks that are not repositories run no job and get no lock.
#[derive(Debug, PartialEq, Eq)]
pub struct WriteLock {
    path: Option<PathBuf>,
}

impl WriteLock {
    /// No lock to hold.
    pub fn none() -> Self {
        Self { path: None }
    }

    /// The lock file at `path`, already created by the caller.
    pub fn held(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// The lock file, when one is held.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Gives up ownership without removing the file: whoever gets the path
    /// (the background job) removes it.
    pub fn hand_over(mut self) -> Option<PathBuf> {
        self.path.take()
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// What a `verify` found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    /// Whether the index matches the files.
    pub consistent: bool,
    /// One line summarising the result, safe to print.
    pub detail: String,
    /// Index lines naming a file that does not exist.
    pub missing_files: Vec<String>,
    /// Markdown files in the notebook that no index line names.
    pub unindexed_files: Vec<String>,
    /// Output of the tool that verified, for `-v` (none for the native
    /// check).
    pub raw_output: Option<String>,
}

impl Verification {
    /// A consistent index.
    pub fn consistent(detail: impl Into<String>) -> Self {
        Self {
            consistent: true,
            detail: detail.into(),
            missing_files: Vec::new(),
            unindexed_files: Vec::new(),
            raw_output: None,
        }
    }

    /// The fix to suggest, when there is something to fix.
    pub fn fix(&self) -> Option<&'static str> {
        (!self.consistent).then_some(RECONCILE_FIX)
    }
}

/// What a `sync` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOutcome {
    /// Whether anything was exchanged with a remote. `false` with a
    /// `detail` saying why (no remote, not a repository).
    pub synced: bool,
    /// One line summarising the result, safe to print.
    pub detail: String,
    /// Sanitised tool output, for `-v`.
    pub raw_output: Option<String>,
}

impl SyncOutcome {
    /// Nothing to sync, for `detail`.
    pub fn skipped(detail: impl Into<String>) -> Self {
        Self {
            synced: false,
            detail: detail.into(),
            raw_output: None,
        }
    }
}

/// Keeps the notebook's `.index` and history consistent after the store
/// wrote a file (ADR-0023). The methods that need the time take the
/// store's clock.
pub trait Bookkeeper {
    /// Strategy name, for diagnostics (`native`, `none`).
    fn name(&self) -> &str;

    /// Waits until earlier background bookkeeping is done and takes the
    /// write lock; `clock` tells an abandoned lock from a live one. The
    /// store calls it before writing anything.
    fn lock(&self, clock: &dyn Clock) -> Result<WriteLock, StoreError>;

    /// Adds a newly created `file` (a path inside the notebook) to the
    /// index. The file must already exist.
    fn register(&self, file: &Path) -> Result<(), StoreError>;

    /// Commits pending changes with `message`, taking over `lock`. The store
    /// calls it only after a write changed bytes on disk. Returns `false`
    /// when nothing will be committed (not a repository, no bookkeeper).
    /// The commit itself may still be running when this returns; its
    /// failure is reported by [`take_failure`](Self::take_failure). `clock`
    /// decides whether a sync is due.
    fn checkpoint(
        &self,
        lock: WriteLock,
        message: &str,
        clock: &dyn Clock,
    ) -> Result<bool, StoreError>;

    /// The failure of the last background job, once.
    fn take_failure(&self) -> Option<String>;

    /// Checks that the index matches the files.
    fn verify(&self) -> Result<Verification, StoreError>;

    /// Exchanges commits with the notebook's remote, when it has one, and
    /// waits for the result. The sync is recorded at `clock`'s time.
    fn sync(&self, clock: &dyn Clock) -> Result<SyncOutcome, StoreError>;
}

/// Does no bookkeeping at all: files are written, nothing else happens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopBookkeeper;

impl Bookkeeper for NoopBookkeeper {
    fn name(&self) -> &'static str {
        "none"
    }

    fn lock(&self, _clock: &dyn Clock) -> Result<WriteLock, StoreError> {
        Ok(WriteLock::none())
    }

    fn register(&self, _file: &Path) -> Result<(), StoreError> {
        Ok(())
    }

    fn checkpoint(
        &self,
        _lock: WriteLock,
        _message: &str,
        _clock: &dyn Clock,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    fn take_failure(&self) -> Option<String> {
        None
    }

    fn verify(&self) -> Result<Verification, StoreError> {
        Ok(Verification::consistent(
            "index not verified (no bookkeeper)",
        ))
    }

    fn sync(&self, _clock: &dyn Clock) -> Result<SyncOutcome, StoreError> {
        Ok(SyncOutcome::skipped("nothing to sync (no bookkeeper)"))
    }
}

/// The bookkeeping error for a failure after `what`, with the manual fix.
pub(crate) fn failure(what: &str, error: &dyn std::fmt::Display, fix: &str) -> StoreError {
    StoreError::Bookkeeping {
        message: format!("{what}: {error}; {fix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;

    #[test]
    fn noop_succeeds_at_everything() {
        let b = NoopBookkeeper;
        let clock = FixedClock::at("2026-10-07 09:30");
        assert_eq!(b.name(), "none");
        assert_eq!(b.lock(&clock).unwrap(), WriteLock::none());
        assert!(b.register(Path::new("/x/y.todo.md")).is_ok());
        assert!(
            !(b.checkpoint(WriteLock::none(), "[tasq] Update: y.todo.md", &clock)
                .unwrap())
        );
        assert_eq!(b.take_failure(), None);
        let v = b.verify().unwrap();
        assert!(v.consistent);
        assert_eq!(v.detail, "index not verified (no bookkeeper)");
        assert_eq!(v.fix(), None);
        let s = b.sync(&clock).unwrap();
        assert!(!s.synced);
        assert_eq!(s.detail, "nothing to sync (no bookkeeper)");
        assert_eq!(s.raw_output, None);
    }

    #[test]
    fn verification_fix_only_when_inconsistent() {
        let ok = Verification::consistent("fine");
        assert_eq!(ok.fix(), None);
        assert_eq!(ok.missing_files, Vec::<String>::new());
        let bad = Verification {
            consistent: false,
            ..ok
        };
        assert_eq!(bad.fix(), Some(RECONCILE_FIX));
    }

    #[test]
    fn results_serialise() {
        let v = Verification {
            consistent: false,
            detail: "1 missing".into(),
            missing_files: vec!["a.todo.md".into()],
            unindexed_files: vec![],
            raw_output: Some("Index corrupted".into()),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json,
            "{\"consistent\":false,\"detail\":\"1 missing\",\"missing_files\":[\"a.todo.md\"],\"unindexed_files\":[],\"raw_output\":\"Index corrupted\"}"
        );
        assert_eq!(serde_json::from_str::<Verification>(&json).unwrap(), v);
        let s = SyncOutcome::skipped("no remote");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            "{\"synced\":false,\"detail\":\"no remote\",\"raw_output\":null}"
        );
        assert_eq!(serde_json::from_str::<SyncOutcome>(&json).unwrap(), s);
    }

    #[test]
    fn failure_message_names_the_fix() {
        let e = failure("registering x.todo.md", &"boom", RECONCILE_FIX);
        assert_eq!(
            e.to_string(),
            "bookkeeping failed after the write: registering x.todo.md: boom; edit .index by hand: blank the line of a file that is gone, append a file that is not listed"
        );
    }

    #[test]
    fn write_lock_releases_on_drop_unless_handed_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        std::fs::write(&path, "").unwrap();
        let lock = WriteLock::held(path.clone());
        assert_eq!(lock.path(), Some(path.as_path()));
        drop(lock);
        assert!(!path.exists(), "dropping releases");

        std::fs::write(&path, "").unwrap();
        let lock = WriteLock::held(path.clone());
        assert_eq!(lock.hand_over(), Some(path.clone()));
        assert!(path.exists(), "the new owner removes it");

        assert_eq!(WriteLock::none().path(), None);
        assert_eq!(WriteLock::none().hand_over(), None);
    }
}
