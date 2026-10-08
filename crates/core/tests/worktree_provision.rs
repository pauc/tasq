//! The provisioning plans of three workspace layouts: the declared path
//! and the steps, from a configuration file, with no git and no disk.

use std::path::{Path, PathBuf};

use tasq_core::config::Config;
use tasq_core::model::{Task, TaskId};
use tasq_core::worktree::declaration::declare;
use tasq_core::worktree::lifecycle::Declared;
use tasq_core::worktree::provision::{Step, plan};

/// The workspace directory holding the configuration file and the
/// templates.
const WORKSPACE: &str = "/ws";
const MAIN: &str = "/ws/shop";

fn fixture(name: &str) -> Config {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/worktree")
        .join(name);
    toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap()
}

/// The declared path for task 12 on `feature/Login-2`, and its plan.
fn declared_plan(name: &str) -> (PathBuf, Vec<Step>) {
    let config = fixture(name).worktree;
    let task = Task::new(TaskId::from(12), "Fix the login form");
    let Ok(Declared::New(worktree)) = declare(
        &task,
        Some("feature/Login-2"),
        &config,
        Path::new(MAIN),
        Some("shop"),
        None,
    ) else {
        panic!("{name}: not declared");
    };
    let steps = plan(
        &config,
        Path::new(MAIN),
        &worktree.path,
        Path::new(WORKSPACE),
    )
    .unwrap();
    (worktree.path, steps)
}

fn link(rel: &str, wt: &str) -> Step {
    Step::Symlink {
        source: Path::new(MAIN).join(rel),
        target: Path::new(wt).join(rel),
    }
}

fn render(source: &str, target: &str, wt: &str) -> Step {
    Step::Render {
        source: Path::new(WORKSPACE).join(source),
        target: Path::new(wt).join(target),
    }
}

#[test]
fn sibling_checkouts_link_dependencies_and_render_the_envrc() {
    let wt = "/ws/feature_Login_2";
    assert_eq!(
        declared_plan("sibling.toml"),
        (
            PathBuf::from(wt),
            vec![
                link("node_modules", wt),
                link("CLAUDE.local.md", wt),
                link("config/master.key", wt),
                render("templates/.envrc.template", ".envrc", wt),
            ]
        )
    );
}

#[test]
fn per_branch_databases_render_two_templates() {
    let wt = "/ws/shop-feature_Login_2";
    assert_eq!(
        declared_plan("databases.toml"),
        (
            PathBuf::from(wt),
            vec![
                link("CLAUDE.local.md", wt),
                render("templates/.envrc.template", ".envrc", wt),
                render("templates/database.yml.template", "config/database.yml", wt),
            ]
        )
    );
}

#[test]
fn an_ignored_envrc_is_copied() {
    let wt = "/ws/shop-feature_Login_2";
    assert_eq!(
        declared_plan("ignored-envrc.toml"),
        (
            PathBuf::from(wt),
            vec![Step::Copy {
                source: "/ws/shop/.envrc".into(),
                target: "/ws/shop-feature_Login_2/.envrc".into(),
            }]
        )
    );
}
