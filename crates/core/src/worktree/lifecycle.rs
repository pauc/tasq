//! The worktree lifecycle: declare, materialize at launch, release at close
//! (plan 4.2).
//!
//! Each step is a function from what the caller observed (does the directory
//! exist, is it dirty, where does the branch live) to what should happen.
//! Nothing here runs a process; the observations come in as values or
//! closures, so every rule is a table test.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{Task, Worktree};

/// Why a lifecycle step cannot proceed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LifecycleError {
    /// A branch name that is empty or contains whitespace.
    #[error("invalid branch name {0:?}")]
    InvalidBranch(String),
}

/// The outcome of [`declare`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declared {
    /// A new entry to add to the task.
    New(Worktree),
    /// The task already declares a worktree on this branch or at this path;
    /// nothing to write.
    Already(Worktree),
}

/// Declares a worktree for `task` (step 1) on `branch`, at
/// `path_for(branch)`. Runs no process and creates nothing; the caller
/// stores the entry. [`super::declaration::declare`] renders the branch
/// and path from the configuration.
///
/// # Errors
///
/// [`LifecycleError::InvalidBranch`] from [`branch_name`], before
/// `path_for` runs; otherwise whatever `path_for` returns.
pub fn declare<E: From<LifecycleError>>(
    task: &Task,
    branch: &str,
    path_for: impl FnOnce(&str) -> Result<PathBuf, E>,
) -> Result<Declared, E> {
    let branch = branch_name(branch)?;
    if let Some(existing) = task
        .worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(branch))
    {
        return Ok(Declared::Already(existing.clone()));
    }
    let path = path_for(branch)?;
    if let Some(existing) = task.worktrees.iter().find(|w| w.path == path) {
        return Ok(Declared::Already(existing.clone()));
    }
    Ok(Declared::New(Worktree::on_branch(path, branch)))
}

/// `branch` trimmed, if it can name a declared branch.
///
/// # Errors
///
/// [`LifecycleError::InvalidBranch`] for an empty branch or one with
/// whitespace (git refuses those, and the file format cannot hold them).
pub fn branch_name(branch: &str) -> Result<&str, LifecycleError> {
    let branch = branch.trim();
    if branch.is_empty() || branch.contains(char::is_whitespace) {
        return Err(LifecycleError::InvalidBranch(branch.to_owned()));
    }
    Ok(branch)
}

/// Where a branch lives, as observed before creating a worktree for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchState {
    /// A local branch of that name exists.
    Local,
    /// Only the remote has it, under this ref (`origin/<branch>`).
    Remote(String),
    /// Neither the repository nor its remote has it.
    Absent,
}

/// How a worktree is created for its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Check out the existing local branch.
    Local,
    /// Create the local branch tracking this remote ref.
    Track(String),
    /// Create the branch from this base ref (a fresh start).
    New(String),
}

/// What a launch does about worktrees (step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchAction {
    /// No declared worktree exists or can be created: start in the
    /// project.
    None,
    /// Start in this existing worktree.
    Reuse(PathBuf),
    /// Create the worktree, then start in it.
    Materialize {
        /// Where, as declared.
        path: PathBuf,
        /// The branch to check out.
        branch: String,
        /// How the branch is obtained.
        start: Start,
    },
}

/// Decides what a launch does (step 2). The first declared worktree that
/// exists is reused; when none exists the first declared one that
/// records a branch is created, from the local branch, else the remote
/// one, else `base`. Entries without a branch (directories tracked by
/// hand) cannot be created again and are passed over.
pub fn on_launch(
    worktrees: &[Worktree],
    exists: impl Fn(&Path) -> bool,
    branch_state: impl FnOnce(&str) -> BranchState,
    base: &str,
) -> LaunchAction {
    if let Some(found) = worktrees.iter().find(|w| exists(&w.path)) {
        return LaunchAction::Reuse(found.path.clone());
    }
    let Some((first, branch)) = worktrees
        .iter()
        .find_map(|w| w.branch.as_ref().map(|b| (w, b.clone())))
    else {
        return LaunchAction::None;
    };
    let start = match branch_state(&branch) {
        BranchState::Local => Start::Local,
        BranchState::Remote(remote) => Start::Track(remote),
        BranchState::Absent => Start::New(base.to_owned()),
    };
    LaunchAction::Materialize {
        path: first.path.clone(),
        branch,
        start,
    }
}

/// Whether a release may delete branches (`worktree.delete_branch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeleteBranch {
    /// Delete a branch when nothing is lost: it is integrated into the
    /// default branch, or its upstream contains it.
    Safe,
    /// Never delete branches; only directories are removed.
    Never,
}

/// What the caller observed about a declared worktree's directory before a
/// release. When several apply, the first in this order wins (main, then
/// locked, then dirty), so the caller can stop looking early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirSeen {
    /// The directory does not exist.
    Absent,
    /// It is the repository's main worktree (the project checkout).
    Main,
    /// `git worktree lock` holds it.
    Locked,
    /// It has modified, staged or untracked files.
    Dirty,
    /// None of the above.
    Clean,
}

