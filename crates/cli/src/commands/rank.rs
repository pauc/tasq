//! `tasq move`: a task's rank within its status group (ADR 0025).
//!
//! The work is [`tasq_core::edit::move_task`], which the TUI will call
//! too; this module maps the options to a [`Target`] and prints the result
//! (or, with `--json`, the task as stored, the fields written and the other
//! tasks ranked for the move).

use tasq_core::edit::{self, Target};

use crate::app::App;
use crate::commands::finish_move;
use crate::error::Result;

/// The [`Target`] of `tasq move`'s options; clap lets exactly one through.
pub fn target(top: bool, unrank: bool) -> Target {
    debug_assert!(top != unrank, "clap requires exactly one target");
    if top { Target::Top } else { Target::Unrank }
}

/// `tasq move <id> (--top | --unrank)`.
pub fn run(app: &App, id: &str, target: &Target) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let moved = edit::move_task(&mut store, &id, target)?;
    let line = format!("[{id}] {}\n", outcome(target, moved.changed.is_empty()));
    finish_move(app, &store, &id, &line, &moved.changed, &moved.ranked)
}

/// What the move did, for the printed line.
fn outcome(target: &Target, unchanged: bool) -> &'static str {
    match (target, unchanged) {
        (Target::Top, false) => "moved to the top",
        (Target::Top, true) => "already at the top",
        (Target::Unrank, false) => "unranked",
        (Target::Unrank, true) => "already unranked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_pick_the_target() {
        assert_eq!(target(true, false), Target::Top);
        assert_eq!(target(false, true), Target::Unrank);
    }

    #[test]
    fn outcomes() {
        assert_eq!(outcome(&Target::Top, false), "moved to the top");
        assert_eq!(outcome(&Target::Top, true), "already at the top");
        assert_eq!(outcome(&Target::Unrank, false), "unranked");
        assert_eq!(outcome(&Target::Unrank, true), "already unranked");
    }
}
