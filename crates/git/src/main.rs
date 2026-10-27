//! `tasq-git` binary: the plugin tasq runs as `tasq-git hook` for every
//! event, and `tasq git ...` runs for its commands.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use clap::{Args, Parser, Subcommand};
use tasq_git::git::Git;
use tasq_git::init::{self, Identity, InitError, Options};
use tasq_git::job::{self, Settings};
use tasq_git::process::{background_env, spawn_detached, without_git_vars};
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
    /// Commit and sync the store's writes (detached by `hook`).
    #[command(hide = true)]
    Job {
        /// The store directory.
        #[arg(long)]
        store: PathBuf,
        /// Seconds after the last fetch that the next one is due.
        #[arg(long, default_value_t = tasq_git::hook::INTERVAL.as_secs())]
        interval: u64,
    },
    /// Rebase the store onto its upstream (run by the job under the store
    /// lock).
    #[command(hide = true)]
    Rebase {
        /// The store directory.
        #[arg(long)]
        store: PathBuf,
    },
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
        Command::Job { store, interval } => report(run_job(&store, interval)),
        Command::Rebase { store } => rebase(&store),
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

/// `tasq` as the command that ran the plugin resolves it: `TASQ_BIN`, this
/// environment and working directory.
fn tasq_here() -> Result<Tasq, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("could not read the working directory: {err}"))?;
    Ok(Tasq {
        bin: tasq::bin(std::env::var_os("TASQ_BIN").map(PathBuf::from).as_deref()),
        env: std::env::vars_os().collect(),
        cwd,
    })
}

fn run_job(store: &Path, interval: u64) -> Result<String, String> {
    let tasq = tasq_here()?;
    let git = Git {
        env: background_env(std::env::vars_os()),
    };
    let settings = Settings {
        interval: Duration::from_secs(interval),
        exe: std::env::current_exe()
            .map_err(|err| format!("could not find tasq-git itself: {err}"))?,
    };
    job::run(&git, &tasq, store, &settings)
        .map(|()| String::new())
        .map_err(|err| err.to_string())
}

/// The rebase under the store lock; git's message alone on failure, which
/// the job turns into the lasting warning.
fn rebase(store: &Path) -> ExitCode {
    let git = Git {
        env: background_env(std::env::vars_os()),
    };
    match job::rebase(&git, store) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_init(args: InitArgs) -> Result<String, String> {
    let tasq = tasq_here()?;
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
        .and_then(|_| {
            tasq_git::hook::run(&stdin, SystemTime::now(), spawn_job).map_err(|err| err.to_string())
        });
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

/// Detaches `tasq-git job --store <store> --interval <secs>`, this very
/// binary.
fn spawn_job(store: &Path, interval: Duration) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let interval = interval.as_secs().to_string();
    spawn_detached(
        &exe,
        &[
            OsStr::new("job"),
            OsStr::new("--store"),
            store.as_os_str(),
            OsStr::new("--interval"),
            OsStr::new(&interval),
        ],
    )
}
