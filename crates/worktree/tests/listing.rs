//! `listing::rows` against a temporary repository: the scope, the
//! worktrees left out, and the changes git reports with the provisioned
//! files left out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::config::WorktreeConfig;
use tasq_core::model::{Status, Task, TaskId, Worktree};
use tasq_core::worktree::listing::{Row, Scope};
use tasq_worktree::listing::{rows, scope};
use tasq_worktree::{Git, Repo, RepoSettings};
use tempfile::TempDir;

/// A main checkout at `<root>/ws/app` with one commit, and a directory
/// `<root>/ws/plain` outside any repository.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    main: PathBuf,
    plain: PathBuf,
    git: Git,
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
        let plain = root.join("ws").join("plain");
        std::fs::create_dir_all(&main).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(main.join("README"), "app\n").unwrap();
        let project = Self {
            _dir: dir,
            root,
            main,
            plain,
            git,
        };
        project.sh(&project.main, &["init", "-q"]);
        project.sh(&project.main, &["add", "."]);
        project.sh(&project.main, &["commit", "-q", "-m", "init"]);
        project
    }

    fn sh(&self, cwd: &Path, args: &[&str]) {
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
    }

    /// A worktree at `<root>/ws/<name>` on the new `branch`.
    fn worktree(&self, name: &str, branch: &str) -> Worktree {
        let path = self.root.join("ws").join(name);
        let p = path.display().to_string();
        self.sh(&self.main, &["worktree", "add", "-q", "-b", branch, &p]);
        Worktree::on_branch(path, branch)
    }

    /// A declaration whose directory does not exist.
    fn missing(&self, name: &str) -> Worktree {
        Worktree::on_branch(self.root.join("ws").join(name), name)
    }

    /// Settings provisioning `seed` by copy.
    fn settings(&self, project: &Path) -> RepoSettings {
        let mut config = WorktreeConfig::default();
        config.copies = vec!["seed".into()];
        RepoSettings {
            project: project.to_path_buf(),
            repo: Repo::of(project),
            main: self.main.clone(),
            config,
            templates_dir: self.main.clone(),
            repo_file: None,
            untrusted: Vec::new(),
        }
    }
}

fn task(id: u64, done: bool, project: Option<&Path>, worktrees: Vec<Worktree>) -> Task {
    Task {
        done,
        project: project.map(Path::to_path_buf),
        worktrees,
        ..Task::new(TaskId::from(id), format!("T{id}"))
    }
}

/// `(id, path name, state)` per row.
fn summary(rows: &[Row]) -> Vec<(String, String, String)> {
    rows.iter()
        .map(|r| {
            let name = r.worktree.path.file_name().unwrap().to_string_lossy();
            (r.id.to_string(), name.into_owned(), r.state())
        })
        .collect()
}

fn triple(id: &str, name: &str, state: &str) -> (String, String, String) {
    (id.to_owned(), name.to_owned(), state.to_owned())
}

#[test]
fn the_scope_is_the_repository_of_the_current_directory() {
    let p = Project::new();
    let a = p.worktree("app-a", "a");
    assert_eq!(scope(false, false, &a.path), Scope::Repo(p.main.clone()));
    assert_eq!(scope(false, false, &p.main), Scope::Repo(p.main.clone()));
    assert_eq!(scope(false, false, &p.plain), Scope::Everywhere);
    assert_eq!(scope(false, true, &p.main), Scope::Everywhere);
    assert_eq!(scope(true, false, &p.main), Scope::Task);
}

#[test]
fn rows_of_the_repository_with_their_changes() {
    let p = Project::new();
    let a = p.worktree("app-a", "a");
    let b = p.worktree("app-b", "b");
    std::fs::write(a.path.join("seed"), "provisioned\n").unwrap();
    std::fs::write(a.path.join("work"), "x\n").unwrap();
    std::fs::write(b.path.join("seed"), "provisioned\n").unwrap();
    let elsewhere = Worktree::on_branch(p.plain.clone(), "p");
    let mut open = task(1, false, Some(&p.main), vec![a.clone(), p.missing("app-c")]);
    open.status = Some(Status::READY);
    let tasks = [
        open,
        task(2, true, Some(&p.main), vec![b.clone(), p.missing("app-d")]),
        task(3, false, Some(&p.plain), vec![elsewhere]),
        task(4, false, None, vec![]),
    ];
    let loaded = RefCell::new(Vec::new());
    let list = |scope: &Scope| {
        rows(
            &p.git,
            &tasks,
            scope,
            |task| task.project.clone(),
            |project| {
                loaded.borrow_mut().push(project.to_path_buf());
                Some(p.settings(project))
            },
        )
    };

    let here = list(&Scope::Repo(p.main.clone()));
    assert_eq!(
        summary(&here),
        [
            triple("1", "app-a", "1 changed"),
            triple("1", "app-c", "not created"),
            triple("2", "app-b", "clean"),
        ],
        "the done task's missing worktree is left out, the other repository too"
    );
    assert_eq!(here[0].changes, Some(vec!["work".to_owned()]));
    assert_eq!(
        (here[0].title.as_str(), here[0].status.as_deref()),
        ("T1", Some("ready"))
    );
    assert_eq!(
        (here[2].status.as_deref(), here[2].done),
        (Some("done"), true)
    );
    assert_eq!(
        *loaded.borrow(),
        std::slice::from_ref(&p.main),
        "settings load once per project"
    );

    loaded.borrow_mut().clear();
    let all = list(&Scope::Everywhere);
    assert_eq!(
        summary(&all)[3..],
        [triple("3", "plain", "unknown")],
        "outside a repository git cannot say"
    );
    assert_eq!(*loaded.borrow(), [p.main.clone(), p.plain.clone()]);

    let one = rows(
        &p.git,
        &tasks[1..2],
        &Scope::Task,
        |_| None,
        |_| unreachable!("a task without a project loads no settings"),
    );
    assert_eq!(
        summary(&one),
        [
            triple("2", "app-b", "unknown"),
            triple("2", "app-d", "not created")
        ],
        "a task asked for shows its missing worktrees; without a project, no changes"
    );
}

#[test]
fn without_settings_every_change_counts() {
    let p = Project::new();
    let a = p.worktree("app-a", "a");
    std::fs::write(a.path.join("seed"), "x\n").unwrap();
    let tasks = [task(1, false, Some(&p.main), vec![a])];
    let listed = rows(
        &p.git,
        &tasks,
        &Scope::Everywhere,
        |task| task.project.clone(),
        |_| None,
    );
    assert_eq!(listed[0].changes, Some(vec!["seed".to_owned()]));
}

#[test]
fn only_projects_with_an_existing_worktree_load_settings() {
    let p = Project::new();
    let tasks = [task(1, false, Some(&p.main), vec![p.missing("app-a")])];
    let listed = rows(
        &p.git,
        &tasks,
        &Scope::Everywhere,
        |task| task.project.clone(),
        |_| unreachable!("no worktree exists"),
    );
    assert_eq!(summary(&listed), [triple("1", "app-a", "not created")]);
}
