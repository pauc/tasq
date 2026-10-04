//! Process-running adapters for `tasq`: the
//! [`Launcher`](tasq_core::launch::Launcher) implementations (shell, Claude
//! Code, tmux, herdr; plan Phase 4), the
//! [`WorktreeManager`](tasq_core::work::WorktreeManager) implementations:
//! plain `git`, or a user-configured command such as gwm (plan T-306), and
//! the command [`Summarizer`](tasq_core::report::Summarizer) behind
//! `tasq summary` (plan T-601).
//!
//! Every process is run with an explicitly injected environment, so tests
//! point `PATH` at fake executables.

#![warn(missing_docs)]

pub mod claude;
pub mod env;
#[cfg(feature = "herdr")]
pub mod herdr;
pub mod process;
pub mod prompt;
pub mod registry;
pub mod shell;
pub mod summarizer;
pub mod tmux;
pub mod worktree;

pub use self::claude::ClaudeLauncher;
pub use self::env::{EnvrcStatus, envrc_status, envrc_warning, parse_direnv_status, wrap_command};
#[cfg(feature = "herdr")]
pub use self::herdr::HerdrLauncher;
pub use self::registry::{LAUNCHER_NAMES, LaunchSettings, launcher_for};
pub use self::shell::ShellLauncher;
pub use self::summarizer::{CommandSummarizer, summarizer_for};
pub use self::tmux::TmuxLauncher;
pub use self::worktree::{
    CommandManager, GitManager, current_branch, expand_template, manager_for,
};
