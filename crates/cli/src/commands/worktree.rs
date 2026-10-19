//! `tasq worktree set <id> [BRANCH|PATH]`: declare a worktree or track an
//! existing one; `tasq worktree up <id>` creates the declared one now, as a
//! launch does; `--create <branch>` (hidden) is "declare, then up";
//! `tasq worktree release <id>` does what closing the task does to its
//! worktrees ([`release()`], also run by `tasq done` and the TUI's `d`).

use std::path::{Path, PathBuf};

use crate::commands::worktree_hooks::{self, Approval};
use crate::commands::worktree_nav;
use tasq_core::config::project_file;
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::store::Store;
use tasq_core::work;
use tasq_core::worktree::declaration;
use tasq_core::worktree::hooks::Kind;
use tasq_core::worktree::lifecycle::Declared;
use tasq_worktree::Repo;
pub use tasq_worktree::RepoSettings;
pub use tasq_worktree::create::Created;
use tasq_worktree::create::{self, CreateError};
use tasq_worktree::release;
pub use tasq_worktree::release::Releasing;

use crate::app::App;
use crate::cli::{Command, WorktreeAction};
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};
use crate::json;
use crate::plugins::HookEvent;

/// `tasq worktree <subcommand>`.
pub fn dispatch(app: &App, command: &Command) -> Result<()> {
    let Command::Worktree { action } = command else {
        unreachable!("dispatch is only called for the worktree command");
    };
    match action {
        WorktreeAction::Set { id, target, create } => {
            run(app, id, target.as_deref(), create.as_deref())
        }
        WorktreeAction::Up {
            id,
            from,
            dry_run,
            yes,
        } => up(app, id, from.as_deref(), *yes, *dry_run),
        WorktreeAction::Trust { dir } => trust(app, dir.as_deref()),
        WorktreeAction::Ls { id, all } => worktree_nav::ls(app, id.as_deref(), *all),
        WorktreeAction::Switch { target, yes } => {
            worktree_nav::switch(app, target.as_deref(), *yes)
        }
        WorktreeAction::Path { target } => worktree_nav::path(app, target),
        WorktreeAction::ShellInit { shell } => worktree_nav::shell_init(app, *shell),
        WorktreeAction::Release {
            id,
            force,
            dry_run,
            yes,
        } => release_command(app, id, *force, *yes, *dry_run),
        WorktreeAction::Prune { all, dry_run, yes } => {
            worktree_nav::prune(app, *all, *yes, *dry_run)
        }
    }
}

/// Declares a worktree on `target` (a branch, or the default without one),
/// tracks `target` when it is a path, or creates a worktree for `create`.
/// Refused when worktrees are disabled (ADR 0022).
pub fn run(app: &App, id: &str, target: Option<&str>, create: Option<&str>) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    if let Some(branch) = create {
        return create_and_track(app, &mut store, &id, branch);
    }
    match target {
        Some(arg) if declaration::is_path(arg, Path::new(arg).is_dir()) => {
            track(app, &mut store, &id, Path::new(arg))
        }
        branch => {
            let mut task = store.get(&id)?;
            let (worktree, new) = declare_and_save(app, &mut store, &mut task, branch)?;
            let already = if new { "" } else { "already " };
            finish(
                app,
                &mut store,
                &id,
                &format!(
                    "[{id}] worktree {already}declared: {}\n",
                    describe(&worktree)
                ),
            )
        }
    }
}

/// The error for a worktree command or flag while the feature is off.
pub fn disabled() -> CliError {
    CliError::user("worktrees are disabled; set worktree.enabled = true (docs/config.md#worktrees)")
}

/// `path (branch)`, or the path alone when no branch is recorded.
pub fn describe(worktree: &Worktree) -> String {
    let path = worktree.path.display();
    worktree
        .branch
        .as_ref()
        .map_or_else(|| path.to_string(), |b| format!("{path} ({b})"))
}

/// The task's project, else `work.default_project`.
pub fn project_of(app: &App, task: &Task) -> Result<PathBuf> {
    work::project_dir(task, app.config().work.default_project.as_deref()).ok_or_else(|| {
        let id = &task.id;
        CliError::user(format!(
            "task {id} tracks no project and work.default_project is unset; set one with tasq project {id} <path>"
        ))
    })
}

/// Declares a worktree for `task` from `project` on `branch` (the
/// configured default without one); also whether it is new.
pub fn declare(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    project: &Path,
) -> Result<(Worktree, bool)> {
    declare_in(app, task, branch, &repo_settings(app, project)?)
}

