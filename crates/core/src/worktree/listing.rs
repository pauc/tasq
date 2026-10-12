//! What `tasq worktree ls` lists and `switch` offers (plan 4.6, WT-207):
//! the declared worktrees in scope, and how each one stands. Looking at
//! the directories and asking git is the caller's; this picks and labels.

use std::path::{Path, PathBuf};

use crate::model::{Task, TaskId, Worktree};

/// The tasks whose worktrees are listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One task asked for by id: all its worktrees, wherever they are.
    Task,
    /// Every task working in the repository with this main checkout.
    Repo(PathBuf),
    /// Every task.
    Everywhere,
}

impl Scope {
    /// The scope of `ls [ID] [--all]`: one task when `one`; else every
    /// task with `all`; else the tasks working in the current directory's
    /// repository (`here`, asked only then), or every task outside one.
    pub fn of(one: bool, all: bool, here: impl FnOnce() -> Option<PathBuf>) -> Self {
        if one {
            Self::Task
        } else if all {
            Self::Everywhere
        } else {
            here().map_or(Self::Everywhere, Self::Repo)
        }
    }
}

/// A declared worktree in scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed<'a> {
    /// The task declaring it.
    pub task: &'a Task,
    /// The declaration.
    pub worktree: &'a Worktree,
    /// Whether its directory exists.
    pub exists: bool,
}

/// The declared worktrees of `tasks` in `scope`, in order. `repo_of` gives
/// the main checkout of a task's project (`None` outside a repository),
/// `exists` whether a directory exists. Outside [`Scope::Task`], the
/// worktrees of closed tasks that do not exist are left out.
pub fn listed<'a>(
    tasks: &'a [Task],
    scope: &Scope,
    repo_of: impl Fn(&Task) -> Option<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Listed<'a>> {
    let mut listed = Vec::new();
    for task in tasks {
        if let Scope::Repo(here) = scope
            && !task.worktrees.is_empty()
            && repo_of(task).as_ref() != Some(here)
        {
            continue;
        }
        for worktree in &task.worktrees {
            let exists = exists(&worktree.path);
            if *scope != Scope::Task && task.done && !exists {
                continue;
            }
            listed.push(Listed {
                task,
                worktree,
                exists,
            });
        }
    }
    listed
}

/// One declared worktree, as `ls` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The task declaring it.
    pub id: TaskId,
    /// The task's title.
    pub title: String,
    /// The task's status, `done` for a closed task.
    pub status: Option<String>,
    /// Whether the task is closed.
    pub done: bool,
    /// The declaration.
    pub worktree: Worktree,
    /// Whether its directory exists.
    pub exists: bool,
    /// Its changed paths (provisioned files left out); `None` when it does
    /// not exist or git cannot say.
    pub changes: Option<Vec<String>>,
}

impl Row {
    /// The row of `listed`, with the `changes` git reports.
    pub fn new(listed: &Listed, changes: Option<Vec<String>>) -> Self {
        let task = listed.task;
        let status = if task.done {
            Some("done".to_owned())
        } else {
            task.status.as_ref().map(|s| s.as_str().to_owned())
        };
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            status,
            done: task.done,
            worktree: listed.worktree.clone(),
            exists: listed.exists,
            changes,
        }
    }

    /// `clean`, `<n> changed`, `not created` or `unknown`.
    pub fn state(&self) -> String {
        match (&self.changes, self.exists) {
            (_, false) => "not created".to_owned(),
            (None, true) => "unknown".to_owned(),
            (Some(c), true) if c.is_empty() => "clean".to_owned(),
            (Some(c), true) => format!("{} changed", c.len()),
        }
    }
}

