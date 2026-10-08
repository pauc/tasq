//! Declaring a worktree (plan 4.2, step 1): the `worktree.branch` and
//! `worktree.path` templates rendered for a task and stored as its
//! `## Worktrees` entry. Nothing runs and nothing is created on disk.

use std::path::Path;

use thiserror::Error;

use crate::config::{OnCreate, WorktreeConfig, expand_tilde};
use crate::model::Task;
use crate::template::TemplateError;

use super::lifecycle::{self, Declared, LifecycleError};
use super::vars::Vars;

/// Why a worktree cannot be declared.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeclareError {
    /// The branch is not usable.
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    /// `worktree.branch` does not render.
    #[error("worktree.branch: {0}")]
    Branch(TemplateError),
    /// `worktree.path` does not render.
    #[error("worktree.path: {0}")]
    Path(TemplateError),
}

/// Whether `tasq create` declares a worktree: always when `--worktree` was
/// given (`asked`), and with `on_create = "auto"` when the project is inside
/// a git work tree.
pub fn wanted(on_create: OnCreate, asked: bool, in_work_tree: bool) -> bool {
    asked || (on_create == OnCreate::Auto && in_work_tree)
}

/// Declares a worktree for `task` in `project`: on `branch`, or the
/// rendered `worktree.branch` without one, at the rendered `worktree.path`.
/// A leading `~` in the path is `home`; a relative path is taken from
/// `project`. `repo` is the repository name for `{{repo}}`.
///
/// # Errors
///
/// [`DeclareError`] when a template does not render or the branch is
/// invalid ([`lifecycle::declare`]).
pub fn declare(
    task: &Task,
    branch: Option<&str>,
    config: &WorktreeConfig,
    project: &Path,
    repo: Option<&str>,
    home: Option<&Path>,
) -> Result<Declared, DeclareError> {
    let vars = Vars::for_task(task, project, repo, &config.variables);
    let branch = match branch {
        Some(branch) => branch.to_owned(),
        None => vars.render(&config.branch).map_err(DeclareError::Branch)?,
    };
    lifecycle::declare(task, &branch, |branch| {
        let path = vars
            .clone()
            .with_branch(branch, config.ports)
            .render(&config.path)
            .map_err(DeclareError::Path)?;
        Ok(project.join(expand_tilde(Path::new(&path), home)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PortsConfig;
    use crate::model::{TaskId, Worktree};

    fn task() -> Task {
        Task::new(TaskId::from(7), "Fix the login form")
    }

    fn new(path: &str, branch: &str) -> Declared {
        Declared::New(Worktree::on_branch(path, branch))
    }

    const PROJECT: &str = "/code/app";

    #[test]
    fn declares_with_the_default_templates() {
        let config = WorktreeConfig::default();
        let d = |branch| declare(&task(), branch, &config, Path::new(PROJECT), None, None);
        assert_eq!(
            d(None),
            Ok(new(
                "/code/app-7_fix_the_login_form",
                "7-fix-the-login-form"
            ))
        );
        assert_eq!(d(Some(" feat/x ")), Ok(new("/code/app-feat_x", "feat/x")));
    }

    #[test]
    fn renders_repo_ports_and_extra_variables() {
        let config = WorktreeConfig {
            branch: "{{who}}/{{id}}".into(),
            path: "~/wt/{{repo}}/{{branch_safe}}-{{port}}".into(),
            ports: Some(PortsConfig {
                base: 3000,
                span: 1,
                block: 1,
            }),
            variables: [("who".to_owned(), "pau".to_owned())].into(),
            ..WorktreeConfig::default()
        };
        assert_eq!(
            declare(
                &task(),
                None,
                &config,
                Path::new(PROJECT),
                Some("tasq"),
                Some(Path::new("/home/me"))
            ),
            Ok(new("/home/me/wt/tasq/pau_7-3000", "pau/7"))
        );
    }

    #[test]
    fn a_relative_path_is_inside_the_project() {
        let config = WorktreeConfig {
            path: ".worktrees/{{branch}}".into(),
            ..WorktreeConfig::default()
        };
        assert_eq!(
            declare(&task(), Some("b"), &config, Path::new(PROJECT), None, None),
            Ok(new("/code/app/.worktrees/b", "b"))
        );
    }

    #[test]
    fn template_and_branch_errors() {
        let bad = |branch: &str, path: &str| WorktreeConfig {
            branch: branch.into(),
            path: path.into(),
            ..WorktreeConfig::default()
        };
        let d = |config: &WorktreeConfig, branch| {
            declare(&task(), branch, config, Path::new(PROJECT), None, None)
        };
        assert_eq!(
            d(&bad("{{nope}}", "{{branch}}"), None),
            Err(DeclareError::Branch(TemplateError::UnknownPlaceholder(
                "nope".into()
            )))
        );
        assert_eq!(
            d(&bad("{{nope}}", "{{branch}}"), Some("given")),
            Ok(new("/code/app/given", "given")),
            "a given branch skips the branch template"
        );
        assert_eq!(
            d(&bad("x", "{{nope}}"), None),
            Err(DeclareError::Path(TemplateError::UnknownPlaceholder(
                "nope".into()
            )))
        );
        assert_eq!(
            d(&bad("a b", "{{nope}}"), None),
            Err(DeclareError::Lifecycle(LifecycleError::InvalidBranch(
                "a b".into()
            ))),
            "the branch is checked before the path renders"
        );
        assert_eq!(
            DeclareError::Path(TemplateError::UnknownPlaceholder("x".into())).to_string(),
            "worktree.path: unknown placeholder {{x}}"
        );
        assert_eq!(
            DeclareError::Branch(TemplateError::UnknownPlaceholder("x".into())).to_string(),
            "worktree.branch: unknown placeholder {{x}}"
        );
    }

    #[test]
    fn an_existing_declaration_is_kept() {
        let mut t = task();
        t.add_worktree(Worktree::on_branch("/elsewhere", "b"));
        assert_eq!(
            declare(
                &t,
                Some("b"),
                &WorktreeConfig::default(),
                Path::new(PROJECT),
                None,
                None
            ),
            Ok(Declared::Already(Worktree::on_branch("/elsewhere", "b")))
        );
    }

    #[test]
    fn wanted_on_create() {
        for (on_create, asked, git, want) in [
            (OnCreate::Auto, false, true, true),
            (OnCreate::Auto, false, false, false),
            (OnCreate::Auto, true, false, true),
            (OnCreate::Never, false, true, false),
            (OnCreate::Never, true, false, true),
        ] {
            assert_eq!(
                wanted(on_create, asked, git),
                want,
                "{on_create:?} {asked} {git}"
            );
        }
    }
}
