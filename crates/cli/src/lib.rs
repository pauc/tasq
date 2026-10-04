//! The `tasq` command-line interface: clap argument parsing, human and
//! `--json` rendering, pager and error-to-exit-code mapping.
//!
//! The CLI is deliberately thin (ADR-0005): every command is a `tasq-core`
//! function (or a `Store` call) followed by rendering. Nothing in this crate
//! is mutation-tested; behaviour is pinned by the integration tests under
//! `tests/`, which run the binary against a temporary copy of the fixture
//! notebook and snapshot its output with `insta`.
//!
//! Exit codes: `0` success, `1` a user error (`tasq: <message>` on stderr:
//! bad config, unknown task, invalid value), `2` a usage error (clap's own
//! message) or an internal error.

#![warn(missing_docs)]

pub mod app;
pub mod cli;
pub mod commands;
pub mod error;
pub mod json;
pub mod output;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;

/// Parses the process arguments, runs the command and maps the outcome to an
/// exit code. Clap's help and version requests exit `0`; its usage errors
/// print clap's message and exit `2`.
#[must_use]
pub fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            // Printing help to a closed pipe is not worth an error.
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    match app::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            e.report();
            ExitCode::from(e.exit_code())
        }
    }
}