/// The rows `switch` offers as destinations: a task with a worktree that
/// exists creates no other, so its declared but missing ones are left out.
pub fn destinations(rows: Vec<Row>) -> Vec<Row> {
    let has_one: Vec<TaskId> = rows
        .iter()
        .filter(|r| r.exists)
        .map(|r| r.id.clone())
        .collect();
    rows.into_iter()
        .filter(|r| r.exists || !has_one.contains(&r.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Status;

    fn task(id: u64, done: bool, project: Option<&str>, worktrees: &[&str]) -> Task {
        let mut task = Task::new(TaskId::from(id), format!("T{id}"));
        task.done = done;
        task.project = project.map(PathBuf::from);
        task.worktrees = worktrees
            .iter()
            .map(|w| Worktree::on_branch(*w, "b"))
            .collect();
        task
    }

    fn repo_of(task: &Task) -> Option<PathBuf> {
        task.project.clone()
    }

    /// Paths under `/x` exist.
    fn exists(path: &Path) -> bool {
        path.starts_with("/x")
    }

    fn paths(listed: &[Listed]) -> Vec<String> {
        listed
            .iter()
            .map(|l| format!("{} {}", l.worktree.path.display(), l.exists))
            .collect()
    }

    #[test]
    fn the_scope_follows_the_arguments() {
        let here = || Some(PathBuf::from("/a"));
        let nowhere = || None;
        assert_eq!(Scope::of(true, false, here), Scope::Task);
        assert_eq!(Scope::of(true, true, here), Scope::Task);
        assert_eq!(Scope::of(false, true, here), Scope::Everywhere);
        assert_eq!(Scope::of(false, false, here), Scope::Repo("/a".into()));
        assert_eq!(Scope::of(false, false, nowhere), Scope::Everywhere);
        let asked = std::cell::Cell::new(false);
        let _ = Scope::of(false, true, || {
            asked.set(true);
            None
        });
        assert!(!asked.get(), "here is only asked without an id or --all");
    }

    #[test]
    fn one_repository_or_every_one() {
        let tasks = [
            task(1, false, Some("/a"), &["/x/1"]),
            task(2, false, Some("/b"), &["/x/2"]),
            task(3, false, None, &["/x/3"]),
            task(4, false, Some("/b"), &[]),
        ];
        let a = listed(&tasks, &Scope::Repo("/a".into()), repo_of, exists);
        assert_eq!(paths(&a), ["/x/1 true"]);
        let all = listed(&tasks, &Scope::Everywhere, repo_of, exists);
        assert_eq!(paths(&all), ["/x/1 true", "/x/2 true", "/x/3 true"]);
        assert_eq!(all[1].task.id, TaskId::from(2));
    }

    #[test]
    fn closed_tasks_show_only_existing_worktrees_unless_asked_for() {
        let tasks = [
            task(1, true, Some("/a"), &["/x/1", "/gone/1"]),
            task(2, false, Some("/a"), &["/gone/2"]),
        ];
        assert_eq!(
            paths(&listed(&tasks, &Scope::Everywhere, repo_of, exists)),
            ["/x/1 true", "/gone/2 false"]
        );
        assert_eq!(
            paths(&listed(&tasks, &Scope::Repo("/a".into()), repo_of, exists)),
            ["/x/1 true", "/gone/2 false"]
        );
        assert_eq!(
            paths(&listed(&tasks[..1], &Scope::Task, |_| None, exists)),
            ["/x/1 true", "/gone/1 false"],
            "a task asked for by id shows every worktree, in any repository"
        );
    }

    fn row(id: u64, exists: bool, changes: Option<usize>) -> Row {
        let task = task(id, false, None, &["/w"]);
        let listed = Listed {
            task: &task,
            worktree: &task.worktrees[0],
            exists,
        };
        Row::new(
            &listed,
            changes.map(|n| (0..n).map(|i| i.to_string()).collect()),
        )
    }

    #[test]
    fn a_row_carries_the_task_and_its_state() {
        let mut task = task(3, false, None, &["/w/3"]);
        task.status = Some(Status::READY);
        let listed = Listed {
            task: &task,
            worktree: &task.worktrees[0],
            exists: true,
        };
        let r = Row::new(&listed, Some(vec!["f".into()]));
        assert_eq!(
            r,
            Row {
                id: TaskId::from(3),
                title: "T3".into(),
                status: Some("ready".into()),
                done: false,
                worktree: Worktree::on_branch("/w/3", "b"),
                exists: true,
                changes: Some(vec!["f".into()]),
            }
        );
        task.done = true;
        let listed = Listed {
            task: &task,
            worktree: &task.worktrees[0],
            exists: false,
        };
        let done = Row::new(&listed, None);
        assert_eq!((done.status.as_deref(), done.done), (Some("done"), true));
        assert_eq!(row(1, false, None).status, None);
        assert_eq!(row(1, true, Some(0)).state(), "clean");
        assert_eq!(row(1, true, Some(2)).state(), "2 changed");
        assert_eq!(row(1, false, Some(2)).state(), "not created");
        assert_eq!(row(1, true, None).state(), "unknown");
    }

    #[test]
    fn destinations_leave_out_missing_worktrees_of_tasks_with_one() {
        let rows = vec![
            row(1, false, None),
            row(1, true, Some(0)),
            row(2, false, None),
            row(3, true, None),
        ];
        let kept: Vec<(String, bool)> = destinations(rows)
            .iter()
            .map(|r| (r.id.to_string(), r.exists))
            .collect();
        assert_eq!(
            kept,
            [
                ("1".to_owned(), true),
                ("2".to_owned(), false),
                ("3".to_owned(), true)
            ]
        );
    }
}
