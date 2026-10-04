//! `tasq worktree <id> <path>` and `tasq worktree <id> --create <branch>`.

use std::path::Path;

use tasq_core::model::{TaskId, Worktree};
use tasq_core::store::Store;
use tasq_core::work;
use tasq_launch::{current_branch, manager_for};
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};

/// Tracks `path`, or creates a worktree for `create` and tracks that.
pub fn run(app: &App, id: &str, path: Option<&Path>, create: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    if let Some(branch) = create {
        let branch = branch.trim();
        if branch.is_empty() {
            return Err(CliError::user("the branch name must not be empty"));
        }
        let task = store.get(&id)?;
        let project = work::project_dir(&task, app.config().work.default_project.as_deref())
            .ok_or_else(|| {
                CliError::user(format!(
                    "task {id} tracks no project and work.default_project is unset; set one with tasq project {id} <path>"
                ))
            })?;
        let manager = manager_for(
            app.config().work.worktree_manager,
            app.config().work.worktree_command.as_deref(),
            app.env_vec(),
        )
        .map_err(|e| CliError::user(e.to_string()))?;
        let created = manager
            .create(&project, branch)
            .map_err(|e| CliError::user(e.to_string()))?;
        if !app.out.json_mode() {
            for message in &created.messages {
                app.out.print(&format!("{message}\n"))?;
            }
        }
        return track(app, &mut store, &id, &created.path, Some(created.branch));
    }
    let path =
        path.ok_or_else(|| CliError::user("a worktree path or --create <branch> is required"))?;
    track(app, &mut store, &id, path, None)
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
