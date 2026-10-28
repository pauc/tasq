//! Step 3 of the lifecycle: releasing a task's declared worktrees, as
//! closing it does. [`tasq_core::worktree::lifecycle::on_close`] decides
//! what to remove and delete; this module looks at the directory and the
//! branch, runs `git` and the `pre-remove` and `post-remove` hooks around
//! the removal.

use std::path::{Path, PathBuf};

use tasq_core::config::OnDone;
use tasq_core::model::{Task, Worktree};
use tasq_core::worktree::hooks::{self, HookEvent, Hooks, Kind};
use tasq_core::worktree::integration::Safety;
use tasq_core::worktree::lifecycle::{self, BranchFate, BranchSeen, WorktreeFate};
use tasq_core::worktree::status;
use tasq_core::worktree::vars::Vars;
use thiserror::Error;

use crate::hooks::{HookFailure, prepare, run_all};
use crate::{Git, GitError, RepoSettings};

/// How [`release`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Releasing {
    /// Remove dirty worktrees and delete unmerged, unpushed branches too.
    pub force: bool,
    /// Say what would happen; change nothing.
    pub dry_run: bool,
    /// Called for a close: `worktree.on_done = "keep"` skips it all.
    pub closing: bool,
}

impl Releasing {
    /// For a task being closed (`tasq done`, the UI): never forced, never
    /// a dry run, and subject to `worktree.on_done`.
    pub fn closing() -> Self {
        Self {
            force: false,
            dry_run: false,
            closing: true,
        }
    }

    /// Asked for by name (`tasq worktree release`, `tasq worktree prune`),
    /// whatever the task's status and `worktree.on_done`.
    pub fn on_demand(force: bool, dry_run: bool) -> Self {
        Self {
            force,
            dry_run,
            closing: false,
        }
    }
}

/// Why worktrees could not be released.
#[derive(Debug, Error)]
pub enum ReleaseError {
    /// The task's project is not in a git repository.
    #[error("{} is not in a git repository", .0.display())]
    NotARepository(PathBuf),
    /// `git` cannot list the worktrees or tell where a branch is safe.
    #[error(transparent)]
    Git(#[from] GitError),
    /// A hook cannot be prepared, or a `post-remove` one cannot run.
    #[error(transparent)]
    Hooks(#[from] HookFailure),
}

/// Step 3 of the lifecycle for every declared worktree of `task`, in the
/// repository `settings` describes: a clean one is removed (with the
/// `pre-remove` hook in it before, the `post-remove` one in the main
/// checkout after), and its branch deleted when that loses nothing
/// ([`Git::safety`]). A worktree holding `cwd` is kept. What is kept is a
/// warning through `report`, with the command that finishes the job when
/// there is one. The entries stay. The lines say what was done (or would
/// be, on a dry run). Hooks run with `git.env` plus the hook variables.
///
/// # Errors
///
/// [`ReleaseError`] when the repository cannot be read or a hook cannot be
/// prepared; a removal or a deletion that fails is a warning.
pub fn release(
    git: &Git,
    task: &Task,
    settings: &RepoSettings,
    cwd: &Path,
    how: Releasing,
    report: &mut dyn FnMut(HookEvent),
) -> Result<Vec<String>, ReleaseError> {
    if how.closing && settings.config.on_done == OnDone::Keep {
        return Ok(Vec::new());
    }
    if settings.repo.is_none() {
        return Err(ReleaseError::NotARepository(settings.project.clone()));
    }
    let listed = git.worktrees(&settings.main)?;
    let main = canonical(&settings.main);
    let cwd = canonical(cwd);
    let mut lines = Vec::new();
    for worktree in &task.worktrees {
        let path = canonical(&worktree.path);
        let provisioned = settings.provisioned(&path);
        let dir = status::dir_seen(&path, path.is_dir(), &main, &cwd, &listed, || {
            // Doubt keeps it: an unreadable status counts as a change.
            git.changes(&path, &provisioned)
                .unwrap_or_else(|e| vec![e.to_string()])
        });
        let safety = match &worktree.branch {
            Some(branch) => git.safety(&settings.main, branch)?,
            None => None,
        };
        let seen = safety.as_ref().map_or(BranchSeen::Absent, Safety::seen);
        let decided = lifecycle::on_close(dir, seen, settings.config.delete_branch, how.force);
        let job = WorktreeRelease {
            git,
            settings,
            task,
            worktree,
            path: &path,
            how,
        };
        let gone = job.directory(decided.worktree, &mut lines, report)?;
        job.branch(decided.branch, gone, safety.as_ref(), &mut lines, report);
    }
    Ok(lines)
}

/// One declared worktree being released, with what releasing it needs.
struct WorktreeRelease<'a> {
    git: &'a Git,
    settings: &'a RepoSettings,
    task: &'a Task,
    worktree: &'a Worktree,
    path: &'a Path,
    how: Releasing,
}

impl WorktreeRelease<'_> {
    /// Carries out `fate` for the directory; whether it is gone after.
    fn directory(
        &self,
        fate: WorktreeFate,
        lines: &mut Vec<String>,
        report: &mut dyn FnMut(HookEvent),
    ) -> Result<bool, HookFailure> {
        match fate {
            WorktreeFate::Absent => Ok(true),
            WorktreeFate::Keep(why) => {
                if let Some(warning) = why.warning(self.path, &self.task.id) {
                    report(HookEvent::Warning(warning));
                }
                Ok(false)
            }
            WorktreeFate::Remove if self.how.dry_run => {
                let hooks = self.hooks(&self.vars())?;
                lines.push(format!("would remove worktree {}", self.path.display()));
                lines.extend(
                    hooks
                        .iter()
                        .flat_map(Hooks::describe)
                        .map(|l| format!("  {l}")),
                );
                Ok(true)
            }
            WorktreeFate::Remove => self.remove(lines, report),
        }
    }

