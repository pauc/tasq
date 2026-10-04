//! The bookkeeping that follows a write: registering a new file in `.index`,
//! committing, verifying the index and syncing with a remote (ADR-0007).
//!
//! Two strategies implement the [`Bookkeeper`] trait: [`NbCliBookkeeper`]
//! shells out to `nb` so the index and git semantics stay nb's, and
//! [`NativeBookkeeper`] appends to `.index` and runs `git` itself for people
//! without nb. [`select_bookkeeper`] maps the `store.bookkeeper` config
//! value onto one of them; [`NoopBookkeeper`] is for tests.
//!
//! Raw `nb`/`git` output never reaches the user through error messages:
//! it is sanitised and returned in the `raw_output` field of the result
//! structs for the CLI to show at verbose level.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tasq_core::store::StoreError;

pub use crate::native::NativeBookkeeper;
use crate::nb::Nb;
pub use crate::nb_cli::NbCliBookkeeper;

/// The manual repair for an index that no longer matches the files.
pub const RECONCILE_FIX: &str = "run 'nb index reconcile' in the notebook";

/// What a `verify` found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    /// Whether the index matches the files.
    pub consistent: bool,
    /// One line summarising the result, safe to print.
    pub detail: String,
    /// Index lines naming a file that does not exist (native only; nb does
    /// not say which).
    pub missing_files: Vec<String>,
    /// Files in the notebook that no index line names (native only).
    pub unindexed_files: Vec<String>,
    /// Sanitised output of the tool that verified, for `-v`.
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
/// wrote a file (ADR-0007).
pub trait Bookkeeper {
    /// Strategy name, for diagnostics (`nb`, `native`, `none`).
    fn name(&self) -> &str;

    /// Adds a newly created `file` (a path inside the notebook) to the
    /// index. The file must already exist.
    fn register(&self, file: &Path) -> Result<(), StoreError>;

    /// Commits pending changes with `message`. Returns `false`, without
    /// committing, when nothing changed or the notebook is not a repository.
    fn checkpoint(&self, message: &str) -> Result<bool, StoreError>;

    /// Checks that the index matches the files.
    fn verify(&self) -> Result<Verification, StoreError>;

    /// Exchanges commits with the notebook's remote, when it has one.
    fn sync(&self) -> Result<SyncOutcome, StoreError>;
}

/// Does no bookkeeping at all: files are written, nothing else happens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopBookkeeper;

impl Bookkeeper for NoopBookkeeper {
    fn name(&self) -> &'static str {
        "none"
    }

    fn register(&self, _file: &Path) -> Result<(), StoreError> {
        Ok(())
    }

    fn checkpoint(&self, _message: &str) -> Result<bool, StoreError> {
        Ok(false)
    }

    fn verify(&self) -> Result<Verification, StoreError> {
        Ok(Verification::consistent(
            "index not verified (no bookkeeper)",
        ))
    }

    fn sync(&self) -> Result<SyncOutcome, StoreError> {
        Ok(SyncOutcome::skipped("nothing to sync (no bookkeeper)"))
    }
}

/// The `store.bookkeeper` config key, named by selection errors.
pub const SETTING: &str = "store.bookkeeper";

/// Picks the strategy `choice` asks for, for the notebook at `dir`.
/// `nb` is the located executable, `None` when nb is not on `PATH`; `env`
/// is what the native strategy runs `git` with. `Auto` takes nb when it is
/// there and native otherwise; `Nb` without nb is a config error.
pub fn select_bookkeeper(
    choice: tasq_core::config::Bookkeeper,
    nb: Option<&Nb>,
    dir: &Path,
    env: &[(String, String)],
) -> Result<Box<dyn Bookkeeper>, StoreError> {
    use tasq_core::config::Bookkeeper as Choice;
    match (choice, nb) {
        (Choice::Auto | Choice::Nb, Some(nb)) => {
            Ok(Box::new(NbCliBookkeeper::new(nb.clone(), dir)))
        }
        (Choice::Auto | Choice::Native, _) => {
            Ok(Box::new(NativeBookkeeper::new(dir, env.to_vec())))
        }
        (Choice::Nb, None) => Err(StoreError::Config {
            setting: SETTING,
            file: None,
            message:
                "set to \"nb\" but nb is not on PATH (install nb, or use \"native\" or \"auto\")"
                    .to_owned(),
        }),
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
    use tasq_core::config::Bookkeeper as Choice;

    #[test]
    fn noop_succeeds_at_everything() {
        let b = NoopBookkeeper;
        assert_eq!(b.name(), "none");
        assert!(b.register(Path::new("/x/y.todo.md")).is_ok());
        assert!(!(b.checkpoint("[tasq] Update: y.todo.md").unwrap()));
        let v = b.verify().unwrap();
        assert!(v.consistent);
        assert_eq!(v.detail, "index not verified (no bookkeeper)");
        assert_eq!(v.fix(), None);
        let s = b.sync().unwrap();
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
        let e = failure("nb index add x.todo.md", &"boom", RECONCILE_FIX);
        assert_eq!(
            e.to_string(),
            "bookkeeping failed after the write: nb index add x.todo.md: boom; run 'nb index reconcile' in the notebook"
        );
    }

    #[test]
    fn selection_matrix() {
        let nb = Nb::at("/usr/bin/nb", Vec::new());
        let dir = Path::new("/nb/home");
        let env = vec![("HOME".to_owned(), "/h".to_owned())];
        let name = |c, nb| select_bookkeeper(c, nb, dir, &env).map(|b| b.name().to_owned());
        assert_eq!(name(Choice::Auto, Some(&nb)).unwrap(), "nb");
        assert_eq!(name(Choice::Auto, None).unwrap(), "native");
        assert_eq!(name(Choice::Nb, Some(&nb)).unwrap(), "nb");
        assert_eq!(name(Choice::Native, Some(&nb)).unwrap(), "native");
        assert_eq!(name(Choice::Native, None).unwrap(), "native");
        let err = name(Choice::Nb, None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.bookkeeper: set to \"nb\" but nb is not on PATH (install nb, or use \"native\" or \"auto\")"
        );
    }
}