/// What the caller observed about a declared worktree's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSeen {
    /// No local branch of that name.
    Absent,
    /// The local branch exists.
    Present {
        /// Its tip is integrated into the default branch.
        integrated: bool,
        /// Its upstream on the remote contains its tip.
        pushed: bool,
    },
}

/// Why a worktree directory is kept by a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepWorktree {
    /// It is the main checkout, which is never removed.
    Main,
    /// It is locked.
    Locked,
    /// It has uncommitted or untracked work.
    Dirty,
}

/// What a release does with the directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeFate {
    /// There is no directory; nothing to do.
    Absent,
    /// Remove it (with the remove hooks).
    Remove,
    /// Leave it, for this reason.
    Keep(KeepWorktree),
}

/// Why a branch is kept by a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepBranch {
    /// Its worktree is kept, and a checked-out branch cannot be deleted.
    CheckedOut,
    /// `delete_branch = "never"`.
    Policy,
    /// Deleting it would lose commits: neither integrated nor pushed.
    Unsafe,
}

/// What a release does with the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchFate {
    /// No local branch; nothing to do.
    Absent,
    /// Delete the local branch, after the directory is gone.
    Delete,
    /// Leave it, for this reason.
    Keep(KeepBranch),
}

/// The release decision for one declared worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    /// The directory.
    pub worktree: WorktreeFate,
    /// The branch.
    pub branch: BranchFate,
}

