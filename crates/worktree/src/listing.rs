//! The rows of `tasq worktree ls`, which `switch` also offers.
//! [`tasq_core::worktree::listing`] picks the declared worktrees in scope;
//! this finds the repositories, looks at the directories and asks git what
//! changed in them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tasq_core::model::Task;
use tasq_core::worktree::listing::{self, Row, Scope};

use crate::release::canonical;
use crate::{Git, Repo, RepoSettings};

/// The scope of `ls [ID] [--all]` run from `cwd`: see [`Scope::of`].
pub fn scope(one: bool, all: bool, cwd: &Path) -> Scope {
    Scope::of(one, all, || Repo::of(cwd).map(|r| r.main))
}

/// The rows of the declared worktrees of `tasks` in `scope`. `project_of`
/// gives a task's project directory, `None` when it has none;
/// `settings_of` the settings of a project, loaded once per project and
/// only for one with an existing worktree. A worktree's changes leave out
/// the files provisioning wrote; they are `None` when it does not exist,
/// its task has no project or git cannot say.
pub fn rows(
    git: &Git,
    tasks: &[Task],
    scope: &Scope,
    project_of: impl Fn(&Task) -> Option<PathBuf>,
    mut settings_of: impl FnMut(&Path) -> Option<RepoSettings>,
) -> Vec<Row> {
    let repo_of = |task: &Task| {
        project_of(task)
            .as_deref()
            .and_then(Repo::of)
            .map(|r| r.main)
    };
    let mut settings: BTreeMap<PathBuf, Option<RepoSettings>> = BTreeMap::new();
    listing::listed(tasks, scope, repo_of, Path::is_dir)
        .iter()
        .map(|listed| {
            let changes = match project_of(listed.task) {
                Some(project) if listed.exists => {
                    let path = canonical(&listed.worktree.path);
                    let skip = settings
                        .entry(project)
                        .or_insert_with_key(|project| settings_of(project))
                        .as_ref()
                        .map(|settings| settings.provisioned(&path))
                        .unwrap_or_default();
                    git.changes(&path, &skip).ok()
                }
                _ => None,
            };
            Row::new(listed, changes)
        })
        .collect()
}
