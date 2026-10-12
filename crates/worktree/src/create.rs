//! Step 2 of the lifecycle: creating a task's declared worktree, as a
//! launch does. [`tasq_core::worktree::lifecycle::on_launch`] decides what
//! to create; this module runs `git`, provisions the checkout and runs the
//! `pre-create` and `post-create` hooks around it.

use std::path::{Path, PathBuf};

use tasq_core::model::Task;
use tasq_core::worktree::hooks::{self, HookEvent, Kind};
use tasq_core::worktree::lifecycle::{self, BranchState, LaunchAction};
use tasq_core::worktree::provision::{self, Outside};
use thiserror::Error;

use crate::hooks::{Approver, HookFailure, prepare as prepare_hooks, run_all};
use crate::provision::{ProvisionError, apply, prepare};
use crate::settings::RepoSettings;
use crate::{Git, GitError, describe};

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

/// Why a worktree could not be created.
#[derive(Debug, Error)]
pub enum CreateError {
    /// A worktree has to be created and the task has no project to create
    /// it in; the caller knows how to say so.
    #[error("the task tracks no project")]
    NoProject,
    /// `git` failed.
    #[error(transparent)]
    Git(#[from] GitError),
    /// A copy, link or template target leaves the worktree.
    #[error(transparent)]
    Outside(#[from] Outside),
    /// A template does not render: nothing was created.
    #[error(transparent)]
    Template(#[from] ProvisionError),
    /// The worktree exists, but copying, linking or writing into it failed.
    #[error("worktree {} was created, but provisioning it failed: {source}", .path.display())]
    Provisioning {
        /// The worktree.
        path: PathBuf,
        /// What failed.
        source: ProvisionError,
    },
    /// A hook cannot be prepared, or a `pre-create` one failed.
    #[error(transparent)]
    Hooks(#[from] HookFailure),
}

/// When none of `task`'s declared worktrees exists, creates the first one
/// that records a branch, in the repository `settings` describes (`None`
/// when the task has no project), and provisions it (copies, links,
/// templates). A new branch starts from `from`, else `worktree.base`.
/// Templates are rendered before git runs, so a broken one stops
/// everything. A dry run decides and renders without fetching or creating
/// anything. `None` when a declared worktree exists or none can be
/// created. The `pre-create` hooks run in the main checkout before git,
/// the `post-create` ones in the worktree after provisioning, with
/// `git.env` plus the hook variables; `approver` approves the repository's
/// own hooks. Notes and warnings go to `report`.
///
/// # Errors
///
/// [`CreateError`]: a `pre-create` hook or `git` failed, or provisioning
/// cannot be planned or done.
pub fn materialize(
    git: &Git,
    task: &Task,
    settings: Option<&RepoSettings>,
    from: Option<&str>,
    approver: &Approver,
    dry_run: bool,
    report: &mut dyn FnMut(HookEvent),
) -> Result<Option<Created>, CreateError> {
    let base = settings.map_or("", |settings| from.unwrap_or(&settings.config.base));
    let mut failed = None;
    let action = lifecycle::on_launch(
        &task.worktrees,
        Path::is_dir,
        |branch| {
            let state = match settings {
                Some(settings) => git
                    .branch_state(&settings.project, branch, !dry_run)
                    .map_err(CreateError::from),
                None => Err(CreateError::NoProject),
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
        Some(settings),
    ) = (action, settings)
    else {
        return Ok(None);
    };
    let (start, note) = git.resolve_start(&settings.project, &branch, &start);
    for note in note.into_iter().chain(start.fresh_note(&branch)) {
        report(HookEvent::Warning(note));
    }
    let steps = provision::plan(
        &settings.config,
        &settings.main,
        &path,
        &settings.templates_dir,
    )?;
    let vars = settings.vars(task, &branch, &path);
    let ready = prepare(&steps, &vars)?;
    let [pre, post] = prepare_hooks(
        settings,
        [Kind::PreCreate, Kind::PostCreate],
        &vars,
        approver,
        dry_run,
        report,
    )?;
    if !dry_run {
        let mut env = git.env.clone();
        env.extend(hooks::env(&vars));
        run_all(&pre, &settings.main, &env, report)?;
        git.add(&settings.project, &path, &branch, &start)?;
        let warnings = apply(&ready).map_err(|source| CreateError::Provisioning {
            path: path.clone(),
            source,
        })?;
        for warning in warnings {
            report(HookEvent::Warning(warning));
        }
        run_all(&post, &path, &env, report)?;
    }
    let mut lines = pre.describe();
    lines.extend(steps.iter().map(|step| step.describe(&path)));
    lines.extend(post.describe());
    Ok(Some(Created {
        line: describe(&path, &branch, &start),
        steps: lines,
        path,
    }))
}
