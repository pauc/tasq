//! Wiring: process state into `LoadOptions`, config into an [`App`], the
//! `App` into an open store, and the parsed command line into a command.

use std::collections::BTreeMap;

use tasq_core::clock::{Clock, FixedClock, SystemClock, When, parse_timestamp};
use tasq_core::config::{Config, LoadOptions, Loaded};
use tasq_core::model::{TaskId, Workflow};
use tasq_store_nb::{NbStore, NbStoreOptions};

use crate::cli::{Cli, Command, GlobalArgs, ListArgs};
use crate::commands;
use crate::error::{CliError, Result};
use crate::output::Output;

/// Environment variable fixing "now" (`YYYY-MM-DD HH:MM`) for every
/// timestamp `tasq` writes; for tests and reproducible demos.
pub const ENV_NOW: &str = "TASQ_NOW";

/// Everything a command needs: flags, loaded configuration and output.
#[derive(Debug)]
pub struct App {
    /// The global flags.
    pub global: GlobalArgs,
    /// Working directory, home and environment the config was loaded with.
    pub opts: LoadOptions,
    /// The effective configuration and its provenance.
    pub loaded: Loaded,
    /// Where output goes.
    pub out: Output,
}

/// Runs the parsed command line.
pub fn run(cli: Cli) -> Result<()> {
    let Cli {
        global,
        word,
        command,
    } = cli;
    if let (Some(word), Some(command)) = (&word, &command) {
        return Err(CliError::user(format!(
            "unexpected argument {word:?} before the '{}' command",
            command_name(command)
        )));
    }
    let opts = load_options(&global)?;
    match command {
        Some(Command::Completions { shell }) => commands::completions::run(shell),
        Some(Command::Doctor) => commands::doctor::run(global, opts),
        other => {
            let app = App::load(global, opts)?;
            match other {
                None => commands::list::run(
                    &app,
                    &ListArgs {
                        word,
                        ..ListArgs::default()
                    },
                ),
                Some(Command::List(args)) => commands::list::run(&app, &args),
                Some(Command::Create(args)) => commands::create::run(&app, &args),
                Some(Command::Set { id, value, note }) => {
                    commands::edit::set(&app, &id, &value, note.as_deref())
                }
                Some(Command::Log { id, note }) => commands::edit::log(&app, &id, &note),
                Some(Command::Done { id, note }) => {
                    commands::edit::done(&app, &id, note.as_deref())
                }
                Some(Command::Next {
                    launcher,
                    detached,
                    no_focus,
                    dry_run,
                }) => {
                    let how = commands::launch::How::new(launcher.as_deref(), detached, no_focus);
                    commands::launch::next(&app, &how, dry_run)
                }
                Some(Command::Pick {
                    id,
                    launcher,
                    detached,
                    no_focus,
                    dry_run,
                }) => {
                    let how = commands::launch::How::new(launcher.as_deref(), detached, no_focus);
                    commands::launch::pick(&app, &id, &how, dry_run)
                }
                Some(Command::View { id, raw }) => commands::view::run(&app, &id, raw),
                Some(Command::Project { id, path }) => {
                    commands::project::run(&app, &id, path.as_deref())
                }
                Some(Command::Worktree { id, path, create }) => {
                    commands::worktree::run(&app, &id, path.as_deref(), create.as_deref())
                }
                Some(Command::Session {
                    id,
                    session_id,
                    description,
                    launcher,
                }) => commands::session::run(
                    &app,
                    &id,
                    &session_id,
                    description.as_deref(),
                    launcher.as_deref(),
                ),
                Some(Command::Mr { id, url, title }) => {
                    commands::mr::run(&app, &id, &url, title.as_deref())
                }
                Some(Command::Apply { file }) => commands::apply::run(&app, file.as_deref()),
                Some(Command::Sync {
                    source,
                    dry_run,
                    interactive,
                    ids,
                }) => commands::sync::run(&app, &source, dry_run, interactive, &ids),
                Some(Command::Summary { day, raw }) => {
                    commands::summary::run(&app, day.as_deref(), raw)
                }
                Some(Command::Dates { spec }) => commands::dates::run(&app, &spec),
                Some(Command::Ui) => commands::ui::run(&app),
                Some(Command::Store(cmd)) => commands::store::run(&app, cmd),
                Some(Command::Config(cmd)) => commands::config::run(&app, cmd),
                Some(Command::Plugins(cmd)) => commands::plugins::run(&app, cmd),
                Some(Command::Doctor | Command::Completions { .. }) => {
                    unreachable!("handled before loading the config")
                }
            }
        }
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::List(_) => "list",
        Command::Create(_) => "create",
        Command::Set { .. } => "set",
        Command::Log { .. } => "log",
        Command::Done { .. } => "done",
        Command::Next { .. } => "next",
        Command::Pick { .. } => "pick",
        Command::View { .. } => "view",
        Command::Project { .. } => "project",
        Command::Worktree { .. } => "worktree",
        Command::Session { .. } => "session",
        Command::Mr { .. } => "mr",
        Command::Apply { .. } => "apply",
        Command::Sync { .. } => "sync",
        Command::Summary { .. } => "summary",
        Command::Dates { .. } => "dates",
        Command::Ui => "ui",
        Command::Store(_) => "store",
        Command::Doctor => "doctor",
        Command::Config(_) => "config",
        Command::Plugins(_) => "plugins",
        Command::Completions { .. } => "completions",
    }
}

