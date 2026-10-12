//! Preparing, approving and running worktree hooks against a temporary
//! trust file, with the question answered by a stub.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use tasq_core::config::{WorktreeConfig, WorktreeHooks};
use tasq_core::model::{Task, TaskId};
use tasq_core::worktree::hooks::{Command, HookEvent, Hooks, Kind, TrustStore};
use tasq_core::worktree::vars::Vars;
use tasq_worktree::RepoSettings;
use tasq_worktree::hooks::{Approval, Approver, HookFailure, prepare, run_all, trust};
use tempfile::TempDir;

thread_local! {
    static ASKED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[expect(clippy::unnecessary_wraps, reason = "the signature of Approver::ask")]
fn yes(question: &str) -> io::Result<bool> {
    ASKED.with(|asked| asked.borrow_mut().push(question.to_owned()));
    Ok(true)
}

#[expect(clippy::unnecessary_wraps, reason = "the signature of Approver::ask")]
fn no(question: &str) -> io::Result<bool> {
    ASKED.with(|asked| asked.borrow_mut().push(question.to_owned()));
    Ok(false)
}

fn never(question: &str) -> io::Result<bool> {
    panic!("asked {question:?}")
}

fn closed(_: &str) -> io::Result<bool> {
    Err(io::Error::other("stdin closed"))
}

fn asked() -> Vec<String> {
    ASKED.with(|asked| asked.borrow().clone())
}

/// A main checkout at `<root>/ws` whose `.tasq.toml` sets the
/// `post-create` hooks; `pre-create` comes from the global file.
struct Repo {
    _dir: TempDir,
    root: PathBuf,
    settings: RepoSettings,
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let main = root.join("ws");
        let hooks = WorktreeHooks {
            pre_create: vec!["echo same".into()],
            post_create: vec!["echo same".into(), "echo '{{branch}}'".into()],
            ..WorktreeHooks::default()
        };
        let mut config = WorktreeConfig::default();
        config.hooks = hooks;
        let settings = RepoSettings {
            project: main.clone(),
            repo: None,
            main: main.clone(),
            config,
            templates_dir: main.clone(),
            repo_file: Some(main.join(".tasq.toml")),
            untrusted: vec![Kind::PostCreate],
        };
        Self {
            _dir: dir,
            root,
            settings,
        }
    }

    fn trust_file(&self) -> PathBuf {
        self.root.join("state").join("tasq").join("trust.toml")
    }

    fn approver(&self, approval: Approval, ask: fn(&str) -> io::Result<bool>) -> Approver {
        Approver {
            approval,
            trust_file: Some(self.trust_file()),
            ask,
        }
    }

    fn prepare(
        &self,
        approver: &Approver,
        dry_run: bool,
    ) -> (Result<[Hooks; 2], HookFailure>, Vec<HookEvent>) {
        let mut events = Vec::new();
        let hooks = prepare(
            &self.settings,
            [Kind::PreCreate, Kind::PostCreate],
            &vars(),
            approver,
            dry_run,
            &mut |e| events.push(e),
        );
        (hooks, events)
    }

    fn stored(&self) -> Option<TrustStore> {
        let text = std::fs::read_to_string(self.trust_file()).ok()?;
        Some(TrustStore::parse(&text).unwrap())
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

fn pre_runs() -> Hooks {
    Hooks {
        kind: Kind::PreCreate,
        run: vec![command("echo same", &["echo", "same"])],
        skipped: Vec::new(),
    }
}

fn post_runs() -> Hooks {
    Hooks {
        kind: Kind::PostCreate,
        run: vec![
            command("echo same", &["echo", "same"]),
            command("echo '{{branch}}'", &["echo", "feat/x"]),
        ],
        skipped: Vec::new(),
    }
}

fn post_skipped() -> Hooks {
    Hooks {
        kind: Kind::PostCreate,
        run: Vec::new(),
        skipped: vec!["echo same".into(), "echo '{{branch}}'".into()],
    }
}

const WARNING: &str = "are not approved and were skipped: \"echo same\", \"echo '{{branch}}'\"; \
                       approve them with tasq worktree trust";

fn not_approved(r: &Repo) -> HookEvent {
    HookEvent::Warning(format!(
        "hooks from {}/ws/.tasq.toml {WARNING} {}/ws, or pass --yes",
        r.root.display(),
        r.root.display()
    ))
}

#[test]
fn approval_from_yes() {
    assert_eq!(Approval::from_yes(true), Approval::Given);
    assert_eq!(Approval::from_yes(false), Approval::Ask);
}

#[test]
fn without_a_repository_file_everything_runs_unasked() {
    let mut r = Repo::new();
    r.settings.repo_file = None;
    let approver = Approver {
        approval: Approval::Skip,
        trust_file: None,
        ask: never,
    };
    let (hooks, events) = r.prepare(&approver, false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_runs()]);
    assert_eq!(events, Vec::new());
}

#[test]
fn given_runs_the_repository_hooks_without_storing_them() {
    let r = Repo::new();
    let (hooks, events) = r.prepare(&r.approver(Approval::Given, never), false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_runs()]);
    assert_eq!(events, Vec::new());
    assert_eq!(r.stored(), None);
}

