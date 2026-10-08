//! `tasq worktree <id> [BRANCH|PATH]`: declare a worktree or track an
//! existing one; `tasq worktree up <id>` creates the declared one now, as a
//! launch does; `--create <branch>` (hidden) is "declare, then up".

use std::path::{Path, PathBuf};

use tasq_core::config::WorktreeConfig;
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::store::Store;
use tasq_core::work;
use tasq_core::worktree::lifecycle::{self, BranchState, Declared, LaunchAction};
use tasq_core::worktree::vars::Vars;
use tasq_core::worktree::{declaration, git, provision, vars};
use tasq_store_nb::NbStore;
use tasq_worktree::provision::{apply, prepare};
use tasq_worktree::{Git, describe as describe_start};

use crate::app::App;
use crate::cli::{Command, WorktreeAction};
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};

/// `tasq worktree`: a subcommand, else declare or track (`run`).
pub fn dispatch(app: &App, command: &Command) -> Result<()> {
    let Command::Worktree {
        action,
        id,
        target,
        create,
    } = command
    else {
        unreachable!("dispatch is only called for the worktree command");
    };
    match action {
        Some(WorktreeAction::Up { id, from, dry_run }) => up(app, id, from.as_deref(), *dry_run),
        None => run(
            app,
            id.as_deref().unwrap_or_default(),
            target.as_deref(),
            create.as_deref(),
        ),
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
            let project = project_of(app, &task)?;
            let (worktree, new) = declare(app, &task, branch, &project)?;
            if new {
                task.add_worktree(worktree.clone());
                store.update(&task)?;
            }
            let already = if new { "" } else { "already " };
            finish(
                app,
                &store,
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
    CliError::user(
        "worktrees are disabled; set worktree.enabled = true (docs/config.md, \"Worktrees\")",
    )
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
    declare_in(app, task, branch, &RepoSettings::load(app, project)?)
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

/// The worktree settings of the repository a task works in: `[worktree]`
/// as loaded from its main checkout, so the `.tasq.toml` next to the
/// repository applies wherever tasq runs (plan 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSettings {
    /// The task's project directory, where `git` runs.
    pub project: PathBuf,
    /// The repository, when `project` is inside one.
    pub repo: Option<Repo>,
    /// The main checkout (`project` outside a repository).
    pub main: PathBuf,
    /// `[worktree]` as configured for the main checkout.
    pub config: WorktreeConfig,
    /// Where relative template sources start: the directory of the file
    /// that set `worktree.templates`, else the main checkout.
    pub templates_dir: PathBuf,
}

impl RepoSettings {
    /// The settings for `project`.
    pub fn load(app: &App, project: &Path) -> Result<Self> {
        let repo = Repo::of(project);
        let main = repo.as_ref().map_or(project, |r| &r.main).to_path_buf();
        let loaded = app.config_at(&main)?;
        let templates_dir = loaded
            .file_for("worktree.templates")
            .and_then(Path::parent)
            .map_or_else(|| main.clone(), Path::to_path_buf);
        Ok(Self {
            project: project.to_path_buf(),
            repo,
            main,
            config: loaded.config.worktree,
            templates_dir,
        })
    }

    /// The repository name for `{{repo}}`, when known.
    pub fn name(&self) -> Option<&str> {
        self.repo.as_ref().and_then(|r| r.name.as_deref())
    }
}

/// A git repository as found on disk, without running `git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The main checkout: where the shared git directory lives, so a
    /// linked worktree names the repository it belongs to.
    pub main: PathBuf,
    /// The repository name from `remote.origin.url`, when there is one.
    pub name: Option<String>,
}

impl Repo {
    /// The repository `dir` is inside, if any.
    pub fn of(dir: &Path) -> Option<Self> {
        let root = git::work_tree_root(dir, Path::exists)?;
        let dot_git = root.join(".git");
        let git_dir = if dot_git.is_file() {
            let text = std::fs::read_to_string(&dot_git).unwrap_or_default();
            git::gitdir(&text).map_or_else(|| dot_git.clone(), |d| root.join(d))
        } else {
            dot_git
        };
        let common = std::fs::read_to_string(git_dir.join("commondir"))
            .map_or_else(|_| git_dir.clone(), |rel| git_dir.join(rel.trim()));
        let common = common.canonicalize().unwrap_or(common);
        let main = match common.parent() {
            Some(parent) if common.file_name().is_some_and(|n| n == ".git") => parent.to_path_buf(),
            _ => root.to_path_buf(),
        };
        let name = std::fs::read_to_string(common.join("config"))
            .ok()
            .and_then(|config| git::origin_url(&config))
            .and_then(|url| vars::repo_name(&url));
        Some(Self { main, name })
    }
}

/// `tasq worktree <id> --create <branch>`: declares a worktree on
/// `branch` and creates it, unless its directory exists already.
pub fn create_and_track(app: &App, store: &mut NbStore, id: &TaskId, branch: &str) -> Result<()> {
    let mut task = store.get(id)?;
    let project = project_of(app, &task)?;
    let (worktree, new) = declare(app, &task, Some(branch), &project)?;
    if new {
        task.add_worktree(worktree.clone());
        store.update(&task)?;
    }
    let only = Task {
        worktrees: vec![worktree.clone()],
        ..task
    };
    let line = match materialize(app, &only, None, false)? {
        Some(created) => format!("[{id}] created worktree {}\n", created.line),
        None => format!("[{id}] worktree: {}\n", describe(&worktree)),
    };
    finish(app, store, id, &line)
}

/// `tasq worktree up <id> [--from REF] [--dry-run]`: creates the task's
/// declared worktree now, as launching it would.
pub fn up(app: &App, id: &str, from: Option<&str>, dry_run: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let store = app.open_store()?;
    let task = store.get(&id)?;
    let line = match materialize(app, &task, from, dry_run)? {
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
                    "task {id} declares no worktree with a branch; declare one with tasq worktree {id} [BRANCH]"
                )));
            }
        },
    };
    finish(app, &store, &id, &line)
}

