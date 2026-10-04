//! [`NbCliBookkeeper`]: bookkeeping delegated to the `nb` command line tool
//! (ADR-0007). The index and git semantics stay nb's: `nb index add`,
//! `nb git dirty`, `nb git checkpoint`, `nb index verify`, `nb sync`.
//!
//! Every `index` subcommand gets the notebook directory as its documented
//! final `<folder-path>` argument; `git` and `sync` take none, so they run
//! with the notebook as working directory, which nb treats as the current
//! ("local") notebook.
//!
//! Facts about nb 7.25 that shaped this (probed in an isolated `NB_DIR`):
//!
//! - `nb git checkpoint` commits in a background subshell unless `--wait`
//!   is passed; without it the commit may land after we return.
//! - `nb git checkpoint` runs `nb sync` afterwards when the user's
//!   `auto_sync` is on. That is the configured behaviour and is kept.
//! - `nb git dirty` exits 1 when clean (and when the notebook is not a git
//!   repository); `nb index verify` exits 1 and prints "Index corrupted" on
//!   standard error when the index is inconsistent; `nb sync` exits 1 with
//!   "No remote configured" when there is no remote.
//! - Most nb *read* commands (`nb todos`, ...) also commit a dirty notebook
//!   in the background as `[nb] Commit`, so a checkpoint has to follow the
//!   write immediately to own its commit message.

use std::path::{Path, PathBuf};

use tasq_core::store::StoreError;

use crate::bookkeeper::{Bookkeeper, RECONCILE_FIX, SyncOutcome, Verification, failure};
use crate::nb::{Nb, NbError};

/// Bookkeeping through the `nb` CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NbCliBookkeeper {
    nb: Nb,
    dir: PathBuf,
}

impl NbCliBookkeeper {
    /// Drives `nb` for the notebook at `dir`.
    pub fn new(nb: Nb, dir: impl Into<PathBuf>) -> Self {
        Self {
            nb,
            dir: dir.into(),
        }
    }

    /// The `nb` in use.
    pub fn nb(&self) -> &Nb {
        &self.nb
    }

    /// The notebook directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn dir_arg(&self) -> String {
        self.dir.display().to_string()
    }

    /// Whether `nb git dirty` reports uncommitted changes. A `.git`-less
    /// notebook is never dirty (nb cannot commit it) and spawns nothing.
    fn is_dirty(&self) -> Result<bool, StoreError> {
        if !self.dir.join(".git").exists() {
            return Ok(false);
        }
        match self
            .nb
            .run_in(Some(&self.dir), &["git", "dirty", &self.dir_arg()])
        {
            Ok(_) => Ok(true),
            Err(NbError::Failed {
                status: Some(1), ..
            }) => Ok(false),
            Err(e) => Err(failure(
                "checking for changes",
                &e,
                "commit by hand with 'nb git checkpoint'",
            )),
        }
    }
}

/// Whether nb's verify output says the index is corrupted; nb prints
/// `! Index corrupted. To fix, run: nb index reconcile`.
pub fn says_corrupted(output: &str) -> bool {
    output.to_ascii_lowercase().contains("corrupted")
}

/// Whether nb's sync output says there is no remote to sync with.
pub fn says_no_remote(output: &str) -> bool {
    output.to_ascii_lowercase().contains("no remote")
}

impl Bookkeeper for NbCliBookkeeper {
    fn name(&self) -> &'static str {
        "nb"
    }

    /// `nb index add <basename> <dir>`: appends the name unless it is
    /// already listed.
    fn register(&self, file: &Path) -> Result<(), StoreError> {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.nb
            .run(&["index", "add", &name, &self.dir_arg()])
            .map(drop)
            .map_err(|e| failure(&format!("registering {name}"), &e, RECONCILE_FIX))
    }

    /// `nb git checkpoint <dir> <message> --wait`, only when `nb git dirty`
    /// says there is something to commit. With nb's `auto_sync` on, nb also
    /// pushes.
    fn checkpoint(&self, message: &str) -> Result<bool, StoreError> {
        if !self.is_dirty()? {
            return Ok(false);
        }
        self.nb
            .run_in(
                Some(&self.dir),
                &["git", "checkpoint", &self.dir_arg(), message, "--wait"],
            )
            .map(|_| true)
            .map_err(|e| failure("committing", &e, "commit by hand with 'nb git checkpoint'"))
    }

    /// `nb index verify <dir>`. The result is read from the exit status and
    /// from the text, because nb versions differ on whether "Index
    /// corrupted" comes with exit 0 or 1.
    fn verify(&self) -> Result<Verification, StoreError> {
        let (ok, output) = match self.nb.run(&["index", "verify", &self.dir_arg()]) {
            Ok(out) => (true, out),
            Err(NbError::Failed { stderr, .. }) => (false, stderr),
            Err(e @ NbError::Spawn { .. }) => {
                return Err(failure("verifying the index", &e, RECONCILE_FIX));
            }
        };
        let output = output.trim().to_owned();
        let consistent = ok && !says_corrupted(&output);
        let detail = if consistent {
            "nb index verify: index matches the files".to_owned()
        } else if says_corrupted(&output) {
            "nb index verify: index corrupted".to_owned()
        } else {
            "nb index verify failed".to_owned()
        };
        Ok(Verification {
            consistent,
            detail,
            missing_files: Vec::new(),
            unindexed_files: Vec::new(),
            raw_output: (!output.is_empty()).then_some(output),
        })
    }

    /// `nb sync` in the notebook. "No remote configured" is not an error.
    fn sync(&self) -> Result<SyncOutcome, StoreError> {
        match self.nb.run_in(Some(&self.dir), &["sync"]) {
            Ok(out) => {
                let out = out.trim().to_owned();
                Ok(SyncOutcome {
                    synced: true,
                    detail: "nb sync: synced with the remote".to_owned(),
                    raw_output: (!out.is_empty()).then_some(out),
                })
            }
            Err(NbError::Failed { stderr, .. }) if says_no_remote(&stderr) => Ok(SyncOutcome {
                synced: false,
                detail: "nb sync: no remote configured".to_owned(),
                raw_output: Some(stderr),
            }),
            Err(e) => Err(failure(
                "syncing",
                &e,
                "fix the remote and run 'nb sync' by hand",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_nb_and_dir() {
        let nb = Nb::at("/bin/nb", Vec::new());
        let b = NbCliBookkeeper::new(nb.clone(), "/nb/home");
        assert_eq!(b.nb(), &nb);
        assert_eq!(b.dir(), Path::new("/nb/home"));
        assert_eq!(b.dir_arg(), "/nb/home");
        assert_eq!(b.name(), "nb");
    }

    #[test]
    fn output_classification() {
        assert!(says_corrupted(
            "! Index corrupted. To fix, run:\n  nb index reconcile"
        ));
        assert!(says_corrupted("INDEX CORRUPTED"));
        assert!(!says_corrupted(""));
        assert!(!says_corrupted("Index ok"));
        assert!(says_no_remote("! No remote configured.\n\nSet the remote"));
        assert!(!says_no_remote("Already up to date."));
    }

    #[test]
    fn no_git_directory_is_never_dirty_without_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let nb = Nb::at(dir.path().join("missing-nb"), Vec::new());
        let b = NbCliBookkeeper::new(nb, dir.path());
        assert!(!(b.is_dirty().unwrap()));
        assert!(!(b.checkpoint("[tasq] Update: x").unwrap()));
    }
}
