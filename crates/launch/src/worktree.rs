//! The worktree manager: plain `git worktree add`.
//!
//! It decides whether the branch is new the way the script did: it exists
//! when `git show-ref` finds it locally or `git ls-remote` finds it on
//! `origin`; otherwise it is created with `-b`.

use std::path::Path;

use tasq_core::work::{CreatedWorktree, WorkError, WorktreeManager, sibling_worktree};

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
