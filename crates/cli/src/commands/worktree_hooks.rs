//! Worktree hooks in the CLI (plan 4.5, WT-204): the terminal side of
//! [`tasq_worktree::hooks`], which decides which lines may run, keeps the
//! trust store and runs them. Here: where the trust file is, asking on the
//! terminal, and printing what the hooks report.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

pub use tasq_core::worktree::hooks::Hooks;
use tasq_core::worktree::hooks::{self, Kind};
use tasq_core::worktree::vars::Vars;
use tasq_worktree::RepoSettings;
pub use tasq_worktree::hooks::Approval;
use tasq_worktree::hooks::{Approver, run_all};

use crate::app::App;
use crate::error::Result;
use crate::plugins::HookEvent;

/// Prints a hook event the way the CLI does: warnings always, a hook's
/// output at `-v`.
pub fn to_terminal(app: &App) -> impl FnMut(HookEvent) + '_ {
    |event| match event {
        HookEvent::Warning(text) => app.out.warn(&text),
        HookEvent::Output(text) => app.out.verbose(&text),
    }
}

/// How hook lines get approval in this environment: `approval`, the trust
/// file of this environment, and questions asked on the terminal.
pub fn approver(app: &App, approval: Approval) -> Approver {
    Approver {
        approval,
        trust_file: trust_file(app),
        ask,
    }
}

/// [`tasq_worktree::hooks::prepare`] with this environment's [`approver`].
pub fn prepare<const N: usize>(
    app: &App,
    settings: &RepoSettings,
    kinds: [Kind; N],
    vars: &Vars,
    approval: Approval,
    dry_run: bool,
    report: &mut dyn FnMut(HookEvent),
) -> Result<[Hooks; N]> {
    let approver = approver(app, approval);
    Ok(tasq_worktree::hooks::prepare(
        settings, kinds, vars, &approver, dry_run, report,
    )?)
}

/// Runs `hooks` in `cwd` with the user's environment plus the hook
/// variables of `vars` ([`run_all`]).
pub fn run(
    app: &App,
    hooks: &Hooks,
    cwd: &Path,
    vars: &Vars,
    report: &mut dyn FnMut(HookEvent),
) -> Result<()> {
    let mut env = app.env_vec();
    env.extend(hooks::env(vars));
    Ok(run_all(hooks, cwd, &env, report)?)
}

/// Stores every hook line the repository's file sets as approved; the
/// lines, by kind ([`tasq_worktree::hooks::trust`]).
pub fn trust(app: &App, settings: &RepoSettings) -> Result<Vec<(Kind, String)>> {
    Ok(tasq_worktree::hooks::trust(
        settings,
        trust_file(app).as_deref(),
    )?)
}

/// The trust file for this environment.
fn trust_file(app: &App) -> Option<PathBuf> {
    hooks::trust_file(
        app.opts.env.get("XDG_STATE_HOME").map(String::as_str),
        app.opts.home.as_deref(),
    )
}

/// Asks `question` on stderr and reads a line; `false` without a terminal.
// Reason: reads the terminal; `is_yes` reads the reply.
#[mutants::skip]
fn ask(question: &str) -> std::io::Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    eprint!("{question}");
    std::io::stderr().flush()?;
    let mut reply = String::new();
    std::io::stdin().lock().read_line(&mut reply)?;
    Ok(is_yes(&reply))
}

/// Whether a reply to a `[y/N]` question is a yes: its first non-blank
/// character is `y` or `Y`.
fn is_yes(reply: &str) -> bool {
    reply.trim_start().starts_with(['y', 'Y'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yes_starts_with_y() {
        for reply in ["y\n", "Y\n", "  yes\n", "Yep"] {
            assert!(is_yes(reply), "{reply:?}");
        }
        for reply in ["", "\n", "n\n", "N", "no", " ny", "x y"] {
            assert!(!is_yes(reply), "{reply:?}");
        }
    }
}
