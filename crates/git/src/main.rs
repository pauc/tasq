//! `tasq-git` binary: the plugin tasq runs as `tasq-git hook` for every
//! event, and `tasq git ...` runs for its commands.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use tasq_git::git::Git;
use tasq_git::init::{self, Identity, InitError, Options};
use tasq_git::process::without_git_vars;
use tasq_git::tasq::{self, Tasq};

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
    /// Make the store a git repository, and push it to <URL> if given.
    Init(InitArgs),
}

#[derive(Args)]
struct InitArgs {
    /// The remote to push the store to.
    url: Option<String>,
    /// The branch, here and on the remote [default: the store name].
    #[arg(long)]
    branch: Option<String>,
    /// The name the store commits as [default: [plugin.git] name].
    #[arg(long)]
    name: Option<String>,
    /// The email the store commits as [default: [plugin.git] email].
    #[arg(long)]
    email: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "The SSH key the store pushes with [default: [plugin.git] ssh_key]"
    )]
    ssh_key: Option<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Hook => hook(),
        Command::Init(args) => report(run_init(args)),
    }
}

/// Prints what a command did, or its error as `tasq-git: <message>` and
/// exit 1.
fn report(result: Result<String, String>) -> ExitCode {
    match result {
        Ok(out) => {
            let _ = std::io::stdout().write_all(out.as_bytes());
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "tasq-git: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_init(args: InitArgs) -> Result<String, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("could not read the working directory: {err}"))?;
    let tasq = Tasq {
        bin: tasq::bin(std::env::var_os("TASQ_BIN").map(PathBuf::from).as_deref()),
        env: std::env::vars_os().collect(),
        cwd,
    };
    let git = Git {
        env: without_git_vars(std::env::vars_os()),
    };
    let result = (|| -> Result<_, InitError> {
        let store = tasq.store()?;
        let identity = Identity {
            name: args.name,
            email: args.email,
            ssh_key: args.ssh_key,
        }
        .or_defaults(&tasq.plugin_config()?)?;
        let options = Options {
            url: args.url,
            branch: args.branch,
            identity,
        };
        init::init(&git, &store, &options)
    })();
    result.map(|r| r.to_string()).map_err(|err| err.to_string())
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
