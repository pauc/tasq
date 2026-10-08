//! `tasq worktree <id> [BRANCH|PATH]`: declare a worktree or track an
//! existing one; `--create <branch>` (hidden) creates it now.

use std::path::{Path, PathBuf};

use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::store::Store;
use tasq_core::work;
use tasq_core::work::WorktreeManager;
use tasq_core::worktree::lifecycle::Declared;
use tasq_core::worktree::{declaration, git, vars};
use tasq_launch::{GitManager, current_branch};
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};

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
            track(app, &mut store, &id, Path::new(arg), None)
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
fn project_of(app: &App, task: &Task) -> Result<PathBuf> {
    work::project_dir(task, app.config().work.default_project.as_deref()).ok_or_else(|| {
        let id = &task.id;
        CliError::user(format!(
            "task {id} tracks no project and work.default_project is unset; set one with tasq project {id} <path>"
        ))
    })
}

/// Declares a worktree for `task` from `project` on `branch` (the
/// configured default without one); also whether it is new. Templates see
/// the repository's main checkout as `{{project}}` when `project` is in a
/// git work tree.
pub fn declare(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    project: &Path,
) -> Result<(Worktree, bool)> {
    let repo = Repo::of(project);
    declare_in(app, task, branch, project, repo.as_ref())
}

/// [`declare`] with the repository already probed.
pub fn declare_in(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    project: &Path,
    repo: Option<&Repo>,
) -> Result<(Worktree, bool)> {
    let declared = declaration::declare(
        task,
        branch,
        &app.config().worktree,
        repo.map_or(project, |r| &r.main),
        repo.and_then(|r| r.name.as_deref()),
        app.opts.home.as_deref(),
    )
    .map_err(|e| CliError::user(e.to_string()))?;
    Ok(match declared {
        Declared::New(worktree) => (worktree, true),
        Declared::Already(worktree) => (worktree, false),
    })
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

/// Creates the worktree for `branch` with the configured manager, in the
/// task's project (or the default), and tracks it. Also used by `next`
/// and `pick` to recreate a worktree that is gone.
pub fn create_and_track(app: &App, store: &mut NbStore, id: &TaskId, branch: &str) -> Result<()> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(CliError::user("the branch name must not be empty"));
    }
    let project = project_of(app, &store.get(id)?)?;
    let manager = GitManager { env: app.env_vec() };
    let created = manager
        .create(&project, branch)
        .map_err(|e| CliError::user(e.to_string()))?;
    if !app.out.json_mode() {
        for message in &created.messages {
            app.out.print(&format!("{message}\n"))?;
        }
    }
    track(app, store, id, &created.path, Some(created.branch))
}

/// Records `path` (which must exist) with its current branch, unless it is
/// already tracked.
fn track(
    app: &App,
    store: &mut NbStore,
    id: &TaskId,
    path: &Path,
    branch_hint: Option<String>,
) -> Result<()> {
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
    let branch = current_branch(&dir, &app.env_vec()).or(branch_hint);
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
