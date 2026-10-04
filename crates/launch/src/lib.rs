//! Process-running adapters for `tasq`: the `Launcher` implementations
//! (shell, Claude Code, tmux, herdr; plan Phase 4) and the
//! [`WorktreeManager`](tasq_core::work::WorktreeManager) implementations:
//! plain `git`, or a user-configured command such as gwm (plan T-306).
//!
//! Every process is run with an explicitly injected environment, so tests
//! point `PATH` at fake executables.

#![warn(missing_docs)]

pub mod process;
pub mod worktree;

pub use self::worktree::{
    CommandManager, GitManager, current_branch, expand_template, manager_for,
};
