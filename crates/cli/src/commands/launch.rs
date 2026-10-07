//! `tasq next` and `tasq pick`: open a work session (plan T-401 to T-405).

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use serde_json::Value;
use tasq_core::config::ENV_PROFILE;
use tasq_core::launch::{ENV_NOTEBOOK, ENV_TASK_ID, LaunchContext, LaunchOutcome, resolve_workdir};
use tasq_core::model::{Status, TaskId};
use tasq_core::query::{self, Filter};
use tasq_core::store::{Store, StoreError};
use tasq_launch::prompt::DEFAULT_TEMPLATE;
use tasq_launch::registry::resolve_name;
use tasq_launch::{LaunchSettings, launcher_for, resolve_detached};
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::commands::worktree::create_and_track;
use crate::error::{CliError, Result};
use crate::json;
use crate::plugins::{self, Hook};

/// How `next`/`pick` open the session: `--launcher`, `--detached`,
/// `--no-focus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct How {
    /// `--launcher`: a launcher name overriding the configured one.
    pub launcher: Option<String>,
    /// `--detached`: a new window with `launch.detached` instead of this
    /// terminal with `launch.default`.
    pub detached: bool,
    /// `--no-focus`: leave the new window in the background.
    pub focus: bool,
}

impl How {
    /// From the flags as clap parsed them.
    pub fn new(launcher: Option<&str>, detached: bool, no_focus: bool) -> Self {
        Self {
            launcher: launcher.map(str::to_owned),
            detached,
            focus: !no_focus,
        }
    }

    /// `tasq pick <id>` in this terminal with the configured launcher.
    pub fn here() -> Self {
        Self::new(None, false, false)
    }

    /// The launcher name to build, before `auto` is resolved: the flag,
    /// else `launch.detached` or `launch.default`. A detached `auto` is
    /// resolved here (herdr or tmux, else an error naming the config key)
    /// because unlike `launch.default` it has no in-terminal fallback.
    pub fn launcher_name(&self, app: &App) -> Result<String> {
        if let Some(name) = &self.launcher {
            return Ok(name.clone());
        }
        let launch = &app.config().launch;
        if self.detached {
            resolve_detached(&launch.detached, &app.env_vec())
                .map(str::to_owned)
                .map_err(|e| CliError::user(format!("launch.detached: {e}")))
        } else {
            Ok(launch.default.clone())
        }
    }
}

/// `tasq next`: the first in-progress task, else the first ready one.
pub fn next(app: &App, how: &How, dry_run: bool) -> Result<()> {
    let workflow = app.workflow();
    let mut store = app.open_store()?;
    let open = store.list(&Filter::default())?;
    let Some(task) = query::next(&open, &workflow) else {
        return Err(CliError::user(format!(
            "no {} todos",
            workflow
                .statuses
                .iter()
                .take(query::NEXT_STATUSES)
                .map(Status::as_str)
                .collect::<Vec<_>>()
                .join(" or ")
        )));
    };
    let id = task.id.clone();
    open_session(app, &mut store, &id, how, dry_run)
}

/// `tasq pick <id>`.
pub fn pick(app: &App, id: &str, how: &How, dry_run: bool) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    open_session(app, &mut store, &id, how, dry_run)
}

/// Launcher settings from the config: the environment, `launch.env`, the
/// prompt template (`launch.claude.prompt_file` or the built-in one) and
/// the default project.
pub fn launch_settings(app: &App) -> Result<LaunchSettings> {
    let template = match &app.config().launch.claude.prompt_file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| {
            CliError::user(format!("launch.claude.prompt_file {}: {e}", file.display()))
        })?,
        None => DEFAULT_TEMPLATE.to_owned(),
    };
    Ok(LaunchSettings {
        env: app.env_vec(),
        strategy: app.config().launch.env,
        template,
        default_project: app.config().work.default_project.clone(),
        placement: app.config().launch.herdr.placement,
    })
}

