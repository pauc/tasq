//! `release` against a temporary repository: what it removes and deletes,
//! what it keeps and says, what a dry run leaves alone, and the hooks
//! around a removal.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::config::{OnDone, WorktreeConfig};
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::worktree::hooks::HookEvent;
use tasq_core::worktree::lifecycle::DeleteBranch;
use tasq_worktree::release::{ReleaseError, Releasing, canonical, release};
use tasq_worktree::{Git, Repo, RepoSettings};
use tempfile::TempDir;

/// A main checkout at `<root>/ws/app` with one commit; the remove hooks
/// append to `<root>/log`.
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
        std::fs::create_dir_all(&main).unwrap();
        std::fs::write(main.join(".gitignore"), ".env\n").unwrap();
        let mut config = WorktreeConfig::default();
        config.copies = vec![".env".into()];
        config
            .variables
            .insert("out".into(), root.display().to_string());
        config.hooks.pre_remove = vec!["sh -c 'echo pre $TASQ_BRANCH $PWD >> {{out}}/log'".into()];
        config.hooks.post_remove = vec!["sh -c 'echo post $PWD >> {{out}}/log'".into()];
        let settings = RepoSettings {
            project: main.clone(),
            repo: None,
            main: main.clone(),
            config,
            templates_dir: main.clone(),
        };
        let mut project = Self {
            _dir: dir,
            root,
            main,
            git,
            settings,
        };
        project.sh(&project.main, &["init", "-q"]);
        project.sh(&project.main, &["add", "."]);
        project.sh(&project.main, &["commit", "-q", "-m", "init"]);
        project.settings.repo = Repo::of(&project.main);
        project
    }

    fn sh(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
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

    /// A worktree at `<root>/ws/<name>` on the new `branch`.
    fn worktree(&self, name: &str, branch: &str) -> Worktree {
        let path = self.root.join("ws").join(name);
        let p = path.display().to_string();
        self.sh(&self.main, &["worktree", "add", "-q", "-b", branch, &p]);
        Worktree::on_branch(path, branch)
    }

    /// A commit on the worktree's branch that is nowhere else.
    fn commit_in(&self, worktree: &Worktree) {
        std::fs::write(worktree.path.join("work"), "x\n").unwrap();
        self.sh(&worktree.path, &["add", "work"]);
        self.sh(&worktree.path, &["commit", "-q", "-m", "work"]);
    }

    fn has_branch(&self, branch: &str) -> bool {
        Command::new("git")
            .args(["show-ref", "--verify", "--quiet"])
            .arg(format!("refs/heads/{branch}"))
            .current_dir(&self.main)
            .env_clear()
            .envs(self.git.env.iter().map(|(k, v)| (k, v)))
            .status()
            .unwrap()
            .success()
    }

    fn release_from(
        &self,
        cwd: &Path,
        worktrees: Vec<Worktree>,
        how: Releasing,
    ) -> (Result<Vec<String>, ReleaseError>, Vec<HookEvent>) {
        let task = Task {
            worktrees,
            ..Task::new(TaskId::from(7), "T")
        };
        let mut events = Vec::new();
        let released = release(&self.git, &task, &self.settings, cwd, how, &mut |e| {
            events.push(e);
        });
        (released, events)
    }

    fn release(
        &self,
        worktrees: Vec<Worktree>,
        how: Releasing,
    ) -> (Result<Vec<String>, ReleaseError>, Vec<HookEvent>) {
        self.release_from(&self.root, worktrees, how)
    }

    fn log(&self) -> Option<String> {
        std::fs::read_to_string(self.root.join("log")).ok()
    }
}

fn warning(text: String) -> HookEvent {
    HookEvent::Warning(text)
}

#[test]
fn a_clean_worktree_is_removed_with_its_hooks_and_its_merged_branch_deleted() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    // Provisioned files do not make it dirty.
    std::fs::write(wt.path.join(".env"), "copied\n").unwrap();
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    let path = wt.path.display();
    assert_eq!(
        released.unwrap(),
        [
            format!("removed worktree {path}"),
            "deleted branch feat/x (same commit as main)".to_owned(),
        ]
    );
    assert_eq!(events, []);
    assert!(!wt.path.exists());
    assert!(!p.has_branch("feat/x"));
    assert_eq!(
        p.log().unwrap(),
        format!("pre feat/x {path}\npost {}\n", p.main.display())
    );
}

