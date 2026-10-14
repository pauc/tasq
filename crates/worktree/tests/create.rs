//! `materialize` against a temporary repository: what it creates,
//! provisions and runs, what a dry run leaves alone, and where it stops.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::path::PathBuf;
use std::process::Command;

use tasq_core::config::{TemplateFile, WorktreeConfig};
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::worktree::hooks::HookEvent;
use tasq_worktree::create::{CreateError, Created, materialize};
use tasq_worktree::hooks::{Approval, Approver, HookFailure};
use tasq_worktree::{Git, Repo, RepoSettings};
use tempfile::TempDir;

/// A main checkout at `<root>/ws/app` with one commit, an untracked
/// `.env`, a `node_modules` directory and a template; hooks append to
/// `<root>/log`.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    main: PathBuf,
    git: Git,
    settings: RepoSettings,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = t\n\temail = t@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let git = Git {
            env: vec![
                ("HOME".to_owned(), home.display().to_string()),
                ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
            ],
        };
        let main = root.join("ws").join("app");
        std::fs::create_dir_all(main.join("node_modules")).unwrap();
        std::fs::create_dir_all(main.join("tpl")).unwrap();
        std::fs::write(main.join(".gitignore"), ".env\nnode_modules/\n").unwrap();
        std::fs::write(main.join(".env"), "SECRET=1\n").unwrap();
        std::fs::write(main.join("tpl/db.yml"), "database: {{branch_safe}}\n").unwrap();
        let mut config = WorktreeConfig::default();
        config.copies = vec![".env".into(), "missing".into()];
        config.symlinks = vec!["node_modules".into()];
        config.templates = vec![TemplateFile {
            source: "tpl/db.yml".into(),
            target: "config/db.yml".into(),
        }];
        config
            .variables
            .insert("out".into(), root.display().to_string());
        config.hooks.pre_create = vec!["sh -c 'echo pre $TASQ_BRANCH >> {{out}}/log'".into()];
        config.hooks.post_create = vec![
            "sh -c 'echo post $PWD >> {{out}}/log'".into(),
            "echo done".into(),
        ];
        let settings = RepoSettings {
            project: main.clone(),
            repo: None,
            main: main.clone(),
            config,
            templates_dir: main.clone(),
            repo_file: None,
            untrusted: Vec::new(),
        };
        let mut project = Self {
            _dir: dir,
            root,
            main,
            git,
            settings,
        };
        project.sh(&["init", "-q"]);
        project.sh(&["add", "."]);
        project.sh(&["commit", "-q", "-m", "init"]);
        project.settings.repo = Repo::of(&project.main);
        project
    }

    fn sh(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.main)
            .env_clear()
            .envs(self.git.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn has_ref(&self, reference: &str) -> bool {
        Command::new("git")
            .args(["show-ref", "--verify", "--quiet", reference])
            .current_dir(&self.main)
            .env_clear()
            .envs(self.git.env.iter().map(|(k, v)| (k, v)))
            .status()
            .unwrap()
            .success()
    }

    fn wt(&self, name: &str) -> PathBuf {
        self.root.join("ws").join(name)
    }

    fn task(worktrees: Vec<Worktree>) -> Task {
        Task {
            worktrees,
            ..Task::new(TaskId::from(7), "T")
        }
    }

    fn on_branch(&self, branch: &str) -> Task {
        Self::task(vec![Worktree::on_branch(self.wt("app-wt"), branch)])
    }

    fn materialize(
        &self,
        task: &Task,
        from: Option<&str>,
        dry_run: bool,
    ) -> (Result<Option<Created>, CreateError>, Vec<HookEvent>) {
        let mut events = Vec::new();
        let created = materialize(
            &self.git,
            task,
            Some(&self.settings),
            from,
            &approver(),
            dry_run,
            &mut |e| events.push(e),
        );
        (created, events)
    }

    fn log(&self) -> Option<String> {
        std::fs::read_to_string(self.root.join("log")).ok()
    }
}

fn never(question: &str) -> std::io::Result<bool> {
    panic!("asked {question:?}")
}

fn approver() -> Approver {
    Approver {
        approval: Approval::Ask,
        trust_file: None,
        ask: never,
    }
}

fn fresh(branch: &str, base: &str) -> HookEvent {
    HookEvent::Warning(format!(
        "branch {branch} is gone or new: neither the repository nor origin has it; \
         it starts fresh from {base}"
    ))
}

