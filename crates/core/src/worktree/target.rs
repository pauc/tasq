//! What `tasq worktree switch` and `tasq worktree path` take (WT-207): a
//! task (`task:<id>`), the main checkout (`^`), the previous worktree
//! (`-`), a directory, or a branch, and where a branch leads.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::declaration::is_path;
use super::lifecycle::branch_name;
use super::status::Listed;
use crate::model::{TaskId, Worktree};

/// A parsed target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `task:<id>`: the task's worktree (created first by `switch`).
    Task(TaskId),
    /// `^`: the repository's main checkout.
    Main,
    /// `-`: the worktree the last `switch` left.
    Previous,
    /// A directory (see [`is_path`]).
    Path(PathBuf),
    /// Anything else: a branch.
    Branch(String),
}

/// A target that cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TargetError {
    /// `task:` without an id.
    #[error("task: needs an id (task:<id>)")]
    NoTaskId,
    /// Not a branch name either.
    #[error("not a task, a directory or a branch: {0:?}")]
    Invalid(String),
}

/// Parses `arg`; `is_dir` says whether it names an existing directory.
///
/// # Errors
///
/// [`TargetError`] for `task:` alone, or text that cannot be a branch.
pub fn parse(arg: &str, is_dir: bool) -> Result<Target, TargetError> {
    if let Some(id) = arg.strip_prefix("task:") {
        return TaskId::new(id)
            .map(Target::Task)
            .map_err(|_| TargetError::NoTaskId);
    }
    match arg {
        "^" => return Ok(Target::Main),
        "-" => return Ok(Target::Previous),
        _ => {}
    }
    if is_path(arg, is_dir) {
        return Ok(Target::Path(PathBuf::from(arg)));
    }
    branch_name(arg)
        .map(|b| Target::Branch(b.to_owned()))
        .map_err(|_| TargetError::Invalid(arg.to_owned()))
}

/// Where a branch leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// An existing worktree.
    Dir(PathBuf),
    /// A task declares it but it does not exist yet: create that task's.
    Create(TaskId),
    /// Nowhere.
    Nothing,
}

/// The worktree on `branch`: one git lists for the current repository,
/// else one a task declares that exists, else the first task declaring it
/// (to create).
pub fn by_branch(
    branch: &str,
    listed: &[Listed],
    declared: &[(TaskId, Worktree)],
    exists: impl Fn(&Path) -> bool,
) -> Found {
    if let Some(l) = listed.iter().find(|l| l.branch.as_deref() == Some(branch)) {
        return Found::Dir(l.path.clone());
    }
    let mut on_branch = declared
        .iter()
        .filter(|(_, w)| w.branch.as_deref() == Some(branch));
    if let Some((_, w)) = on_branch.clone().find(|(_, w)| exists(&w.path)) {
        return Found::Dir(w.path.clone());
    }
    on_branch
        .next()
        .map_or(Found::Nothing, |(id, _)| Found::Create(id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(parse("task:12", false), Ok(Target::Task(TaskId::from(12))));
        assert_eq!(parse("task:", false), Err(TargetError::NoTaskId));
        assert_eq!(parse("^", false), Ok(Target::Main));
        assert_eq!(parse("-", false), Ok(Target::Previous));
        assert_eq!(
            parse("/w/app-b", false),
            Ok(Target::Path("/w/app-b".into()))
        );
        assert_eq!(parse("app-b", true), Ok(Target::Path("app-b".into())));
        assert_eq!(parse("feat/x", false), Ok(Target::Branch("feat/x".into())));
        assert_eq!(parse("a b", false), Err(TargetError::Invalid("a b".into())));
        assert_eq!(
            TargetError::Invalid("a b".into()).to_string(),
            "not a task, a directory or a branch: \"a b\""
        );
        assert_eq!(
            TargetError::NoTaskId.to_string(),
            "task: needs an id (task:<id>)"
        );
    }

    fn listed(path: &str, branch: Option<&str>) -> Listed {
        Listed {
            path: path.into(),
            branch: branch.map(str::to_owned),
            locked: false,
            bare: false,
        }
    }

    #[test]
    fn a_branch_leads_to_git_then_to_the_tasks() {
        let git = [
            listed("/app", Some("main")),
            listed("/app-a", Some("a")),
            listed("/d", None),
        ];
        let declared = [
            (TaskId::from(1), Worktree::on_branch("/gone-b", "b")),
            (TaskId::from(2), Worktree::on_branch("/here-b", "b")),
            (TaskId::from(3), Worktree::on_branch("/app-a2", "a")),
            (TaskId::from(4), Worktree::on_branch("/gone-c", "c")),
            (TaskId::from(5), Worktree::on_branch("/gone-c2", "c")),
            (TaskId::from(6), Worktree::new("/no-branch")),
        ];
        let exists = |p: &Path| p == Path::new("/here-b") || p == Path::new("/app-a2");
        let found = |b: &str| by_branch(b, &git, &declared, exists);
        assert_eq!(found("a"), Found::Dir("/app-a".into()), "git first");
        assert_eq!(found("b"), Found::Dir("/here-b".into()), "an existing one");
        assert_eq!(found("c"), Found::Create(TaskId::from(4)), "the first task");
        assert_eq!(found("z"), Found::Nothing);
        assert_eq!(
            by_branch("b", &[], &declared, |_| false),
            Found::Create(TaskId::from(1))
        );
    }
}