/// [`declare`] in the task's project, added to `task` and saved when it is
/// new: the worktree and whether it is new.
fn declare_and_save(
    app: &App,
    store: &mut dyn Store,
    task: &mut Task,
    branch: Option<&str>,
) -> Result<(Worktree, bool)> {
    let project = project_of(app, task)?;
    let (worktree, new) = declare(app, task, branch, &project)?;
    if new {
        task.add_worktree(worktree.clone());
        store.update(task)?;
    }
    Ok((worktree, new))
}

/// [`declare`] with the repository's settings loaded already. Templates
/// see the repository's main checkout as `{{project}}`.
pub fn declare_in(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    settings: &RepoSettings,
) -> Result<(Worktree, bool)> {
    let declared = declaration::declare(
        task,
        branch,
        &settings.config,
        &settings.main,
        settings.name(),
        app.opts.home.as_deref(),
    )
    .map_err(|e| CliError::user(e.to_string()))?;
    Ok(match declared {
        Declared::New(worktree) => (worktree, true),
        Declared::Already(worktree) => (worktree, false),
    })
}

/// The worktree settings of the repository `project` is in: `[worktree]`
/// as loaded from its main checkout, so the `.tasq.toml` next to the
/// repository applies wherever tasq runs (plan 4.4).
pub fn repo_settings(app: &App, project: &Path) -> Result<RepoSettings> {
    let repo = Repo::of(project);
    let main = repo.as_ref().map_or(project, |r| &r.main).to_path_buf();
    let loaded = app.config_at(&main)?;
    let templates_dir = loaded
        .file_for("worktree.templates")
        .and_then(Path::parent)
        .map_or_else(|| main.clone(), Path::to_path_buf);
    let repo_file = project_file(&main, app.opts.home.as_deref());
    let untrusted = Kind::ALL
        .into_iter()
        .filter(|kind| {
            repo_file.is_some()
                && loaded.file_for(&format!("worktree.hooks.{}", kind.key()))
                    == repo_file.as_deref()
        })
        .collect();
    Ok(RepoSettings {
        project: project.to_path_buf(),
        repo,
        main,
        config: loaded.config.worktree,
        templates_dir,
        repo_file,
        untrusted,
    })
}

/// `tasq worktree set <id> --create <branch>`: declares a worktree on
/// `branch` and creates it, unless its directory exists already.
pub fn create_and_track(app: &App, store: &mut dyn Store, id: &TaskId, branch: &str) -> Result<()> {
    let mut task = store.get(id)?;
    let (worktree, _) = declare_and_save(app, store, &mut task, Some(branch))?;
    let only = Task {
        worktrees: vec![worktree.clone()],
        ..task
    };
    let line = match materialize(app, &only, None, false, false)? {
        Some(created) => format!("[{id}] created worktree {}\n", created.line),
        None => format!("[{id}] worktree: {}\n", describe(&worktree)),
    };
    finish(app, store, id, &line)
}

/// `tasq worktree up <id> [--from REF] [--dry-run]`: creates the task's
/// declared worktree now, as launching it would.
pub fn up(app: &App, id: &str, from: Option<&str>, yes: bool, dry_run: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let task = store.get(&id)?;
    let line = match materialize(app, &task, from, yes, dry_run)? {
        Some(created) if dry_run => {
            let mut lines = vec![format!("[{id}] would create worktree {}", created.line)];
            lines.extend(created.steps.iter().map(|step| format!("  {step}")));
            lines.join("\n") + "\n"
        }
        Some(created) => format!("[{id}] created worktree {}\n", created.line),
        None => match task.worktrees.iter().find(|w| w.path.is_dir()) {
            Some(existing) => format!("[{id}] worktree exists: {}\n", describe(existing)),
            None => {
                return Err(CliError::user(format!(
                    "task {id} declares no worktree with a branch; declare one with tasq worktree set {id} [BRANCH]"
                )));
            }
        },
    };
    finish(app, &mut store, &id, &line)
}

/// `tasq worktree trust [DIR]`: approves the hooks of the repository's own
/// `.tasq.toml` ahead of time.
pub fn trust(app: &App, dir: Option<&Path>) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let dir = dir.unwrap_or(&app.opts.cwd);
    let dir = existing_dir(dir)
        .ok_or_else(|| CliError::user(format!("directory not found: {}", dir.display())))?;
    let settings = repo_settings(app, &dir)?;
    let approved = worktree_hooks::trust(app, &settings)?;
    let file = settings.repo_file.as_deref().unwrap_or(&settings.main);
    if app.out.json_mode() {
        return app.out.json(&trusted_json(file, &approved));
    }
    let text = if approved.is_empty() {
        format!("no hooks to approve in {}\n", file.display())
    } else {
        let mut lines = vec![format!("approved the hooks of {}:", file.display())];
        lines.extend(
            approved
                .iter()
                .map(|(kind, line)| format!("  {}: {line}", kind.key())),
        );
        lines.join("\n") + "\n"
    };
    app.out.print(&text)
}