#[test]
fn nothing_to_create() {
    let p = Project::new();
    let none = |task: &Task, settings: Option<&RepoSettings>| {
        let mut events = Vec::new();
        let created = materialize(&p.git, task, settings, None, &approver(), false, &mut |e| {
            events.push(e);
        });
        assert_eq!(created.unwrap(), None);
        assert_eq!(events, Vec::new());
    };
    none(&Project::task(Vec::new()), Some(&p.settings));
    none(&Project::task(Vec::new()), None);
    none(
        &Project::task(vec![Worktree {
            path: p.wt("by-hand"),
            branch: None,
        }]),
        None,
    );
    let existing = Project::task(vec![Worktree::on_branch(p.main.clone(), "main")]);
    none(&existing, Some(&p.settings));
    none(&existing, None);
}

#[test]
fn a_worktree_to_create_without_a_project_is_an_error() {
    let p = Project::new();
    let mut events = Vec::new();
    let created = materialize(
        &p.git,
        &p.on_branch("feat/x"),
        None,
        None,
        &approver(),
        false,
        &mut |e| events.push(e),
    );
    assert!(
        matches!(created, Err(CreateError::NoProject)),
        "{created:?}"
    );
}

#[test]
fn a_dry_run_describes_and_creates_nothing() {
    let p = Project::new();
    let path = p.wt("app-wt");
    let (created, events) = p.materialize(&p.on_branch("feat/x"), None, true);
    let main = p.main.display();
    let root = p.root.display();
    assert_eq!(
        created.unwrap(),
        Some(Created {
            path: path.clone(),
            line: format!("{} on feat/x (new from HEAD)", path.display()),
            steps: vec![
                format!("run pre-create hook: sh -c 'echo pre $TASQ_BRANCH >> {root}/log'"),
                format!("copy .env from {main}/.env"),
                format!("copy missing from {main}/missing"),
                format!("link node_modules -> {main}/node_modules"),
                format!("render config/db.yml from {main}/tpl/db.yml"),
                format!("run post-create hook: sh -c 'echo post $PWD >> {root}/log'"),
                "run post-create hook: echo done".to_owned(),
            ],
        })
    );
    assert_eq!(
        events,
        [
            HookEvent::Warning(
                "origin/HEAD is not set; feat/x starts from HEAD (set worktree.base, \
                 or run git remote set-head origin --auto)"
                    .into()
            ),
            fresh("feat/x", "HEAD")
        ]
    );
    assert!(!path.exists());
    assert_eq!(p.log(), None);
    assert!(!p.has_ref("refs/heads/feat/x"));
}

#[test]
fn creates_provisions_and_runs_the_hooks_around_it() {
    let p = Project::new();
    let path = p.wt("app-wt");
    let (created, events) = p.materialize(&p.on_branch("feat/x"), Some("main"), false);
    let created = created.unwrap().unwrap();
    assert_eq!(created.path, path);
    assert_eq!(
        created.line,
        format!("{} on feat/x (new from main)", path.display())
    );
    assert_eq!(created.steps.len(), 7);
    assert_eq!(
        events,
        [
            fresh("feat/x", "main"),
            HookEvent::Warning(format!(
                "copy: {}/missing not found, skipped",
                p.main.display()
            )),
            HookEvent::Output("post-create hook \"echo done\": done".into()),
        ]
    );
    assert_eq!(p.git.current_branch(&path).as_deref(), Some("feat/x"));
    assert_eq!(
        std::fs::read_to_string(path.join(".env")).unwrap(),
        "SECRET=1\n"
    );
    assert_eq!(
        std::fs::read_link(path.join("node_modules")).unwrap(),
        p.main.join("node_modules")
    );
    assert_eq!(
        std::fs::read_to_string(path.join("config/db.yml")).unwrap(),
        "database: feat_x\n"
    );
    assert_eq!(
        p.log().unwrap(),
        format!("pre feat/x\npost {}\n", path.display())
    );
}

