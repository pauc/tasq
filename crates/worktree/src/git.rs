//! `git` for one repository: where a branch lives, the branch checked out
//! in a directory, and `git worktree add` with cleanup on failure.

use std::path::{Path, PathBuf};

use tasq_core::worktree::lifecycle::{BranchState, Start};
use thiserror::Error;

use crate::process::{Finished, run};

/// The base `worktree.base` defaults to.
const ORIGIN_HEAD: &str = "origin/HEAD";

/// The exit code of `git ls-remote --exit-code` when the remote has no
/// matching ref (it answered; the branch is not there).
const NO_MATCHING_REFS: i32 = 2;

/// Why a git step failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitError {
    /// The repository directory does not exist.
    #[error("project directory not found: {}", .0.display())]
    NoRepo(PathBuf),
    /// Something already exists where the worktree goes.
    #[error("{} already exists; tasq does not create a worktree over it", .0.display())]
    Exists(PathBuf),
    /// A git command exited with an error.
    #[error("git {command} failed: {message}")]
    Failed {
        /// The command line after `git`.
        command: String,
        /// What git printed, trimmed.
        message: String,
    },
}

/// What [`Git::add`] did besides creating the worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    /// Something the user should know (`origin/HEAD` not set, so the
    /// branch starts from `HEAD`).
    pub note: Option<String>,
}

/// `git`, with the environment every process gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    /// The environment the processes run with (`PATH` and `HOME` at least).
    pub env: Vec<(String, String)>,
}

impl Git {
    fn run(&self, repo: &Path, args: &[&str]) -> Finished {
        run("git", args, repo, &self.env)
    }

    fn check(&self, repo: &Path, args: &[&str]) -> Result<Finished, GitError> {
        let out = self.run(repo, args);
        if out.success {
            Ok(out)
        } else {
            Err(GitError::Failed {
                command: args.join(" "),
                message: out.message(),
            })
        }
    }

    fn has_ref(&self, repo: &Path, reference: &str) -> bool {
        self.run(repo, &["show-ref", "--verify", "--quiet", reference])
            .success
    }

    /// The branch checked out at `dir` (`git branch --show-current`),
    /// `None` when it is not a repository, is detached or git is missing.
    pub fn current_branch(&self, dir: &Path) -> Option<String> {
        if !dir.is_dir() {
            return None;
        }
        let out = self.run(dir, &["branch", "--show-current"]);
        let branch = out.stdout.trim();
        (out.success && !branch.is_empty()).then(|| branch.to_owned())
    }

    /// Where `branch` lives for the repository at `repo`: a local branch;
    /// else on `origin` (asked with `ls-remote`, then fetched into
    /// `origin/<branch>` when `fetch`); else nowhere. When `origin` cannot
    /// be asked (no such remote, offline), an `origin/<branch>` already
    /// fetched counts.
    ///
    /// # Errors
    ///
    /// [`GitError::NoRepo`] for a missing directory, [`GitError::Failed`]
    /// when the fetch fails.
    pub fn branch_state(
        &self,
        repo: &Path,
        branch: &str,
        fetch: bool,
    ) -> Result<BranchState, GitError> {
        if !repo.is_dir() {
            return Err(GitError::NoRepo(repo.to_path_buf()));
        }
        if self.has_ref(repo, &format!("refs/heads/{branch}")) {
            return Ok(BranchState::Local);
        }
        let remote = format!("origin/{branch}");
        let heads = format!("refs/heads/{branch}");
        let asked = self.run(repo, &["ls-remote", "--exit-code", "origin", &heads]);
        if asked.success {
            if fetch {
                let spec = format!("+{heads}:refs/remotes/{remote}");
                self.check(repo, &["fetch", "--quiet", "origin", &spec])?;
            }
            return Ok(BranchState::Remote(remote));
        }
        if asked.code == Some(NO_MATCHING_REFS) {
            return Ok(BranchState::Absent);
        }
        if self.has_ref(repo, &format!("refs/remotes/{remote}")) {
            return Ok(BranchState::Remote(remote));
        }
        Ok(BranchState::Absent)
    }