#[test]
fn skip_leaves_out_only_the_repository_lines_with_a_warning() {
    let r = Repo::new();
    let (hooks, events) = r.prepare(&r.approver(Approval::Skip, never), false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_skipped()]);
    assert_eq!(events, [not_approved(&r)]);

    let (hooks, events) = r.prepare(&r.approver(Approval::Skip, never), true);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_skipped()]);
    assert_eq!(events, Vec::new(), "a dry run does not warn");
}

#[test]
fn a_line_approved_before_runs_next_to_one_skipped() {
    let r = Repo::new();
    let mut store = TrustStore::default();
    store.approve(
        r.settings.repo_file.as_deref().unwrap(),
        &["echo same".to_owned()],
    );
    std::fs::create_dir_all(r.trust_file().parent().unwrap()).unwrap();
    std::fs::write(r.trust_file(), store.to_toml()).unwrap();
    let (hooks, _) = r.prepare(&r.approver(Approval::Skip, never), false);
    assert_eq!(
        hooks.unwrap()[1],
        Hooks {
            kind: Kind::PostCreate,
            run: vec![command("echo same", &["echo", "same"])],
            skipped: vec!["echo '{{branch}}'".into()],
        }
    );
}

#[test]
fn a_yes_runs_and_stores_every_line_of_the_file() {
    let r = Repo::new();
    let (hooks, events) = r.prepare(&r.approver(Approval::Ask, yes), false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_runs()]);
    assert_eq!(events, Vec::new());
    assert_eq!(
        asked(),
        [format!(
            "{}/ws/.tasq.toml wants to run these hooks:\n  post-create: echo same\n  \
             post-create: echo '{{{{branch}}}}'\nRun them, now and from now on? [y/N] ",
            r.root.display()
        )]
    );
    let file = r.settings.repo_file.as_deref().unwrap();
    let lines = &r.settings.config.hooks.post_create;
    assert_eq!(r.stored().unwrap().pending(file, lines), Vec::<&str>::new());

    let (hooks, _) = r.prepare(&r.approver(Approval::Ask, never), false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_runs()], "asked once");
}

#[test]
fn a_no_skips_with_a_warning_and_stores_nothing() {
    let r = Repo::new();
    let (hooks, events) = r.prepare(&r.approver(Approval::Ask, no), false);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_skipped()]);
    assert_eq!(events, [not_approved(&r)]);
    assert_eq!(asked().len(), 1);
    assert_eq!(r.stored(), None);
}

#[test]
fn a_dry_run_never_asks() {
    let r = Repo::new();
    let (hooks, events) = r.prepare(&r.approver(Approval::Ask, never), true);
    assert_eq!(hooks.unwrap(), [pre_runs(), post_skipped()]);
    assert_eq!(events, Vec::new());
}