#[test]
fn a_branch_on_origin_is_fetched_only_for_real() {
    let p = Project::new();
    let origin = p.root.join("origin.git");
    p.sh(&["init", "-q", "--bare", origin.to_str().unwrap()]);
    p.sh(&["remote", "add", "origin", origin.to_str().unwrap()]);
    p.sh(&["push", "-q", "origin", "main"]);
    p.sh(&["checkout", "-q", "-b", "feat/r"]);
    p.sh(&["push", "-q", "origin", "feat/r"]);
    p.sh(&["checkout", "-q", "main"]);
    p.sh(&["branch", "-q", "-D", "feat/r"]);
    p.sh(&["update-ref", "-d", "refs/remotes/origin/feat/r"]);
    let task = p.on_branch("feat/r");
    let path = p.wt("app-wt");
    let line = format!("{} on feat/r (tracking origin/feat/r)", path.display());

    let (created, events) = p.materialize(&task, None, true);
    assert_eq!(created.unwrap().unwrap().line, line);
    assert_eq!(events, Vec::new());
    assert!(!p.has_ref("refs/remotes/origin/feat/r"), "not fetched");

    let (created, _) = p.materialize(&task, None, false);
    assert_eq!(created.unwrap().unwrap().line, line);
    assert!(p.has_ref("refs/remotes/origin/feat/r"), "fetched");
    assert_eq!(p.git.current_branch(&path).as_deref(), Some("feat/r"));
}

#[test]
fn a_broken_template_stops_everything() {
    let mut p = Project::new();
    std::fs::write(p.main.join("tpl/db.yml"), "{{nope}}").unwrap();
    p.settings.config.hooks.pre_create = vec!["sh -c 'echo pre >> {{out}}/log'".into()];
    let (created, _) = p.materialize(&p.on_branch("feat/x"), Some("main"), false);
    let error = created.unwrap_err();
    assert!(matches!(error, CreateError::Template(_)), "{error:?}");
    assert!(
        error
            .to_string()
            .starts_with(&format!("template {}/tpl/db.yml: ", p.main.display())),
        "{error}"
    );
    assert!(!p.wt("app-wt").exists());
    assert_eq!(p.log(), None, "no hook ran");
}

#[test]
fn a_copy_outside_the_worktree_is_an_error() {
    let mut p = Project::new();
    p.settings.config.copies = vec!["../elsewhere".into()];
    let (created, _) = p.materialize(&p.on_branch("feat/x"), Some("main"), true);
    let error = created.unwrap_err();
    assert!(matches!(error, CreateError::Outside(_)), "{error:?}");
}

#[test]
fn a_failing_pre_create_hook_stops_before_git() {
    let mut p = Project::new();
    p.settings.config.hooks.pre_create = vec!["sh -c 'exit 4'".into()];
    let (created, _) = p.materialize(&p.on_branch("feat/x"), Some("main"), false);
    let error = created.unwrap_err();
    assert!(
        matches!(&error, CreateError::Hooks(HookFailure::Blocking(_))),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "pre-create hook \"sh -c 'exit 4'\" failed: exit status 4"
    );
    assert!(!p.wt("app-wt").exists());
    assert!(!p.has_ref("refs/heads/feat/x"));
}

#[test]
fn a_git_failure_is_the_error() {
    let p = Project::new();
    let (created, _) = p.materialize(&p.on_branch("bad..name"), Some("main"), false);
    let error = created.unwrap_err();
    assert!(matches!(error, CreateError::Git(_)), "{error:?}");
    assert!(!p.wt("app-wt").exists());
    assert_eq!(
        p.log().as_deref(),
        Some("pre bad..name\n"),
        "no post-create"
    );
}

#[test]
fn provisioning_that_fails_after_git_names_the_worktree() {
    let mut p = Project::new();
    p.settings.config.templates[0].target = "README/db.yml".into();
    std::fs::write(p.main.join("README"), "hi\n").unwrap();
    p.sh(&["add", "README"]);
    p.sh(&["commit", "-q", "-m", "readme"]);
    let path = p.wt("app-wt");
    let (created, _) = p.materialize(&p.on_branch("feat/x"), Some("main"), false);
    let error = created.unwrap_err();
    assert!(
        error.to_string().starts_with(&format!(
            "worktree {} was created, but provisioning it failed: {}/README/db.yml: ",
            path.display(),
            path.display()
        )),
        "{error}"
    );
    assert!(path.is_dir());
    assert_eq!(p.log().as_deref(), Some("pre feat/x\n"), "no post-create");
}

#[test]
fn a_failing_post_create_hook_is_a_warning() {
    let mut p = Project::new();
    p.settings.config.hooks.post_create = vec!["sh -c 'exit 5'".into()];
    let (created, events) = p.materialize(&p.on_branch("feat/x"), Some("main"), false);
    assert!(created.unwrap().is_some());
    assert_eq!(
        events.last(),
        Some(&HookEvent::Warning(
            "post-create hook \"sh -c 'exit 5'\" failed: exit status 5".into()
        ))
    );
}