/// Decides what closing a task does with one declared worktree (step 3).
/// The entry itself always stays.
pub fn on_close(dir: DirSeen, branch: BranchSeen, policy: DeleteBranch) -> Release {
    let worktree = match dir {
        DirSeen::Absent => WorktreeFate::Absent,
        DirSeen::Main => WorktreeFate::Keep(KeepWorktree::Main),
        DirSeen::Locked => WorktreeFate::Keep(KeepWorktree::Locked),
        DirSeen::Dirty => WorktreeFate::Keep(KeepWorktree::Dirty),
        DirSeen::Clean => WorktreeFate::Remove,
    };
    let branch = match branch {
        BranchSeen::Absent => BranchFate::Absent,
        BranchSeen::Present { .. } if matches!(worktree, WorktreeFate::Keep(_)) => {
            BranchFate::Keep(KeepBranch::CheckedOut)
        }
        BranchSeen::Present { .. } if policy == DeleteBranch::Never => {
            BranchFate::Keep(KeepBranch::Policy)
        }
        BranchSeen::Present { integrated, pushed } if integrated || pushed => BranchFate::Delete,
        BranchSeen::Present { .. } => BranchFate::Keep(KeepBranch::Unsafe),
    };
    Release { worktree, branch }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskId;

    fn task(worktrees: &[Worktree]) -> Task {
        let mut t = Task::new(TaskId::from(7), "Fix login");
        for w in worktrees {
            t.add_worktree(w.clone());
        }
        t
    }

    fn sibling(branch: &str) -> PathBuf {
        PathBuf::from(format!("/code/app-{branch}"))
    }

    #[test]
    fn declare_trims_the_branch() {
        assert_eq!(
            declare(&task(&[]), " feat/x ", |b| Ok::<_, LifecycleError>(
                sibling(b)
            )),
            Ok(Declared::New(Worktree::on_branch(
                "/code/app-feat/x",
                "feat/x"
            )))
        );
    }

    #[test]
    fn declare_rejects_empty_and_whitespace_branches() {
        let t = task(&[]);
        for bad in ["", "  ", "a b", "a\tb"] {
            assert_eq!(
                declare(&t, bad, |_| -> Result<PathBuf, LifecycleError> {
                    unreachable!()
                }),
                Err(LifecycleError::InvalidBranch(bad.trim().into())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn declare_passes_on_path_errors() {
        assert_eq!(
            declare(&task(&[]), "b", |_| Err(LifecycleError::InvalidBranch(
                "/p".into()
            ))),
            Err(LifecycleError::InvalidBranch("/p".into()))
        );
    }

    #[test]
    fn declare_is_idempotent_by_branch_or_path() {
        let same_branch = Worktree::on_branch("/elsewhere", "b");
        let t = task(std::slice::from_ref(&same_branch));
        assert_eq!(
            declare(&t, "b", |_| -> Result<PathBuf, LifecycleError> {
                unreachable!()
            }),
            Ok(Declared::Already(same_branch))
        );
        let same_path = Worktree::new("/code/app-c");
        let t = task(std::slice::from_ref(&same_path));
        assert_eq!(
            declare(&t, "c", |b| Ok::<_, LifecycleError>(sibling(b))),
            Ok(Declared::Already(same_path))
        );
        let other = task(&[Worktree::on_branch("/code/app-d", "d")]);
        assert_eq!(
            declare(&other, "c", |b| Ok::<_, LifecycleError>(sibling(b))),
            Ok(Declared::New(Worktree::on_branch("/code/app-c", "c")))
        );
    }

    #[test]
    fn launch_without_worktrees_starts_in_the_project() {
        assert_eq!(
            on_launch(&[], |_| true, |_| unreachable!(), "origin/HEAD"),
            LaunchAction::None
        );
    }

    #[test]
    fn launch_reuses_the_first_existing_worktree() {
        let wts = [
            Worktree::on_branch("/gone", "a"),
            Worktree::on_branch("/here", "b"),
            Worktree::on_branch("/also", "c"),
        ];
        assert_eq!(
            on_launch(
                &wts,
                |p| p != Path::new("/gone"),
                |_| unreachable!(),
                "main"
            ),
            LaunchAction::Reuse(PathBuf::from("/here"))
        );
    }

    #[test]
    fn launch_materializes_the_first_declared_from_where_the_branch_lives() {
        let wts = [
            Worktree::on_branch("/a", "x"),
            Worktree::on_branch("/b", "y"),
        ];
        let cases = [
            (BranchState::Local, Start::Local),
            (
                BranchState::Remote("origin/x".into()),
                Start::Track("origin/x".into()),
            ),
            (BranchState::Absent, Start::New("origin/HEAD".into())),
        ];
        for (state, start) in cases {
            let asked = std::cell::RefCell::new(String::new());
            let action = on_launch(
                &wts,
                |_| false,
                |b| {
                    asked.replace(b.to_owned());
                    state.clone()
                },
                "origin/HEAD",
            );
            assert_eq!(
                action,
                LaunchAction::Materialize {
                    path: PathBuf::from("/a"),
                    branch: "x".into(),
                    start,
                }
            );
            assert_eq!(asked.into_inner(), "x");
        }
    }

    #[test]
    fn launch_passes_over_declarations_without_a_branch() {
        assert_eq!(
            on_launch(&[Worktree::new("/a")], |_| false, |_| unreachable!(), "m"),
            LaunchAction::None
        );
        assert_eq!(
            on_launch(
                &[Worktree::new("/a"), Worktree::on_branch("/b", "y")],
                |_| false,
                |_| BranchState::Local,
                "m"
            ),
            LaunchAction::Materialize {
                path: PathBuf::from("/b"),
                branch: "y".into(),
                start: Start::Local,
            }
        );
        assert_eq!(
            LifecycleError::InvalidBranch("a b".into()).to_string(),
            "invalid branch name \"a b\""
        );
    }

    const UNSAFE: BranchSeen = BranchSeen::Present {
        integrated: false,
        pushed: false,
    };
    const MERGED: BranchSeen = BranchSeen::Present {
        integrated: true,
        pushed: false,
    };
    const PUSHED: BranchSeen = BranchSeen::Present {
        integrated: false,
        pushed: true,
    };
    const BOTH: BranchSeen = BranchSeen::Present {
        integrated: true,
        pushed: true,
    };

    fn release(worktree: WorktreeFate, branch: BranchFate) -> Release {
        Release { worktree, branch }
    }

    #[test]
    fn close_removes_a_clean_worktree_and_deletes_only_a_safe_branch() {
        use BranchFate::{Absent, Delete, Keep};
        use DeleteBranch::{Never, Safe};
        use KeepBranch::{Policy, Unsafe};
        let cases = [
            (MERGED, Safe, Delete),
            (PUSHED, Safe, Delete),
            (BOTH, Safe, Delete),
            (UNSAFE, Safe, Keep(Unsafe)),
            (BOTH, Never, Keep(Policy)),
            (UNSAFE, Never, Keep(Policy)),
            (BranchSeen::Absent, Safe, Absent),
        ];
        for (branch, policy, fate) in cases {
            assert_eq!(
                on_close(DirSeen::Clean, branch, policy),
                release(WorktreeFate::Remove, fate),
                "{branch:?} {policy:?}"
            );
        }
    }

    #[test]
    fn close_keeps_main_locked_and_dirty_worktrees_and_their_branches() {
        let cases = [
            (DirSeen::Main, KeepWorktree::Main),
            (DirSeen::Locked, KeepWorktree::Locked),
            (DirSeen::Dirty, KeepWorktree::Dirty),
        ];
        for (dir, why) in cases {
            assert_eq!(
                on_close(dir, BOTH, DeleteBranch::Safe),
                release(
                    WorktreeFate::Keep(why),
                    BranchFate::Keep(KeepBranch::CheckedOut)
                ),
                "{dir:?}"
            );
            assert_eq!(
                on_close(dir, BranchSeen::Absent, DeleteBranch::Safe),
                release(WorktreeFate::Keep(why), BranchFate::Absent),
                "{dir:?}"
            );
        }
    }

    #[test]
    fn close_with_the_directory_gone_still_decides_the_branch() {
        use BranchFate::{Absent, Delete, Keep};
        let cases = [
            (MERGED, DeleteBranch::Safe, Delete),
            (UNSAFE, DeleteBranch::Safe, Keep(KeepBranch::Unsafe)),
            (MERGED, DeleteBranch::Never, Keep(KeepBranch::Policy)),
            (BranchSeen::Absent, DeleteBranch::Safe, Absent),
        ];
        for (branch, policy, fate) in cases {
            assert_eq!(
                on_close(DirSeen::Absent, branch, policy),
                release(WorktreeFate::Absent, fate),
                "{branch:?} {policy:?}"
            );
        }
    }
}
