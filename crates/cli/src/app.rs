//! Wiring: process state into `LoadOptions`, config into an [`App`], the
//! `App` into an open store, and the parsed command line into a command.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tasq_core::clock::{Clock, FixedClock, SystemClock, When, parse_timestamp};
use tasq_core::config::{Config, LoadOptions, Loaded};
use tasq_core::launch::{ENV_DATA_DIR, ENV_STORE_NAME};
use tasq_core::model::{TaskId, Workflow};
use tasq_core::store::Store;
use tasq_store::TasqStore;
use tasq_worktree::Git;

use crate::cli::{Cli, Command, EditCommand, GlobalArgs, ListArgs};
use crate::commands;
use crate::error::{CliError, Result};
use crate::output::Output;

/// Environment variable fixing "now" (`YYYY-MM-DD HH:MM`) for every
/// timestamp `tasq` writes; for tests and reproducible demos.
pub const ENV_NOW: &str = "TASQ_NOW";

/// The two settings that make up the store directory, and where each was
/// set ([`App::store_provenance`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The resolved, absolute `data_dir`.
    pub data_dir: PathBuf,
    /// The layer that set `data_dir`.
    pub data_dir_from: String,
    /// `store.name`.
    pub name: String,
    /// The layer that set `store.name`.
    pub name_from: String,
}