    /// The remove hooks around `git worktree remove`; whether it is gone.
    fn remove(
        &self,
        lines: &mut Vec<String>,
        report: &mut dyn FnMut(HookEvent),
    ) -> Result<bool, HookFailure> {
        let main = &self.settings.main;
        let path = self.path.display();
        let vars = self.vars();
        let [pre, post] = self.hooks(&vars)?;
        let mut env = self.git.env.clone();
        env.extend(hooks::env(&vars));
        if let Err(e) = run_all(&pre, self.path, &env, report) {
            report(HookEvent::Warning(format!("kept worktree {path}: {e}")));
            return Ok(false);
        }
        if let Err(e) = self.git.remove(main, self.path) {
            report(HookEvent::Warning(format!("kept worktree {path}: {e}")));
            return Ok(false);
        }
        lines.push(format!("removed worktree {path}"));
        run_all(&post, main, &env, report)?;
        Ok(true)
    }

    /// Carries out `fate` for the branch, once its directory is `gone`.
    fn branch(
        &self,
        fate: BranchFate,
        gone: bool,
        safety: Option<&Safety>,
        lines: &mut Vec<String>,
        report: &mut dyn FnMut(HookEvent),
    ) {
        let Some(branch) = &self.worktree.branch else {
            return;
        };
        let why = safety
            .and_then(Safety::reason)
            .unwrap_or_else(|| "forced".to_owned());
        match fate {
            BranchFate::Delete if !gone => {}
            BranchFate::Delete if self.how.dry_run => {
                lines.push(format!("would delete branch {branch} ({why})"));
            }
            BranchFate::Delete => {
                let main = &self.settings.main;
                // A directory removed by hand leaves its registration, and
                // git refuses to delete a branch checked out there.
                let _ = self.git.prune(main, false);
                match self.git.delete_branch(main, branch) {
                    Ok(()) => lines.push(format!("deleted branch {branch} ({why})")),
                    Err(e) => report(HookEvent::Warning(format!("kept branch {branch}: {e}"))),
                }
            }
            BranchFate::Keep(keep) => {
                if let Some(warning) = keep.warning(branch, &self.task.id) {
                    report(HookEvent::Warning(warning));
                }
            }
            BranchFate::Absent => {}
        }
    }

    /// The `pre-remove` and `post-remove` hooks of this worktree.
    fn hooks(&self, vars: &Vars) -> Result<[Hooks; 2], HookFailure> {
        prepare(self.settings, [Kind::PreRemove, Kind::PostRemove], vars)
    }

    /// The template variables of this worktree, for its hooks.
    fn vars(&self) -> Vars {
        self.settings.vars(
            self.task,
            self.worktree.branch.as_deref().unwrap_or_default(),
            self.path,
        )
    }
}

/// `path` with symlinks resolved when it exists, as git records paths.
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_is_never_forced_nor_a_dry_run() {
        assert_eq!(
            Releasing::closing(),
            Releasing {
                force: false,
                dry_run: false,
                closing: true,
            }
        );
    }

    #[test]
    fn on_demand_keeps_its_flags_and_ignores_on_done() {
        assert_eq!(
            Releasing::on_demand(true, true),
            Releasing {
                force: true,
                dry_run: true,
                closing: false,
            }
        );
        assert_eq!(
            Releasing::on_demand(false, false),
            Releasing {
                force: false,
                dry_run: false,
                closing: false,
            }
        );
    }
}