/// A worktree [`materialize`] created, or would create on a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its directory.
    pub path: PathBuf,
    /// `<path> on <branch> (<where the branch comes from>)`.
    pub line: String,
    /// The provisioning steps, one line each (`link node_modules -> ...`).
    pub steps: Vec<String>,
}

/// Step 2 of the lifecycle: when none of `task`'s declared worktrees
/// exists, creates the first one that records a branch, in the task's
/// project repository, and provisions it (copies, links, templates). A new
/// branch starts from `from`, else `worktree.base`. Templates are rendered
/// before git runs, so a broken one stops everything. A dry run decides
/// and renders without fetching or creating anything. `None` when a
/// declared worktree exists or none can be created. The caller checks
/// that worktrees are enabled.
pub fn materialize(
    app: &App,
    task: &Task,
    from: Option<&str>,
    dry_run: bool,
) -> Result<Option<Created>> {
    let git = Git { env: app.env_vec() };
    let settings = match project_of(app, task) {
        Ok(project) => Some(RepoSettings::load(app, &project)?),
        Err(_) => None,
    };
    let configured = settings
        .as_ref()
        .map_or(&app.config().worktree, |s| &s.config);
    let base = from.unwrap_or(&configured.base);
    let mut failed = None;
    let action = lifecycle::on_launch(
        &task.worktrees,
        Path::is_dir,
        |branch| {
            let state = match &settings {
                Some(s) => git
                    .branch_state(&s.project, branch, !dry_run)
                    .map_err(|e| CliError::user(e.to_string())),
                None => project_of(app, task).map(|_| BranchState::Absent),
            };
            state.unwrap_or_else(|e| {
                failed = Some(e);
                BranchState::Absent
            })
        },
        base,
    );
    if let Some(e) = failed {
        return Err(e);
    }
    let (
        LaunchAction::Materialize {
            path,
            branch,
            start,
        },
        Some(s),
    ) = (action, settings)
    else {
        return Ok(None);
    };
    let (start, note) = git.resolve_start(&s.project, &branch, &start);
    if let Some(note) = note {
        app.out.warn(&note);
    }
    let user = |e: &dyn std::fmt::Display| CliError::user(e.to_string());
    let steps =
        provision::plan(&s.config, &s.main, &path, &s.templates_dir).map_err(|e| user(&e))?;
    let vars = Vars::for_task(task, &s.main, s.name(), &s.config.variables)
        .with_branch(&branch, s.config.ports)
        .with_worktree(&path);
    let ready = prepare(&steps, &vars).map_err(|e| user(&e))?;
    if !dry_run {
        git.add(&s.project, &path, &branch, &start)
            .map_err(|e| user(&e))?;
        let warnings = apply(&ready).map_err(|e| {
            CliError::user(format!(
                "worktree {} was created, but provisioning it failed: {e}",
                path.display()
            ))
        })?;
        for warning in warnings {
            app.out.warn(&warning);
        }
    }
    Ok(Some(Created {
        line: describe_start(&path, &branch, &start),
        steps: steps.iter().map(|step| step.describe(&path)).collect(),
        path,
    }))
}

/// Records `path` (which must exist) with its current branch, unless it is
/// already tracked.
fn track(app: &App, store: &mut NbStore, id: &TaskId, path: &Path) -> Result<()> {
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
    let branch = Git { env: app.env_vec() }.current_branch(&dir);
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
