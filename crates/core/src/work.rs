//! Work context: where a task is worked on.
//!
//! A task tracks a `## Project` directory and `## Worktrees`. Sessions start
//! in the first declared worktree that exists, else the project, else the
//! configured default (plan FR-8; the resolver itself is T-401). Declared
//! worktrees are created by `tasq-worktree` ([`crate::worktree`]).

use std::path::{Path, PathBuf};

use crate::model::Task;

/// The directory a task's work happens in when no worktree is involved:
/// its `## Project`, else `default` (`work.default_project`).
pub fn project_dir(task: &Task, default: Option<&Path>) -> Option<PathBuf> {
    task.project
        .clone()
        .or_else(|| default.map(Path::to_path_buf))
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
}
