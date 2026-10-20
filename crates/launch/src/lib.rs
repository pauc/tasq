//! Process-running adapters for `tasq`: the
//! [`Launcher`](tasq_core::launch::Launcher) implementations (shell, Claude
//! Code, tmux, herdr; plan Phase 4) and the command [`Summarizer`](tasq_core::report::Summarizer) behind
//! `tasq summary` (plan T-601).
//!
//! Every process is run with an explicitly injected environment, so tests
//! point `PATH` at fake executables. [`which`] finds the optional tools on
//! such a `PATH` (`tasq doctor`'s tool checks, `glow` for `tasq view`).

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
pub mod which;

pub use self::claude::{ClaudeLauncher, command_in};
pub use self::env::{EnvrcStatus, envrc_status, envrc_warning, parse_direnv_status, wrap_command};
#[cfg(feature = "herdr")]
pub use self::herdr::HerdrLauncher;
pub use self::registry::{
    LAUNCHER_NAMES, LaunchSettings, WINDOW_LAUNCHER_NAMES, launcher_for, resolve_window,
};
pub use self::shell::ShellLauncher;
pub use self::summarizer::{CommandSummarizer, summarizer_for};
pub use self::tmux::TmuxLauncher;
pub use self::which::{find_in_path, tool_check};
