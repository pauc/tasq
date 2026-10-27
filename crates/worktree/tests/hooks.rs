//! Preparing and running worktree hooks.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::collections::BTreeMap;
use std::path::Path;

use tasq_core::config::{WorktreeConfig, WorktreeHooks};
use tasq_core::model::{Task, TaskId};
use tasq_core::worktree::hooks::{Command, HookEvent, Hooks, Kind};
use tasq_core::worktree::vars::Vars;
use tasq_worktree::RepoSettings;
use tasq_worktree::hooks::{HookFailure, prepare, run_all};

/// The settings of a main checkout at `/ws` with `pre-create` and
/// `post-create` hooks.
fn settings() -> RepoSettings {
    let mut config = WorktreeConfig::default();
    config.hooks = WorktreeHooks {
        pre_create: vec!["echo same".into()],
        post_create: vec!["echo same".into(), "echo '{{branch}}'".into()],
        ..WorktreeHooks::default()
    };
    RepoSettings {
        project: "/ws".into(),
        repo: None,
        main: "/ws".into(),
        config,
        templates_dir: "/ws".into(),
    }
}

fn vars() -> Vars {
    Vars::for_task(
        &Task::new(TaskId::from(7), "T"),
        Path::new("/ws"),
        None,
        &BTreeMap::new(),
    )
    .with_branch("feat/x", None)
}

fn command(line: &str, argv: &[&str]) -> Command {
    Command {
        line: line.to_owned(),
        argv: argv.iter().map(|&w| w.to_owned()).collect(),
    }
}

#[test]
fn every_line_of_the_kinds_asked_for_is_rendered() {
    let hooks = prepare(&settings(), [Kind::PreCreate, Kind::PostCreate], &vars()).unwrap();
    assert_eq!(
        hooks,
        [
            Hooks {
                kind: Kind::PreCreate,
                run: vec![command("echo same", &["echo", "same"])],
            },
            Hooks {
                kind: Kind::PostCreate,
                run: vec![
                    command("echo same", &["echo", "same"]),
                    command("echo '{{branch}}'", &["echo", "feat/x"]),
                ],
            }
        ]
    );
    let [remove] = prepare(&settings(), [Kind::PreRemove], &vars()).unwrap();
    assert_eq!(remove.run, Vec::new());
}

#[test]
fn the_first_line_that_does_not_render_is_the_error() {
    let mut settings = settings();
    settings.config.hooks.pre_create = vec!["echo {{nope}}".into()];
    settings.config.hooks.post_create = vec!["echo {{other}}".into()];
    let hooks = prepare(&settings, [Kind::PreCreate, Kind::PostCreate], &vars());
    assert_eq!(
        hooks.unwrap_err().to_string(),
        "worktree.hooks.pre-create \"echo {{nope}}\": unknown placeholder {{nope}}"
    );
    let hooks = prepare(&settings, [Kind::PostCreate, Kind::PreCreate], &vars());
    assert_eq!(
        hooks.unwrap_err().to_string(),
        "worktree.hooks.post-create \"echo {{other}}\": unknown placeholder {{other}}"
    );
}

fn sh(script: &str) -> Command {
    Command {
        line: script.to_owned(),
        argv: vec!["sh".into(), "-c".into(), script.into()],
    }
}

fn run(kind: Kind, scripts: &[&str]) -> (Result<(), HookFailure>, Vec<HookEvent>) {
    let dir = tempfile::tempdir().unwrap();
    let env = vec![
        ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ("X".to_owned(), "y".to_owned()),
    ];
    let hooks = Hooks {
        kind,
        run: scripts.iter().map(|s| sh(s)).collect(),
    };
    let mut events = Vec::new();
    let result = run_all(&hooks, dir.path(), &env, &mut |e| events.push(e));
    (result, events)
}

#[test]
fn a_failing_blocking_hook_stops_the_rest() {
    let (result, events) = run(
        Kind::PreCreate,
        &["echo $X", "true", "echo no >&2; exit 3", "echo after"],
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "pre-create hook \"echo no >&2; exit 3\" failed: exit status 3: no"
    );
    assert_eq!(
        events,
        [HookEvent::Output("pre-create hook \"echo $X\": y".into())]
    );
}

#[test]
fn a_failing_other_hook_warns_and_the_rest_run() {
    let (result, events) = run(Kind::PostRemove, &["exit 1", "echo after"]);
    assert!(result.is_ok());
    assert_eq!(
        events,
        [
            HookEvent::Warning("post-remove hook \"exit 1\" failed: exit status 1".into()),
            HookEvent::Output("post-remove hook \"echo after\": after".into())
        ]
    );
}