#[test]
fn a_question_that_cannot_be_asked_is_the_error() {
    let r = Repo::new();
    let (hooks, _) = r.prepare(&r.approver(Approval::Ask, closed), false);
    assert!(
        matches!(&hooks, Err(HookFailure::Ask(e)) if e.to_string() == "stdin closed"),
        "{hooks:?}"
    );
}

#[test]
fn trust_file_failures() {
    let r = Repo::new();
    let approver = Approver {
        approval: Approval::Given,
        trust_file: None,
        ask: never,
    };
    let (hooks, _) = r.prepare(&approver, false);
    assert_eq!(
        hooks.unwrap_err().to_string(),
        "cannot tell where to keep hook approvals: HOME is not set"
    );

    std::fs::create_dir_all(r.trust_file().parent().unwrap()).unwrap();
    std::fs::write(r.trust_file(), "other = 1\n").unwrap();
    let (hooks, _) = r.prepare(&r.approver(Approval::Given, never), false);
    let message = hooks.unwrap_err().to_string();
    assert!(
        message.starts_with(&format!("{}: ", r.trust_file().display()))
            && message.contains("unknown field"),
        "{message}"
    );

    std::fs::remove_file(r.trust_file()).unwrap();
    std::fs::create_dir(r.trust_file()).unwrap();
    let (hooks, _) = r.prepare(&r.approver(Approval::Given, never), false);
    assert!(
        matches!(&hooks, Err(HookFailure::TrustFile { path, .. }) if *path == r.trust_file()),
        "{hooks:?}"
    );
}

#[test]
fn the_first_line_that_does_not_render_is_the_error() {
    let mut r = Repo::new();
    r.settings.config.hooks.pre_create = vec!["echo {{nope}}".into()];
    r.settings.config.hooks.post_create = vec!["echo {{other}}".into()];
    r.settings.repo_file = None;
    let (hooks, _) = r.prepare(&r.approver(Approval::Given, never), false);
    assert_eq!(
        hooks.unwrap_err().to_string(),
        "worktree.hooks.pre-create \"echo {{nope}}\": unknown placeholder {{nope}}"
    );
}

#[test]
fn trust_stores_the_lines_of_the_untrusted_kinds() {
    let r = Repo::new();
    let file = r.settings.repo_file.clone().unwrap();
    assert_eq!(
        trust(&r.settings, Some(&r.trust_file())).unwrap(),
        [
            (Kind::PostCreate, "echo same".to_owned()),
            (Kind::PostCreate, "echo '{{branch}}'".to_owned())
        ]
    );
    let mut expected = TrustStore::default();
    expected.approve(&file, &r.settings.config.hooks.post_create);
    assert_eq!(r.stored(), Some(expected), "only the file's own kinds");
}

#[test]
fn trust_without_lines_or_a_file_writes_nothing() {
    let mut r = Repo::new();
    r.settings.untrusted = Vec::new();
    assert_eq!(
        trust(&r.settings, Some(&r.trust_file())).unwrap(),
        Vec::new()
    );
    assert_eq!(r.stored(), None);
    r.settings.repo_file = None;
    r.settings.untrusted = vec![Kind::PostCreate];
    assert_eq!(trust(&r.settings, None).unwrap(), Vec::new());
}

#[test]
fn trust_reports_a_trust_file_it_cannot_write() {
    let r = Repo::new();
    std::fs::write(r.root.join("state"), "a file").unwrap();
    let failed = trust(&r.settings, Some(&r.trust_file())).unwrap_err();
    assert!(
        matches!(&failed, HookFailure::TrustFile { path, .. } if *path == r.trust_file()),
        "{failed:?}"
    );
    std::fs::remove_file(r.root.join("state")).unwrap();
    std::fs::create_dir_all(r.trust_file()).unwrap();
    let failed = trust(&r.settings, Some(&r.trust_file())).unwrap_err();
    assert!(
        matches!(&failed, HookFailure::TrustFile { path, .. } if *path == r.trust_file()),
        "{failed:?}"
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
        skipped: vec!["never runs".into()],
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