impl Provenance {
    /// The store directory, `<data_dir>/<store.name>`.
    pub fn dir(&self) -> PathBuf {
        self.data_dir.join(&self.name)
    }
}

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
        Some(Command::Completions { shell }) => commands::completions::run(shell, &global),
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
                Some(Command::Edit(command)) => edit(&app, command),
                Some(command @ (Command::Next { .. } | Command::Pick { .. })) => {
                    commands::launch::dispatch(&app, &command)
                }
                Some(Command::View { id, raw }) => commands::view::run(&app, &id, raw),
                Some(Command::Project { id, path }) => {
                    commands::project::run(&app, &id, path.as_deref())
                }
                Some(command @ Command::Worktree { .. }) => {
                    commands::worktree::dispatch(&app, &command)
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

/// The commands that change one task in place: `set`, `log`, `done`,
/// `reopen` and the one-field edits.
fn edit(app: &App, command: EditCommand) -> Result<()> {
    match command {
        EditCommand::Set { id, value, note } => {
            commands::edit::set(app, &id, &value, note.into_note().as_deref())
        }
        EditCommand::Log { id, note } => commands::edit::log(app, &id, &note),
        EditCommand::Done { id, note, yes } => {
            commands::edit::done(app, &id, note.into_note().as_deref(), yes)
        }
        EditCommand::Reopen { id, status, note } => {
            commands::edit::reopen(app, &id, status.as_deref(), note.into_note().as_deref())
        }
        EditCommand::Rename { id, title, note } => {
            commands::fields::rename(app, &id, &title, note.as_deref())
        }
        EditCommand::Due { id, date, note } => {
            commands::fields::due(app, &id, &date, note.as_deref())
        }
        EditCommand::Tag { id, tags, note } => {
            commands::fields::tag(app, &id, &tags, note.as_deref())
        }
        EditCommand::Desc {
            id,
            text,
            edit: true,
            note,
        } => {
            debug_assert!(text.is_none(), "clap keeps TEXT and --edit apart");
            commands::fields::desc_edit(app, &id, note.as_deref())
        }
        EditCommand::Desc { id, text, note, .. } => {
            commands::fields::desc(app, &id, text.as_deref(), note.as_deref())
        }
        EditCommand::Move {
            id,
            top,
            unrank,
            before,
            after,
        } => {
            let target = commands::rank::target(top, unrank, before.as_deref(), after.as_deref())?;
            commands::rank::run(app, &id, &target)
        }
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::List(_) => "list",
        Command::Create(_) => "create",
        Command::Edit(edit) => match edit {
            EditCommand::Set { .. } => "set",
            EditCommand::Log { .. } => "log",
            EditCommand::Done { .. } => "done",
            EditCommand::Reopen { .. } => "reopen",
            EditCommand::Rename { .. } => "rename",
            EditCommand::Due { .. } => "due",
            EditCommand::Tag { .. } => "tag",
            EditCommand::Desc { .. } => "desc",
            EditCommand::Move { .. } => "move",
        },
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

/// Whether worktrees are enabled for these flags, for decisions taken
/// before a command runs (`--help`, completions). A configuration that does
/// not load counts as disabled here; the command then reports the error.
pub fn worktrees_enabled(global: &GlobalArgs) -> bool {
    load_options(global)
        .ok()
        .and_then(|opts| Config::load(&opts).ok())
        .is_some_and(|loaded| loaded.config.worktree.enabled)
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

    /// The configuration as loaded from `dir` instead of the current
    /// directory, so the `.tasq.toml` next to a repository applies wherever
    /// tasq runs (the worktree commands, plan 4.4). Everything else (the
    /// global file, `--config`, `--profile`, `--set`, the environment) is as
    /// for [`App::config`].
    pub fn config_at(&self, dir: &std::path::Path) -> Result<Loaded> {
        let mut opts = self.opts.clone();
        opts.cwd = dir.to_path_buf();
        Ok(Config::load(&opts)?)
    }

    /// The configured status workflow.
    pub fn workflow(&self) -> Workflow {
        self.config().workflow.workflow()
    }

    /// Whether worktrees are enabled (`worktree.enabled`, ADR 0022).
    pub fn worktrees(&self) -> bool {
        self.config().worktree.enabled
    }

    /// A task as JSON, without `worktrees` when they are disabled.
    pub fn task_json(&self, task: &tasq_core::model::Task) -> serde_json::Value {
        crate::json::task(task, self.worktrees())
    }

    /// The environment as the store wants it.
    pub fn env_vec(&self) -> Vec<(String, String)> {
        env_vec(&self.opts.env)
    }

    /// `git`, run with this environment.
    pub fn git(&self) -> Git {
        Git {
            env: self.env_vec(),
        }
    }

    /// The resolved `data_dir`, absolute: a relative one (from `--set` or
    /// `TASQ_DATA_DIR`) is taken from the working directory. An error when
    /// the config could not derive a default (no `$XDG_DATA_HOME`, no home).
    pub fn data_dir(&self) -> Result<PathBuf> {
        let Some(dir) = &self.config().data_dir else {
            return Err(CliError::user(
                "data_dir is unknown: neither XDG_DATA_HOME nor HOME is set; set data_dir in the config or TASQ_DATA_DIR",
            ));
        };
        Ok(if dir.is_relative() {
            self.opts.cwd.join(dir)
        } else {
            dir.clone()
        })
    }

    /// The store directory, `<data_dir>/<store.name>` (ADR-0026).
    pub fn store_dir(&self) -> Result<PathBuf> {
        Ok(self.data_dir()?.join(&self.config().store.name))
    }

    /// Where the store directory came from: the resolved `data_dir` and
    /// `store.name`, each with the layer that set it (`defaults`, a file,
    /// `env`, `--set`), as `tasq config show` names them.
    pub fn store_provenance(&self) -> Result<Provenance> {
        let from = |key: &str| {
            self.loaded
                .explain(key)
                .map_or_else(|| "unset".to_owned(), ToString::to_string)
        };
        Ok(Provenance {
            data_dir: self.data_dir()?,
            data_dir_from: from("data_dir"),
            name: self.config().store.name.clone(),
            name_from: from("store.name"),
        })
    }

    /// The variables that point a launched session's `tasq` at this store:
    /// [`ENV_DATA_DIR`] (resolved, absolute) and [`ENV_STORE_NAME`].
    pub fn store_env(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![
            (
                ENV_DATA_DIR.to_owned(),
                self.data_dir()?.display().to_string(),
            ),
            (ENV_STORE_NAME.to_owned(), self.config().store.name.clone()),
        ])
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

    /// Opens the configured store as a [`Store`] trait object: this is the
    /// one place that knows which implementation backs the commands (the
    /// tasq store at [`App::store_dir`], ADR-0026). Warnings raised while
    /// opening go to stderr.
    pub fn open_store(&self) -> Result<Box<dyn Store>> {
        let mut store: Box<dyn Store> =
            Box::new(TasqStore::new(self.store_dir()?, self.workflow()).with_clock(self.clock()?));
        for warning in store.take_warnings() {
            self.out.warn(&warning);
        }
        let info = store.describe();
        self.out.verbose(&format!(
            "store: {} at {}",
            info.name,
            info.location.display()
        ));
        Ok(store)
    }
}

/// A `BTreeMap` environment as the `Vec` the store API takes.
pub fn env_vec(env: &BTreeMap<String, String>) -> Vec<(String, String)> {
    env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}
