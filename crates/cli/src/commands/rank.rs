//! `tasq move`: a task's rank within its status group (ADR 0025).
//!
//! The work is [`tasq_core::edit::move_task`], which the TUI calls too;
//! this module maps the options to a [`Target`] and prints the result
//! ([`tasq_core::edit::Moved::describe`], or, with `--json`, the task as
//! stored, the fields written and the other tasks ranked for the move).

use tasq_core::edit::{self, Target};

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
    match (top, unrank, before, after) {
        (true, false, None, None) => Ok(Target::Top),
        (false, true, None, None) => Ok(Target::Unrank),
        (false, false, Some(anchor), None) => Ok(Target::Before(App::task_id(anchor)?)),
        (false, false, None, Some(anchor)) => Ok(Target::After(App::task_id(anchor)?)),
        _ => unreachable!("clap requires exactly one target"),
    }
}

/// `tasq move <id> (--before <id> | --after <id> | --top | --unrank)`.
pub fn run(app: &App, id: &str, target: &Target) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let moved = edit::move_task(&mut store, &id, target)?;
    let line = format!("{}\n", moved.describe(target));
    finish_move(app, &store, &id, &line, &moved.changed, &moved.ranked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::model::TaskId;

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
}
