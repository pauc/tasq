//! `tasq-git` binary: the plugin tasq runs as `tasq-git hook` for every
//! event, and `tasq git ...` runs for its commands.

use std::io::{Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Store sync for tasq: commits, pushes and pulls a store's task files.
#[derive(Parser)]
#[command(name = "tasq-git", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Handle one tasq event: the payload on stdin (run by tasq).
    Hook,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Hook => hook(),
    }
}

/// Reads the payload, prints the answer; an error is one line on stderr
/// and exit 1, which tasq shows as a warning naming the plugin and event.
fn hook() -> ExitCode {
    let mut stdin = String::new();
    let result = std::io::stdin()
        .read_to_string(&mut stdin)
        .map_err(|err| format!("could not read the event payload: {err}"))
        .and_then(|_| tasq_git::hook::run(&stdin).map_err(|err| err.to_string()));
    match result {
        Ok(out) => {
            // tasq reads stdout until it closes; nothing to do if it went away.
            let _ = std::io::stdout().write_all(out.as_bytes());
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            ExitCode::FAILURE
        }
    }
}