#[test]
fn a_dry_run_says_what_it_would_do_and_changes_nothing() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    let (released, events) = p.release(vec![wt.clone()], Releasing::on_demand(false, true));
    let root = p.root.display();
    assert_eq!(
        released.unwrap(),
        [
            format!("would remove worktree {}", wt.path.display()),
            format!("  run pre-remove hook: sh -c 'echo pre $TASQ_BRANCH $PWD >> {root}/log'"),
            format!("  run post-remove hook: sh -c 'echo post $PWD >> {root}/log'"),
            "would delete branch feat/x (same commit as main)".to_owned(),
        ]
    );
    assert_eq!(events, []);
    assert!(wt.path.is_dir());
    assert!(p.has_branch("feat/x"));
    assert_eq!(p.log(), None);
}

#[test]
fn on_done_keep_skips_a_close_but_not_a_release_on_demand() {
    let mut p = Project::new();
    p.settings.config.on_done = OnDone::Keep;
    let wt = p.worktree("app-x", "feat/x");
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    assert_eq!(released.unwrap(), Vec::<String>::new());
    assert_eq!(events, []);
    assert!(wt.path.is_dir());
    let (released, _) = p.release(vec![wt.clone()], Releasing::on_demand(false, false));
    assert_eq!(released.unwrap().len(), 2);
    assert!(!wt.path.exists());
}

#[test]
fn outside_a_repository_nothing_is_released() {
    let mut p = Project::new();
    p.settings.repo = None;
    p.settings.project = p.root.join("elsewhere");
    let wt = Worktree::on_branch(p.root.join("ws/app-x"), "feat/x");
    let (released, events) = p.release(vec![wt], Releasing::closing());
    assert_eq!(
        released.unwrap_err().to_string(),
        format!("{}/elsewhere is not in a git repository", p.root.display())
    );
    assert_eq!(events, []);
}

#[test]
fn a_git_failure_is_the_error() {
    let mut p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    // The repository was found, but its main checkout is gone.
    p.settings.main = p.root.join("home");
    let (released, _) = p.release(vec![wt], Releasing::closing());
    assert!(
        matches!(released, Err(ReleaseError::Git(_))),
        "{released:?}"
    );
}

#[test]
fn a_dirty_worktree_and_its_branch_are_kept_unless_forced() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    std::fs::write(wt.path.join("notes"), "wip\n").unwrap();
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    assert_eq!(released.unwrap(), Vec::<String>::new());
    assert_eq!(
        events,
        [warning(format!(
            "kept worktree {}: it has uncommitted or untracked changes; \
             tasq worktree release 7 --force removes it",
            wt.path.display()
        ))]
    );
    assert!(wt.path.is_dir());
    assert!(p.has_branch("feat/x"));
    assert_eq!(p.log(), None);

    let (released, events) = p.release(vec![wt.clone()], Releasing::on_demand(true, false));
    assert_eq!(
        released.unwrap(),
        [
            format!("removed worktree {}", wt.path.display()),
            "deleted branch feat/x (same commit as main)".to_owned(),
        ]
    );
    assert_eq!(events, []);
}

#[test]
fn an_unsafe_branch_is_kept_with_a_warning_unless_forced() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    p.commit_in(&wt);
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    assert_eq!(
        released.unwrap(),
        [format!("removed worktree {}", wt.path.display())]
    );
    assert_eq!(
        events,
        [warning(
            "kept branch feat/x: it is neither merged into the default branch nor pushed; \
             tasq worktree release 7 --force deletes it"
                .to_owned()
        )]
    );
    assert!(p.has_branch("feat/x"));

    let (released, events) = p.release(vec![wt], Releasing::on_demand(true, false));
    assert_eq!(released.unwrap(), ["deleted branch feat/x (forced)"]);
    assert_eq!(events, []);
    assert!(!p.has_branch("feat/x"));
}