    /// Creates the worktree at `path` on `branch` as `start` says, from the
    /// repository at `repo`. A new branch from `origin/HEAD` starts from
    /// `HEAD` when the repository has no `origin/HEAD`. When git fails,
    /// whatever appeared at `path` is removed and the worktree list pruned,
    /// so a retry starts clean.
    ///
    /// # Errors
    ///
    /// [`GitError::NoRepo`], [`GitError::Exists`] when `path` exists, or
    /// [`GitError::Failed`] with git's message.
    pub fn add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &str,
        start: &Start,
    ) -> Result<Added, GitError> {
        if !repo.is_dir() {
            return Err(GitError::NoRepo(repo.to_path_buf()));
        }
        if path.exists() {
            return Err(GitError::Exists(path.to_path_buf()));
        }
        let mut added = Added::default();
        let start = match start {
            Start::New(base)
                if base == ORIGIN_HEAD && !self.has_ref(repo, "refs/remotes/origin/HEAD") =>
            {
                added.note = Some(format!(
                    "{ORIGIN_HEAD} is not set; {branch} starts from HEAD \
                     (set worktree.base, or run git remote set-head origin --auto)"
                ));
                Start::New("HEAD".to_owned())
            }
            other => other.clone(),
        };
        let args = add_args(path, branch, &start);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.check(repo, &args) {
            Ok(_) => Ok(added),
            Err(e) => {
                if path.exists() {
                    let _ = std::fs::remove_dir_all(path);
                }
                let _ = self.run(repo, &["worktree", "prune"]);
                Err(e)
            }
        }
    }
}

/// The `git` arguments that create the worktree at `path` on `branch`:
/// check out a local branch, create one tracking a remote branch, or
/// create one from a base without tracking it (a branch started from
/// `origin/main` must not push to `main`).
pub fn add_args(path: &Path, branch: &str, start: &Start) -> Vec<String> {
    let path = path.display().to_string();
    let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
    match start {
        Start::Local => args.extend([path, branch.to_owned()]),
        Start::Track(remote) => {
            args.extend([
                "--track".into(),
                "-b".into(),
                branch.to_owned(),
                path,
                remote.clone(),
            ]);
        }
        Start::New(base) => {
            args.extend([
                "--no-track".into(),
                "-b".into(),
                branch.to_owned(),
                path,
                base.clone(),
            ]);
        }
    }
    args
}

/// One line for `--dry-run` and progress: the path, the branch and where
/// the branch comes from.
pub fn describe(path: &Path, branch: &str, start: &Start) -> String {
    let from = match start {
        Start::Local => "the local branch".to_owned(),
        Start::Track(remote) => format!("tracking {remote}"),
        Start::New(base) => format!("new from {base}"),
    };
    format!("create worktree {} on {branch} ({from})", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_args_per_start() {
        let path = Path::new("/wt/app-b");
        assert_eq!(
            add_args(path, "b", &Start::Local),
            ["worktree", "add", "/wt/app-b", "b"]
        );
        assert_eq!(
            add_args(path, "b", &Start::Track("origin/b".into())),
            [
                "worktree",
                "add",
                "--track",
                "-b",
                "b",
                "/wt/app-b",
                "origin/b"
            ]
        );
        assert_eq!(
            add_args(path, "b", &Start::New("origin/HEAD".into())),
            [
                "worktree",
                "add",
                "--no-track",
                "-b",
                "b",
                "/wt/app-b",
                "origin/HEAD"
            ]
        );
    }

    #[test]
    fn descriptions() {
        let path = Path::new("/wt/x");
        assert_eq!(
            describe(path, "b", &Start::Local),
            "create worktree /wt/x on b (the local branch)"
        );
        assert_eq!(
            describe(path, "b", &Start::Track("origin/b".into())),
            "create worktree /wt/x on b (tracking origin/b)"
        );
        assert_eq!(
            describe(path, "b", &Start::New("main".into())),
            "create worktree /wt/x on b (new from main)"
        );
    }

    #[test]
    fn errors_read_well() {
        assert_eq!(
            GitError::Exists("/wt/x".into()).to_string(),
            "/wt/x already exists; tasq does not create a worktree over it"
        );
        assert_eq!(
            GitError::NoRepo("/p".into()).to_string(),
            "project directory not found: /p"
        );
        assert_eq!(
            GitError::Failed {
                command: "worktree add".into(),
                message: "fatal: x".into()
            }
            .to_string(),
            "git worktree add failed: fatal: x"
        );
    }
}
