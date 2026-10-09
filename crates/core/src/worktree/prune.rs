//! What `tasq worktree prune` touches (WT-301): the done tasks whose
//! worktrees it releases, and the repositories where it runs
//! `git worktree prune`. Releasing follows the same rules as closing a task
//! ([`super::lifecycle::on_close`]); this only picks the scope.

use std::path::{Path, PathBuf};

use crate::model::Task;

/// The scope of a prune.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan<'a> {
    /// The done tasks to release, in the order given.
    pub tasks: Vec<&'a Task>,
    /// The repositories (main checkouts) to run `git worktree prune` in:
    /// `here` first, then every repository a task in scope declares
    /// worktrees in, open tasks included.
    pub repos: Vec<PathBuf>,
}

/// Picks the scope among `tasks`. `repo_of` gives the main checkout of a
/// task's project, `None` outside a repository (such a task has no git
/// worktree to release). With `here`, only tasks working in that
/// repository count; without it, every task does.
pub fn plan<'a>(
    tasks: &'a [Task],
    here: Option<&Path>,
    repo_of: impl Fn(&Task) -> Option<PathBuf>,
) -> Plan<'a> {
    let mut plan = Plan {
        tasks: Vec::new(),
        repos: here.map(Path::to_path_buf).into_iter().collect(),
    };
    for task in tasks.iter().filter(|t| !t.worktrees.is_empty()) {
        let Some(repo) = repo_of(task) else {
            continue;
        };
        if here.is_some_and(|here| here != repo) {
            continue;
        }
        if !plan.repos.contains(&repo) {
            plan.repos.push(repo);
        }
        if task.done {
            plan.tasks.push(task);
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TaskId, Worktree};

    fn task(id: u64, done: bool, project: Option<&str>, worktrees: usize) -> Task {
        let mut task = Task::new(TaskId::from(id), "T");
        task.done = done;
        task.project = project.map(PathBuf::from);
        task.worktrees = (0..worktrees)
            .map(|i| Worktree::on_branch(format!("/w/{id}-{i}"), format!("b{i}")))
            .collect();
        task
    }

    fn repo_of(task: &Task) -> Option<PathBuf> {
        task.project.clone()
    }

    fn ids(plan: &Plan) -> Vec<String> {
        plan.tasks.iter().map(|t| t.id.to_string()).collect()
    }

    #[test]
    fn done_tasks_with_worktrees_in_a_repository() {
        let tasks = [
            task(1, true, Some("/a"), 1),
            task(2, false, Some("/b"), 1),
            task(3, true, Some("/a"), 0),
            task(4, true, None, 2),
            task(5, true, Some("/c"), 2),
            task(6, false, Some("/d"), 0),
        ];
        let all = plan(&tasks, None, repo_of);
        assert_eq!(ids(&all), ["1", "5"]);
        assert_eq!(
            all.repos,
            [PathBuf::from("/a"), "/b".into(), "/c".into()],
            "open tasks' repositories too, each once, none without worktrees"
        );
    }

    #[test]
    fn here_limits_to_one_repository_and_comes_first() {
        let tasks = [
            task(1, true, Some("/a"), 1),
            task(2, true, Some("/b"), 1),
            task(3, true, Some("/b"), 1),
        ];
        let b = plan(&tasks, Some(Path::new("/b")), repo_of);
        assert_eq!(ids(&b), ["2", "3"]);
        assert_eq!(b.repos, [PathBuf::from("/b")]);
        let elsewhere = plan(&tasks, Some(Path::new("/z")), repo_of);
        assert_eq!(ids(&elsewhere), Vec::<String>::new());
        assert_eq!(
            elsewhere.repos,
            [PathBuf::from("/z")],
            "the current repository is pruned even with no task in it"
        );
    }
}