#[test]
fn a_directory_removed_by_hand_still_has_its_branch_deleted() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    std::fs::remove_dir_all(&wt.path).unwrap();
    let (released, events) = p.release(vec![wt], Releasing::closing());
    assert_eq!(
        released.unwrap(),
        ["deleted branch feat/x (same commit as main)"]
    );
    assert_eq!(events, []);
    assert!(!p.has_branch("feat/x"));
    assert_eq!(p.log(), None);
}

#[test]
fn the_main_checkout_is_kept_without_a_word() {
    let p = Project::new();
    let main = Worktree::on_branch(p.main.clone(), "main");
    let (released, events) = p.release(vec![main], Releasing::on_demand(true, false));
    assert_eq!(released.unwrap(), Vec::<String>::new());
    assert_eq!(events, []);
    assert!(p.main.is_dir());
    assert!(p.has_branch("main"));
}

#[test]
fn a_worktree_holding_the_current_directory_is_kept() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    let inside = wt.path.join("sub");
    std::fs::create_dir_all(&inside).unwrap();
    let (released, events) = p.release_from(&inside, vec![wt.clone()], Releasing::closing());
    assert_eq!(released.unwrap(), Vec::<String>::new());
    assert_eq!(
        events,
        [warning(format!(
            "kept worktree {}: the current directory is inside it; \
             tasq worktree release 7 from another directory removes it",
            wt.path.display()
        ))]
    );
    assert!(wt.path.is_dir());
}

#[test]
fn a_worktree_without_a_branch_is_removed_alone() {
    let p = Project::new();
    let wt = p.worktree("app-x", "feat/x");
    let unnamed = Worktree {
        path: wt.path.clone(),
        branch: None,
    };
    let (released, events) = p.release(vec![unnamed], Releasing::closing());
    assert_eq!(
        released.unwrap(),
        [format!("removed worktree {}", wt.path.display())]
    );
    assert_eq!(events, []);
    assert!(p.has_branch("feat/x"));
    assert_eq!(
        p.log().unwrap(),
        format!("pre {}\npost {}\n", wt.path.display(), p.main.display())
    );
}

#[test]
fn delete_branch_never_keeps_every_branch() {
    let mut p = Project::new();
    p.settings.config.delete_branch = DeleteBranch::Never;
    let wt = p.worktree("app-x", "feat/x");
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    assert_eq!(
        released.unwrap(),
        [format!("removed worktree {}", wt.path.display())]
    );
    assert_eq!(events, []);
    assert!(p.has_branch("feat/x"));
}

#[test]
fn a_failing_pre_remove_hook_keeps_the_worktree_and_its_branch() {
    let mut p = Project::new();
    p.settings.config.hooks.pre_remove = vec!["sh -c 'exit 3'".into()];
    let wt = p.worktree("app-x", "feat/x");
    let (released, events) = p.release(vec![wt.clone()], Releasing::closing());
    assert_eq!(released.unwrap(), Vec::<String>::new());
    assert_eq!(
        events,
        [warning(format!(
            "kept worktree {}: pre-remove hook \"sh -c 'exit 3'\" failed: exit status 3",
            wt.path.display()
        ))]
    );
    assert!(wt.path.is_dir());
    assert!(p.has_branch("feat/x"));
    assert_eq!(p.log(), None);
}

#[test]
fn a_hook_that_does_not_render_is_the_error() {
    let mut p = Project::new();
    p.settings.config.hooks.post_remove = vec!["echo {{nope}}".into()];
    let wt = p.worktree("app-x", "feat/x");
    for how in [Releasing::closing(), Releasing::on_demand(false, true)] {
        let (released, _) = p.release(vec![wt.clone()], how);
        assert!(
            matches!(released, Err(ReleaseError::Hooks(_))),
            "{released:?}"
        );
        assert!(wt.path.is_dir());
    }
}

#[test]
fn canonical_resolves_symlinks_and_keeps_missing_paths() {
    let p = Project::new();
    let link = p.root.join("link");
    std::os::unix::fs::symlink(&p.main, &link).unwrap();
    assert_eq!(canonical(&link), p.main);
    let missing = p.root.join("missing");
    assert_eq!(canonical(&missing), missing);
}
