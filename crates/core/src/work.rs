//! Work context: where a task is worked on.
//!
//! A task tracks a `## Project` directory and `## Worktrees`. Sessions start
//! in the first tracked worktree that exists, else the project, else the
//! configured default (plan FR-8; the resolver itself is T-401). This module
//! holds the pure pieces and the [`WorktreeManager`] extension point that
//! `tasq worktree --create` uses; the implementation (plain `git`) lives in
//! `tasq-launch`.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::Task;

/// Creates git worktrees for a project.
pub trait WorktreeManager {
    /// Implementation name (`git`), for messages.
    fn name(&self) -> &str;

    /// Makes (or finds) the worktree for `branch` of the repository at
    /// `project` and says where it is.
    fn create(&self, project: &Path, branch: &str) -> Result<CreatedWorktree, WorkError>;
}

/// What [`WorktreeManager::create`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktree {
    /// The worktree directory.
    pub path: PathBuf,
    /// The branch checked out there.
    pub branch: String,
    /// Lines the tool printed besides the path, for the user to see.
    pub messages: Vec<String>,
}

/// Why a worktree could not be created.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkError {
    /// The project directory does not exist.
    #[error("project directory not found: {0}")]
    ProjectMissing(PathBuf),
    /// The tool exited with an error.
    #[error("{tool} failed for branch '{branch}': {message}")]
    Tool {
        /// The program that ran (`git`).
        tool: String,
        /// The branch asked for.
        branch: String,
        /// The tool's output, trimmed.
        message: String,
    },
}

/// The directory a task's work happens in when no worktree is involved:
/// its `## Project`, else `default` (`work.default_project`).
pub fn project_dir(task: &Task, default: Option<&Path>) -> Option<PathBuf> {
    task.project
        .clone()
        .or_else(|| default.map(Path::to_path_buf))
}

/// A branch name as a directory name: `/`, whitespace and anything that is
/// not alphanumeric, `.`, `_` or `-` becomes `-`; runs collapse.
pub fn branch_slug(branch: &str) -> String {
    let mut slug = String::with_capacity(branch.len());
    for c in branch.chars() {
        if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// Where the plain-git manager puts the worktree for `branch`: next to the
/// project, as `<project>-<slug>`.
pub fn sibling_worktree(project: &Path, branch: &str) -> PathBuf {
    let name = project.file_name().map_or_else(
        || "worktree".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let dir = format!("{name}-{}", branch_slug(branch));
    match project.parent() {
        Some(parent) => parent.join(dir),
        None => project.join(dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskId;

    #[test]
    fn project_dir_prefers_the_task() {
        let mut task = Task::new(TaskId::from(1), "T");
        assert_eq!(project_dir(&task, None), None);
        assert_eq!(
            project_dir(&task, Some(Path::new("/default"))),
            Some(PathBuf::from("/default"))
        );
        task.project = Some("/proj".into());
        assert_eq!(
            project_dir(&task, Some(Path::new("/default"))),
            Some(PathBuf::from("/proj"))
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(branch_slug("feature/login-form"), "feature-login-form");
        assert_eq!(branch_slug("fix  spaces"), "fix-spaces");
        assert_eq!(branch_slug("v1.2_rc"), "v1.2_rc");
        assert_eq!(branch_slug("/weird//name/"), "weird-name");
        assert_eq!(branch_slug("a@@b"), "a-b");
    }

    #[test]
    fn sibling_path() {
        assert_eq!(
            sibling_worktree(Path::new("/code/app"), "feature/x"),
            PathBuf::from("/code/app-feature-x")
        );
        assert_eq!(
            sibling_worktree(Path::new("app"), "b"),
            PathBuf::from("app-b")
        );
        assert_eq!(
            sibling_worktree(Path::new("/"), "b"),
            PathBuf::from("/worktree-b")
        );
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            WorkError::ProjectMissing("/p".into()).to_string(),
            "project directory not found: /p"
        );
        assert_eq!(
            WorkError::Tool {
                tool: "git".into(),
                branch: "b".into(),
                message: "boom".into()
            }
            .to_string(),
            "git failed for branch 'b': boom"
        );
    }
}
