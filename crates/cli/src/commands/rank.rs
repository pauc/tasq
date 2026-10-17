//! `tasq move`: a task's rank within its status group (ADR 0025).
//!
//! The work is [`tasq_core::edit::move_task`], which the TUI will call
//! too; this module maps the options to a [`Target`] and prints the result
//! (or, with `--json`, the task as stored, the fields written and the other
//! tasks ranked for the move).

use tasq_core::edit::{self, Target};
use tasq_core::model::TaskId;

use crate::app::App;
use crate::commands::finish_move;
use crate::error::Result;

/// The [`Target`] of `tasq move`'s options; clap lets exactly one through.
/// An empty anchor id is a user error.
pub fn target(
    top: bool,
    unrank: bool,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<Target> {
    debug_assert!(
        usize::from(top)
            + usize::from(unrank)
            + usize::from(before.is_some())
            + usize::from(after.is_some())
            == 1,
        "clap requires exactly one target"
    );
    if top {
        return Ok(Target::Top);
    }
    if unrank {
        return Ok(Target::Unrank);
    }
    match (before, after) {
        (Some(anchor), _) => Ok(Target::Before(App::task_id(anchor)?)),
        (None, Some(anchor)) => Ok(Target::After(App::task_id(anchor)?)),
        (None, None) => unreachable!("clap requires exactly one target"),
    }
}

/// `tasq move <id> (--before <id> | --after <id> | --top | --unrank)`.
pub fn run(app: &App, id: &str, target: &Target) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let moved = edit::move_task(&mut store, &id, target)?;
    let unchanged = moved.changed.is_empty() && moved.ranked.is_empty();
    let line = format!(
        "[{id}] {}{}\n",
        outcome(target, unchanged),
        also_ranked(&moved.ranked)
    );
    finish_move(app, &store, &id, &line, &moved.changed, &moved.ranked)
}

/// What the move did, for the printed line.
fn outcome(target: &Target, unchanged: bool) -> String {
    match (target, unchanged) {
        (Target::Top, false) => "moved to the top".to_owned(),
        (Target::Top, true) => "already at the top".to_owned(),
        (Target::Unrank, false) => "unranked".to_owned(),
        (Target::Unrank, true) => "already unranked".to_owned(),
        (Target::Before(anchor), false) => format!("moved before task {anchor}"),
        (Target::Before(anchor), true) => format!("already before task {anchor}"),
        (Target::After(anchor), false) => format!("moved after task {anchor}"),
        (Target::After(anchor), true) => format!("already after task {anchor}"),
    }
}

/// ` (also ranked: 1, 8)` when the move ranked other tasks of the group
/// too, so the user knows which files were written; empty otherwise.
fn also_ranked(ranked: &[TaskId]) -> String {
    if ranked.is_empty() {
        return String::new();
    }
    let ids: Vec<String> = ranked.iter().map(ToString::to_string).collect();
    format!(" (also ranked: {})", ids.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_pick_the_target() {
        assert_eq!(target(true, false, None, None).unwrap(), Target::Top);
        assert_eq!(target(false, true, None, None).unwrap(), Target::Unrank);
        assert_eq!(
            target(false, false, Some("3"), None).unwrap(),
            Target::Before(TaskId::from(3))
        );
        assert_eq!(
            target(false, false, None, Some("3")).unwrap(),
            Target::After(TaskId::from(3))
        );
        assert_eq!(
            target(false, false, Some(""), None)
                .unwrap_err()
                .to_string(),
            "task id must not be empty"
        );
        assert_eq!(
            target(false, false, None, Some(""))
                .unwrap_err()
                .to_string(),
            "task id must not be empty"
        );
    }

    #[test]
    fn outcomes() {
        assert_eq!(outcome(&Target::Top, false), "moved to the top");
        assert_eq!(outcome(&Target::Top, true), "already at the top");
        assert_eq!(outcome(&Target::Unrank, false), "unranked");
        assert_eq!(outcome(&Target::Unrank, true), "already unranked");
        let anchor = TaskId::from(3);
        assert_eq!(
            outcome(&Target::Before(anchor.clone()), false),
            "moved before task 3"
        );
        assert_eq!(
            outcome(&Target::Before(anchor.clone()), true),
            "already before task 3"
        );
        assert_eq!(
            outcome(&Target::After(anchor.clone()), false),
            "moved after task 3"
        );
        assert_eq!(
            outcome(&Target::After(anchor), true),
            "already after task 3"
        );
    }

    #[test]
    fn also_ranked_names_the_other_tasks_written() {
        assert_eq!(also_ranked(&[]), "");
        assert_eq!(also_ranked(&[TaskId::from(1)]), " (also ranked: 1)");
        assert_eq!(
            also_ranked(&[TaskId::from(1), TaskId::from(8)]),
            " (also ranked: 1, 8)"
        );
    }
}
