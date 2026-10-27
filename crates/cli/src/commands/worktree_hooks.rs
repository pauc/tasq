//! Worktree hooks in the CLI (plan 4.5, WT-204): printing what the hooks
//! [`tasq_worktree::hooks`] runs report.

use crate::app::App;
use crate::plugins::HookEvent;

/// Prints a hook event the way the CLI does: warnings always, a hook's
/// output at `-v`.
pub fn to_terminal(app: &App) -> impl FnMut(HookEvent) + '_ {
    |event| match event {
        HookEvent::Warning(text) => app.out.warn(&text),
        HookEvent::Output(text) => app.out.verbose(&text),
    }
}
