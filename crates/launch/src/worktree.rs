//! Worktree managers: `gwm` (the author's tool, which links per-project
//! config into new worktrees) and plain `git worktree add`.
//!
//! Both decide whether the branch is new the way the script did: it exists
//! when `git show-ref` finds it locally or `git ls-remote` finds it on
//! `origin`; otherwise it is created with `-b`.

use std::path::{Path, PathBuf};

use tasq_core::config::WorktreeManager as Kind;
use tasq_core::work::{
    CreatedWorktree, GWM_MARKER, WorkError, WorktreeManager, find_up, sibling_worktree,
};

use crate::process::run;

/// Whether `branch` exists in the repository at `project`, locally or on
/// `origin`.
pub fn branch_exists(project: &Path, branch: &str, env: &[(String, String)]) -> bool {
    let local = run(
        "git",
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
        Some(project),
        env,
        &[],
    );
    if local.success {
        return true;
    }
    run(
        "git",
        &["ls-remote", "--exit-code", "--heads", "origin", branch],
        Some(project),
        env,
        &[],
    )
    .success
}

/// The branch checked out at `dir` (`git branch --show-current`), `None`
/// when it is not a repository, is detached or git is unavailable.
pub fn current_branch(dir: &Path, env: &[(String, String)]) -> Option<String> {
    let out = run("git", &["branch", "--show-current"], Some(dir), env, &[]);
    if !out.success {
        return None;
    }
    let branch = out.stdout.trim();
    (!branch.is_empty()).then(|| branch.to_owned())
}

/// The manager `work.worktree_manager` asks for.
pub fn manager_for(kind: Kind, env: Vec<(String, String)>) -> Box<dyn WorktreeManager> {
    match kind {
        Kind::Gwm => Box::new(GwmManager { env }),
        Kind::Git => Box::new(GitManager { env }),
    }
}

/// `gwm create [-b] <branch> --no-tmux -s`, run inside the project with
/// `GWM_SHELL_MODE=1`; the last line of its output is the worktree path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GwmManager {
    /// Environment the processes run with.
    pub env: Vec<(String, String)>,
}

impl WorktreeManager for GwmManager {
    fn name(&self) -> &'static str {
        "gwm"
    }

    fn create(&self, project: &Path, branch: &str) -> Result<CreatedWorktree, WorkError> {
        if !project.is_dir() {
            return Err(WorkError::ProjectMissing(project.to_path_buf()));
        }
        find_up(project, GWM_MARKER, Path::is_file).ok_or_else(|| WorkError::NoWorkspace {
            project: project.to_path_buf(),
        })?;
        let mut args = vec!["create"];
        if !branch_exists(project, branch, &self.env) {
            args.push("-b");
        }
        args.extend([branch, "--no-tmux", "-s"]);
        let out = run(
            "gwm",
            &args,
            Some(project),
            &self.env,
            &[("GWM_SHELL_MODE", "1")],
        );
        if !out.success {
            return Err(WorkError::Tool {
                tool: "gwm".to_owned(),
                branch: branch.to_owned(),
                message: out.message(),
            });
        }
        let mut lines: Vec<&str> = out
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        let path = lines.pop().map(|l| PathBuf::from(l.trim()));
        match path {
            Some(path) if path.is_dir() => Ok(CreatedWorktree {
                path,
                branch: branch.to_owned(),
                messages: lines.iter().map(|l| (*l).to_owned()).collect(),
            }),
            _ => Err(WorkError::NoPath {
                tool: "gwm".to_owned(),
                output: out.stdout.trim().to_owned(),
            }),
        }
    }
}

/// `git worktree add` into `<project>-<branch>` next to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitManager {
    /// Environment the processes run with.
    pub env: Vec<(String, String)>,
}

impl WorktreeManager for GitManager {
    fn name(&self) -> &'static str {
        "git"
    }

    fn create(&self, project: &Path, branch: &str) -> Result<CreatedWorktree, WorkError> {
        if !project.is_dir() {
            return Err(WorkError::ProjectMissing(project.to_path_buf()));
        }
        let path = sibling_worktree(project, branch);
        if path.is_dir() {
            return Ok(CreatedWorktree {
                path: path.clone(),
                branch: branch.to_owned(),
                messages: vec![format!("reusing existing worktree {}", path.display())],
            });
        }
        let path_text = path.display().to_string();
        let args: Vec<&str> = if branch_exists(project, branch, &self.env) {
            vec!["worktree", "add", &path_text, branch]
        } else {
            vec!["worktree", "add", "-b", branch, &path_text]
        };
        let out = run("git", &args, Some(project), &self.env, &[]);
        if !out.success {
            return Err(WorkError::Tool {
                tool: "git".to_owned(),
                branch: branch.to_owned(),
                message: out.message(),
            });
        }
        Ok(CreatedWorktree {
            path,
            branch: branch.to_owned(),
            messages: Vec::new(),
        })
    }
}
