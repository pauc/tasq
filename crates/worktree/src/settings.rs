//! The worktree settings of the repository a task works in.

use std::path::{Path, PathBuf};

use tasq_core::config::WorktreeConfig;
use tasq_core::model::Task;
use tasq_core::worktree::hooks::Kind;
use tasq_core::worktree::provision;
use tasq_core::worktree::vars::Vars;

use crate::Repo;

/// The worktree settings of the repository a task works in: `[worktree]`
/// with the repository config of its main checkout applied, wherever tasq
/// runs (ADR-0031). Loading them is the caller's, which owns the
/// configuration layers.
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
    /// The repository config that applied, when one did.
    pub repo_file: Option<PathBuf>,
    /// The hook kinds that file sets: they run only once approved.
    pub untrusted: Vec<Kind>,
}

impl RepoSettings {
    /// The repository name for `{{repo}}`, when known.
    pub fn name(&self) -> Option<&str> {
        self.repo.as_ref().and_then(|r| r.name.as_deref())
    }

    /// The template variables of `task`'s worktree at `path` on `branch`.
    pub fn vars(&self, task: &Task, branch: &str, path: &Path) -> Vars {
        Vars::for_task(task, &self.main, self.name(), &self.config.variables)
            .with_branch(branch, self.config.ports)
            .with_worktree(path)
    }

    /// The paths provisioning writes into the worktree at `path`, relative
    /// to it: they can be made again, so they do not make it dirty. None
    /// when provisioning cannot be planned.
    pub fn provisioned(&self, path: &Path) -> Vec<PathBuf> {
        provision::plan(&self.config, &self.main, path, &self.templates_dir)
            .unwrap_or_default()
            .iter()
            .filter_map(|step| step.target().strip_prefix(path).ok().map(Path::to_path_buf))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::config::PortsConfig;
    use tasq_core::model::TaskId;

    fn settings(name: Option<&str>) -> RepoSettings {
        let mut config = WorktreeConfig::default();
        config.variables.insert("db".into(), "shop".into());
        config.ports = Some(PortsConfig {
            base: 3000,
            span: 100,
            block: 10,
        });
        RepoSettings {
            project: "/ws/app/sub".into(),
            repo: Some(Repo {
                main: "/ws/app".into(),
                name: name.map(str::to_owned),
            }),
            main: "/ws/app".into(),
            config,
            templates_dir: "/ws/app".into(),
            repo_file: None,
            untrusted: Vec::new(),
        }
    }

    #[test]
    fn the_name_is_the_repository_s() {
        assert_eq!(settings(Some("shop")).name(), Some("shop"));
        assert_eq!(settings(None).name(), None);
        let outside = RepoSettings {
            repo: None,
            ..settings(Some("shop"))
        };
        assert_eq!(outside.name(), None);
    }

    #[test]
    fn the_vars_are_the_task_s_in_the_main_checkout() {
        let settings = settings(Some("shop"));
        let task = Task::new(TaskId::from(7), "T");
        let path = Path::new("/ws/app-feat_x");
        let map = settings.vars(&task, "feat/x", path);
        let map = map.map();
        assert_eq!(map.get("project").map(String::as_str), Some("/ws/app"));
        assert_eq!(map.get("repo").map(String::as_str), Some("shop"));
        assert_eq!(map.get("db").map(String::as_str), Some("shop"));
        assert_eq!(map.get("branch").map(String::as_str), Some("feat/x"));
        assert_eq!(
            map.get("worktree").map(String::as_str),
            Some("/ws/app-feat_x")
        );
        assert!(map.contains_key("port"));
    }

    #[test]
    fn provisioned_paths_are_relative_to_the_worktree() {
        let mut settings = settings(Some("shop"));
        settings.config.copies = vec![".env".into()];
        settings.config.symlinks = vec!["node_modules".into()];
        let path = Path::new("/ws/app-feat_x");
        assert_eq!(
            settings.provisioned(path),
            vec![PathBuf::from(".env"), PathBuf::from("node_modules")]
        );
        settings.config.copies = vec!["../outside".into()];
        assert_eq!(settings.provisioned(path), Vec::<PathBuf>::new());
    }
}