/// The JSON document of `trust`: the file and the lines it approved.
fn trusted_json(file: &Path, approved: &[(Kind, String)]) -> serde_json::Value {
    let approved = approved
        .iter()
        .map(|(kind, line)| serde_json::json!({"kind": kind.key(), "line": line}))
        .collect();
    json::document([
        ("file", serde_json::json!(file)),
        ("approved", serde_json::Value::Array(approved)),
    ])
}

/// Step 2 of the lifecycle ([`tasq_worktree::create::materialize`]) in
/// the task's project, with `--yes` as `yes` and the hooks' output on the
/// terminal. The caller checks that worktrees are enabled.
pub fn materialize(
    app: &App,
    task: &Task,
    from: Option<&str>,
    yes: bool,
    dry_run: bool,
) -> Result<Option<Created>> {
    let project = project_of(app, task);
    let settings = match &project {
        Ok(project) => Some(repo_settings(app, project)?),
        Err(_) => None,
    };
    let approver = worktree_hooks::approver(app, Approval::from_yes(yes));
    create::materialize(
        &app.git(),
        task,
        settings.as_ref(),
        from,
        &approver,
        dry_run,
        &mut worktree_hooks::to_terminal(app),
    )
    .map_err(|e| match (e, project) {
        (CreateError::NoProject, Err(no_project)) => no_project,
        (e, _) => e.into(),
    })
}

/// Records `path` (which must exist) with its current branch, unless it is
/// already tracked.
fn track(app: &App, store: &mut dyn Store, id: &TaskId, path: &Path) -> Result<()> {
    let dir = existing_dir(path)
        .ok_or_else(|| CliError::user(format!("worktree path not found: {}", path.display())))?;
    let mut task = store.get(id)?;
    if task.worktrees.iter().any(|w| w.path == dir) {
        return finish(
            app,
            store,
            id,
            &format!("[{id}] worktree already tracked: {}\n", dir.display()),
        );
    }
    let branch = app.git().current_branch(&dir);
    task.add_worktree(Worktree {
        path: dir.clone(),
        branch: branch.clone(),
    });
    store.update(&task)?;
    let suffix = branch.map_or_else(String::new, |b| format!(" ({b})"));
    finish(
        app,
        store,
        id,
        &format!("[{id}] worktree: {}{suffix}\n", dir.display()),
    )
}

/// `tasq worktree release <id> [--force] [--dry-run] [--yes]`: what closing
/// the task does to its worktrees, whatever its status and `on_done`.
pub fn release_command(app: &App, id: &str, force: bool, yes: bool, dry_run: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let task = store.get(&id)?;
    let how = Releasing::on_demand(force, dry_run);
    let approval = Approval::from_yes(yes);
    let lines = release(
        app,
        &task,
        how,
        approval,
        &mut worktree_hooks::to_terminal(app),
    )?;
    let text = if lines.is_empty() {
        format!("[{id}] nothing released\n")
    } else {
        prefixed(&id, &lines)
    };
    finish(app, &mut store, &id, &text)
}

/// `lines`, one per line, each prefixed with `[id] ` unless indented (the
/// steps under a dry run's line).
pub fn prefixed(id: &TaskId, lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| {
            if l.starts_with(' ') {
                format!("{l}\n")
            } else {
                format!("[{id}] {l}\n")
            }
        })
        .collect::<Vec<_>>()
        .concat()
}

/// Step 3 of the lifecycle ([`tasq_worktree::release::release`]) in the
/// task's project, run from the current directory, with `approval` for the
/// repository's own remove hooks. The caller checks that worktrees are
/// enabled.
///
/// # Errors
///
/// When the task's repository cannot be read; a removal or a deletion
/// that fails is a warning.
pub fn release(
    app: &App,
    task: &Task,
    how: Releasing,
    approval: Approval,
    report: &mut dyn FnMut(HookEvent),
) -> Result<Vec<String>> {
    if task.worktrees.is_empty() {
        return Ok(Vec::new());
    }
    let settings = repo_settings(app, &project_of(app, task)?)?;
    Ok(release::release(
        &app.git(),
        task,
        &settings,
        &app.opts.cwd,
        how,
        &worktree_hooks::approver(app, approval),
        report,
    )?)
}