/// The script's `open_session`.
fn open_session(
    app: &App,
    store: &mut NbStore,
    id: &TaskId,
    how: &How,
    dry_run: bool,
) -> Result<()> {
    // Before any write: a `--detached` with nowhere to open is a usage
    // error, not a task left in-progress with no session.
    let name = how.launcher_name(app)?;
    let mut task = store.get(id)?;
    if task.done {
        return Err(CliError::user(format!(
            "task {id} is done; reopen it first (tasq reopen {id})"
        )));
    }
    if task.status.as_ref() != Some(&Status::IN_PROGRESS) {
        if dry_run {
            say(app, &format!("[{id}] -> in-progress (skipped: dry run)\n"))?;
        } else {
            task.set_status(Status::IN_PROGRESS);
            store.update(&task)?;
            say(app, &format!("[{id}] -> in-progress\n"))?;
        }
    }
    say(app, &format!("Task: [{id}] {}\n", task.title))?;
    let file = store.path_of(id)?;
    let markdown = std::fs::read_to_string(&file).map_err(|source| StoreError::Io {
        path: file.clone(),
        source,
    })?;

    let default_project = app.config().work.default_project.as_deref();
    let mut resolution = resolve_workdir(&task, default_project, Path::is_dir)
        .map_err(|e| CliError::user(e.to_string()))?;
    for warning in &resolution.warnings {
        app.out.warn(warning);
    }
    if let Some(missing) = &resolution.missing_worktree {
        app.out.warn(&format!(
            "tracked worktree is gone: {}",
            missing.path.display()
        ));
        if recreate(app, store, id, missing.branch.as_deref(), dry_run)? {
            task = store.get(id)?;
            resolution = resolve_workdir(&task, default_project, Path::is_dir)
                .map_err(|e| CliError::user(e.to_string()))?;
        }
    }
    if resolution.in_worktree {
        say(
            app,
            &format!(
                "Starting in tracked worktree: {}\n",
                resolution.workdir.display()
            ),
        )?;
    } else {
        say(
            app,
            &format!("Starting in project: {}\n", resolution.workdir.display()),
        )?;
    }

    let mut env = vec![
        (ENV_TASK_ID.to_owned(), id.to_string()),
        (ENV_NOTEBOOK.to_owned(), app.config().store.notebook.clone()),
    ];
    if let Some(profile) = &app.loaded.profile {
        env.push((ENV_PROFILE.to_owned(), profile.clone()));
    }
    let ctx = LaunchContext {
        statuses: app
            .workflow()
            .statuses
            .iter()
            .map(ToString::to_string)
            .collect(),
        task,
        file,
        markdown,
        workdir: resolution.workdir,
        in_worktree: resolution.in_worktree,
        env,
        focus: how.focus,
    };
    launch_or_describe(app, &ctx, &name, dry_run)
}

/// Builds the launcher called `name` and either describes (`--dry-run`,
/// as text or JSON) or runs it.
fn launch_or_describe(app: &App, ctx: &LaunchContext, name: &str, dry_run: bool) -> Result<()> {
    let settings = launch_settings(app)?;
    let resolved = resolve_name(name, &settings.env).to_owned();
    let launcher = launcher_for(name, &settings).map_err(|e| CliError::user(e.to_string()))?;
    let hook_fields = [
        ("workdir", json::to_value(&ctx.workdir)),
        ("in_worktree", Value::from(ctx.in_worktree)),
        ("launcher", Value::from(resolved.as_str())),
    ];

    if dry_run {
        let mut steps = launcher
            .describe(ctx)
            .map_err(|e| CliError::user(e.to_string()))?;
        for hook in &app.config().hooks.pre_launch {
            steps.insert(0, format!("pre-launch hook (skipped: dry run): {hook}"));
        }
        if app.out.json_mode() {
            return app.out.json(&json::document([
                ("task", json::to_value(&ctx.task)),
                ("workdir", json::to_value(&ctx.workdir)),
                ("in_worktree", Value::from(ctx.in_worktree)),
                ("launcher", Value::from(resolved)),
                ("env", json::to_value(&ctx.env)),
                ("steps", json::to_value(&steps)),
            ]));
        }
        let mut text = format!("Launcher: {resolved} (dry run)\n");
        for step in steps {
            text.push_str(&step);
            text.push('\n');
        }
        return app.out.page(&text);
    }
    plugins::run_hooks(app, Hook::PreLaunch, &ctx.task, &hook_fields)?;
    match launcher
        .launch(ctx)
        .map_err(|e| CliError::user(e.to_string()))?
    {
        LaunchOutcome::Opened(detail) => app.out.print(&format!("{detail}\n")),
        LaunchOutcome::Replaced => Ok(()),
    }
}

/// Prints progress text unless `--json` is on (the JSON document must be
/// the only thing on stdout).
fn say(app: &App, text: &str) -> Result<()> {
    if app.out.json_mode() {
        Ok(())
    } else {
        app.out.print(text)
    }
}

/// Offers to recreate the newest tracked worktree on its recorded branch.
/// Only asks on a terminal; returns whether a worktree was created.
fn recreate(
    app: &App,
    store: &mut NbStore,
    id: &TaskId,
    branch: Option<&str>,
    dry_run: bool,
) -> Result<bool> {
    let Some(branch) = branch else {
        app.out
            .warn("no branch recorded for it, starting in the project instead");
        return Ok(false);
    };
    if dry_run {
        app.out.warn(&format!(
            "dry run: would offer to recreate it on branch {branch}; starting in the project instead"
        ));
        return Ok(false);
    }
    if !std::io::stdin().is_terminal() {
        app.out
            .warn("no terminal to ask on, starting in the project instead");
        return Ok(false);
    }
    eprint!("Recreate it on branch {branch}? [y/N] ");
    std::io::stderr().flush()?;
    let mut reply = String::new();
    std::io::stdin().lock().read_line(&mut reply)?;
    if !reply.trim_start().starts_with(['y', 'Y']) {
        return Ok(false);
    }
    match create_and_track(app, store, id, branch) {
        Ok(()) => Ok(true),
        Err(e) => {
            // The session can still start in the project directory.
            app.out
                .warn(&format!("could not recreate the worktree: {e}"));
            Ok(false)
        }
    }
}