/// [`LoadOptions`] from the process (cwd, `$HOME`, environment) plus the
/// `--config`, `--profile` and `--set` flags.
pub fn load_options(global: &GlobalArgs) -> Result<LoadOptions> {
    let mut opts = LoadOptions::from_process()?;
    opts.explicit_file.clone_from(&global.config);
    opts.profile.clone_from(&global.profile);
    opts.overrides.clone_from(&global.set);
    Ok(opts)
}

impl App {
    /// Loads the configuration for `opts` and decides output settings.
    pub fn load(global: GlobalArgs, opts: LoadOptions) -> Result<Self> {
        let loaded = Config::load(&opts)?;
        let out = Output::from_process(&global, &loaded.config.ui, &opts.env);
        Ok(Self::new(global, opts, loaded, out))
    }

    /// An app from already loaded parts.
    pub fn new(global: GlobalArgs, opts: LoadOptions, loaded: Loaded, out: Output) -> Self {
        Self {
            global,
            opts,
            loaded,
            out,
        }
    }

    /// The effective configuration.
    pub fn config(&self) -> &Config {
        &self.loaded.config
    }

    /// The configured status workflow.
    pub fn workflow(&self) -> Workflow {
        self.config().workflow.workflow()
    }

    /// The environment as the store wants it.
    pub fn env_vec(&self) -> Vec<(String, String)> {
        env_vec(&self.opts.env)
    }

    /// Store options from the config and the process environment: `nb` is
    /// looked up on this environment's `PATH`, `NB_DIR` in it, and the
    /// file that set `store.notebook` is recorded for error messages.
    pub fn store_options(&self) -> NbStoreOptions {
        let mut options = NbStoreOptions::new(self.workflow())
            .with_env(self.env_vec())
            .with_bookkeeper(self.config().store.bookkeeper);
        if let Some(home) = &self.opts.home {
            options = options.with_home(home);
        }
        if let Some(file) = self.loaded.file_for("store.notebook") {
            options = options.with_config_file(file);
        }
        options
    }

    /// The clock that stamps notes, sessions and new files: the system
    /// clock, or a fixed one when [`ENV_NOW`] is set (a testing aid).
    pub fn clock(&self) -> Result<Box<dyn Clock>> {
        match self.opts.env.get(ENV_NOW).filter(|v| !v.is_empty()) {
            None => Ok(Box::new(SystemClock)),
            Some(text) => match parse_timestamp(text) {
                Ok(When::DateTime(at)) => Ok(Box::new(FixedClock(at))),
                Ok(When::Date(_)) | Err(_) => Err(CliError::user(format!(
                    "{ENV_NOW}={text:?}: expected YYYY-MM-DD HH:MM"
                ))),
            },
        }
    }

    /// A task id from the command line (anything non-empty).
    pub fn task_id(text: &str) -> Result<TaskId> {
        TaskId::new(text).map_err(|_| CliError::user("task id must not be empty"))
    }

    /// Opens the configured store. Warnings raised while opening (a
    /// rebuilt index) go to stderr.
    pub fn open_store(&self) -> Result<NbStore> {
        let mut store =
            NbStore::open(&self.config().store, &self.store_options())?.with_clock(self.clock()?);
        for warning in store.take_warnings() {
            self.out.warn(&warning.to_string());
        }
        self.out
            .verbose(&format!("notebook: {}", store.dir().display()));
        Ok(store)
    }
}

/// A `BTreeMap` environment as the `Vec` the store API takes.
pub fn env_vec(env: &BTreeMap<String, String>) -> Vec<(String, String)> {
    env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}
