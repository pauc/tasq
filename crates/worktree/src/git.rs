//! `git` for one repository: where a branch lives, the branch checked out
//! in a directory, `git worktree add` with cleanup on failure, whether
//! deleting a branch would lose anything, and what a release runs: the
//! worktree list, the status of one, `git worktree remove`, `git branch -D`.

use std::path::{Path, PathBuf};

use tasq_core::worktree::integration::{self, Check, Safety};
use tasq_core::worktree::lifecycle::{BranchState, Start};
use tasq_core::worktree::status::{self, Listed};
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

    /// Where the tip of the local `branch` is safe, for a release: the
    /// default branch it is integrated into (the checks of
    /// [`integration::Check`], cheapest first, against each of
    /// [`integration::targets`]) and the remote ref that contains it. `None`
    /// when there is no such local branch. Reads the repository only; the
    /// remote-tracking refs are as last fetched. A check git cannot answer
    /// counts as failed, so doubt keeps the branch.
    ///
    /// # Errors
    ///
    /// [`GitError::NoRepo`] for a missing directory.
    pub fn safety(&self, repo: &Path, branch: &str) -> Result<Option<Safety>, GitError> {
        if !repo.is_dir() {
            return Err(GitError::NoRepo(repo.to_path_buf()));
        }
        let own = format!("refs/heads/{branch}");
        if !self.has_ref(repo, &own) {
            return Ok(None);
        }
        let origin_head = self.output(repo, &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"]);
        let targets =
            integration::targets(origin_head.as_deref(), branch, |r| self.has_ref(repo, r));
        let integrated = targets.into_iter().find_map(|target| {
            integration::first_passing(|check| self.passes(repo, check, &own, &target))
                .map(|check| (target, check))
        });
        let upstream = self.output(
            repo,
            &[
                "rev-parse",
                "--symbolic-full-name",
                &format!("{own}@{{upstream}}"),
            ],
        );
        let remote = integration::pushed_ref(upstream.as_deref(), branch);
        let pushed = self.contains(repo, &own, &remote).then_some(remote);
        Ok(Some(Safety { integrated, pushed }))
    }

    /// The repository's worktrees, the main one first.
    ///
    /// # Errors
    ///
    /// [`GitError::NoRepo`], or [`GitError::Failed`] outside a repository.
    pub fn worktrees(&self, repo: &Path) -> Result<Vec<Listed>, GitError> {
        if !repo.is_dir() {
            return Err(GitError::NoRepo(repo.to_path_buf()));
        }
        let out = self.check(repo, &["worktree", "list", "--porcelain", "-z"])?;
        Ok(status::listed(&out.stdout))
    }

    /// The modified, staged and untracked paths of the worktree at `dir`,
    /// leaving out the `provisioned` ones (relative to `dir`).
    ///
    /// # Errors
    ///
    /// [`GitError::NoRepo`], or [`GitError::Failed`] when git cannot say.
    pub fn changes(&self, dir: &Path, provisioned: &[PathBuf]) -> Result<Vec<String>, GitError> {
        if !dir.is_dir() {
            return Err(GitError::NoRepo(dir.to_path_buf()));
        }
        let out = self.check(dir, &["status", "--porcelain", "-z"])?;
        Ok(status::changes(&out.stdout, provisioned))
    }

    /// `git worktree remove --force <path>` from the repository at `repo`.
    /// The caller decided it loses nothing: `--force` only lets the
    /// provisioned untracked files go; a locked worktree is still refused.
    ///
    /// # Errors
    ///
    /// [`GitError::Failed`] with git's message.
    pub fn remove(&self, repo: &Path, path: &Path) -> Result<(), GitError> {
        let path = path.display().to_string();
        self.check(repo, &["worktree", "remove", "--force", &path])
            .map(drop)
    }

    /// `git worktree prune --verbose`: forgets worktrees whose directory is
    /// gone, so their branches can be deleted; with `dry_run`, only says
    /// which. The lines git reports, one per forgotten worktree
    /// (`Removing worktrees/<name>: <why>`).
    ///
    /// # Errors
    ///
    /// [`GitError::Failed`] with git's message.
    pub fn prune(&self, repo: &Path, dry_run: bool) -> Result<Vec<String>, GitError> {
        let mut args = vec!["worktree", "prune", "--verbose"];
        if dry_run {
            args.push("--dry-run");
        }
        let out = self.check(repo, &args)?;
        // git reports on stderr; stdout is read too in case that changes.
        Ok(out
            .stderr
            .lines()
            .chain(out.stdout.lines())
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// `git branch -D <branch>`: the caller decided nothing is lost.
    ///
    /// # Errors
    ///
    /// [`GitError::Failed`] with git's message (checked out somewhere).
    pub fn delete_branch(&self, repo: &Path, branch: &str) -> Result<(), GitError> {
        self.check(repo, &["branch", "-D", branch]).map(drop)
    }

    /// Whether `check` passes for `branch` against `target` (both full refs).
    fn passes(&self, repo: &Path, check: Check, branch: &str, target: &str) -> bool {
        match check {
            Check::SameCommit => self.same(repo, "commit", branch, target),
            Check::Ancestor => self.contains(repo, branch, target),
            Check::NoAddedChanges => {
                self.run(
                    repo,
                    &["diff", "--quiet", &format!("{target}...{branch}"), "--"],
                )
                .success
            }
            Check::SameTree => self.same(repo, "tree", branch, target),
            Check::MergeAddsNothing => {
                let merged = self.run(repo, &["merge-tree", "--write-tree", target, branch]);
                merged.success
                    && integration::merge_tree(&merged.stdout)
                        == self.object(repo, target, "tree").as_deref()
            }
            Check::PatchesUpstream => {
                let cherry = self.run(repo, &["cherry", target, branch]);
                cherry.success && integration::cherry_all_upstream(&cherry.stdout)
            }
        }
    }

    /// Whether `a` and `b` peel to the same object of `kind`.
    fn same(&self, repo: &Path, kind: &str, a: &str, b: &str) -> bool {
        let a = self.object(repo, a, kind);
        a.is_some() && a == self.object(repo, b, kind)
    }

    /// The id of `reference` peeled to `kind` (`commit`, `tree`).
    fn object(&self, repo: &Path, reference: &str, kind: &str) -> Option<String> {
        self.output(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{{kind}}}"),
            ],
        )
    }

    /// Whether `tip` is an ancestor of (or is) `of`; a missing ref is not.
    fn contains(&self, repo: &Path, tip: &str, of: &str) -> bool {
        self.run(repo, &["merge-base", "--is-ancestor", tip, of])
            .success
    }

    /// The trimmed standard output of a successful command (the ones run
    /// here always print when they succeed).
    fn output(&self, repo: &Path, args: &[&str]) -> Option<String> {
        let out = self.run(repo, args);
        out.success.then(|| out.stdout.trim().to_owned())
    }

    /// `start` as it will run in the repository at `repo`: a new branch
    /// from `origin/HEAD` starts from `HEAD` when the repository has no
    /// `origin/HEAD` (a clone always has one; `git init` plus `remote add`
    /// does not), with a note saying so. Reads refs only.
    pub fn resolve_start(
        &self,
        repo: &Path,
        branch: &str,
        start: &Start,
    ) -> (Start, Option<String>) {
        match start {
            Start::New(base)
                if base == ORIGIN_HEAD && !self.has_ref(repo, "refs/remotes/origin/HEAD") =>
            {
                let note = format!(
                    "{ORIGIN_HEAD} is not set; {branch} starts from HEAD \
                     (set worktree.base, or run git remote set-head origin --auto)"
                );
                (Start::New("HEAD".to_owned()), Some(note))
            }
            other => (other.clone(), None),
        }
    }

    /// Creates the worktree at `path` on `branch` as `start` says, from the
    /// repository at `repo`. When git fails, whatever appeared at `path`
    /// is removed and the worktree list pruned, so a retry starts clean.
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
    ) -> Result<(), GitError> {
        if !repo.is_dir() {
            return Err(GitError::NoRepo(repo.to_path_buf()));
        }
        if path.exists() {
            return Err(GitError::Exists(path.to_path_buf()));
        }
        let args = add_args(path, branch, start);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.check(repo, &args) {
            Ok(_) => Ok(()),
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

/// The worktree to create, for `--dry-run` and progress lines: its path,
/// its branch and where the branch comes from (`/wt/app-b on b (new from
/// origin/HEAD)`).
pub fn describe(path: &Path, branch: &str, start: &Start) -> String {
    let from = match start {
        Start::Local => "the local branch".to_owned(),
        Start::Track(remote) => format!("tracking {remote}"),
        Start::New(base) => format!("new from {base}"),
    };
    format!("{} on {branch} ({from})", path.display())
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
            "/wt/x on b (the local branch)"
        );
        assert_eq!(
            describe(path, "b", &Start::Track("origin/b".into())),
            "/wt/x on b (tracking origin/b)"
        );
        assert_eq!(
            describe(path, "b", &Start::New("main".into())),
            "/wt/x on b (new from main)"
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
