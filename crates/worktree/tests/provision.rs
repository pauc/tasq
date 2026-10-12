//! Provisioning a real worktree from the workspace fixture configurations
//! of `tasq-core` (`tests/fixtures/worktree/*.toml`): the tree that results
//! is checked against the files under `tests/fixtures/<workspace>/expected`.

use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::config::{Config, WorktreeConfig};
use tasq_core::model::{Task, TaskId};
use tasq_core::worktree::declaration::declare;
use tasq_core::worktree::lifecycle::{Declared, Start};
use tasq_core::worktree::provision::{Step, plan};
use tasq_core::worktree::vars::Vars;
use tasq_worktree::Git;
use tasq_worktree::provision::{ProvisionError, Ready, apply, prepare};
use tempfile::TempDir;

const BRANCH: &str = "feature/Login-2";

/// A main checkout at `<root>/ws/shop` with gitignored files to share.
struct Workspace {
    _dir: TempDir,
    main: PathBuf,
    git: Git,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        let bin = root.join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = t\n\temail = t@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let git_bin = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join("git"))
            .find(|p| p.is_file())
            .expect("git on PATH");
        std::os::unix::fs::symlink(git_bin, bin.join("git")).unwrap();
        let git = Git {
            env: vec![
                ("HOME".to_owned(), home.display().to_string()),
                ("PATH".to_owned(), bin.display().to_string()),
            ],
        };
        let main = root.join("ws").join("shop");
        let ws = Self {
            _dir: dir,
            main,
            git,
        };
        for (path, text) in [
            (
                ".gitignore",
                "node_modules/\nCLAUDE.local.md\nconfig/master.key\n.envrc\n",
            ),
            ("README", "shop\n"),
            ("config/app.yml", "name: shop\n"),
            ("node_modules/pkg/index.js", "module.exports = 1;\n"),
            ("CLAUDE.local.md", "# notes\n"),
            ("config/master.key", "secret\n"),
            (".envrc", "export SECRET=1\n"),
        ] {
            let file = ws.main.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        ws.sh(&["init", "-q"]);
        ws.sh(&["add", "."]);
        ws.sh(&["commit", "-q", "-m", "init"]);
        ws
    }

    fn sh(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.main)
            .env_clear()
            .envs(self.git.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    /// Declares task 12's worktree with `config`, creates it, and returns
    /// its path, the plan and the variables.
    fn worktree(&self, config: &WorktreeConfig, templates: &Path) -> (PathBuf, Vec<Step>, Vars) {
        let task = Task::new(TaskId::from(12), "Fix the login form");
        let Ok(Declared::New(declared)) =
            declare(&task, Some(BRANCH), config, &self.main, Some("shop"), None)
        else {
            panic!("not declared");
        };
        let path = declared.path;
        self.git
            .add(&self.main, &path, BRANCH, &Start::New("HEAD".into()))
            .unwrap();
        let steps = plan(config, &self.main, &path, templates).unwrap();
        let vars = Vars::for_task(&task, &self.main, Some("shop"), &config.variables)
            .with_branch(BRANCH, config.ports)
            .with_worktree(&path);
        (path, steps, vars)
    }
}

fn config(name: &str) -> WorktreeConfig {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/tests/fixtures/worktree")
        .join(name);
    let config: Config = toml::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    config.worktree
}

fn fixtures(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn expected(workspace: &str, file: &str) -> String {
    std::fs::read_to_string(fixtures(workspace).join("expected").join(file)).unwrap()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn sibling_links_dependencies_and_renders_the_envrc() {
    let ws = Workspace::new();
    let (wt, steps, vars) = ws.worktree(&config("sibling.toml"), &fixtures("sibling"));
    assert_eq!(wt, ws.main.parent().unwrap().join("feature_Login_2"));
    let warnings = apply(&prepare(&steps, &vars).unwrap()).unwrap();
    assert_eq!(warnings, Vec::<String>::new());
    for rel in ["node_modules", "CLAUDE.local.md", "config/master.key"] {
        assert_eq!(
            std::fs::read_link(wt.join(rel)).unwrap(),
            ws.main.join(rel),
            "{rel}"
        );
    }
    assert_eq!(
        read(&wt.join("node_modules/pkg/index.js")),
        "module.exports = 1;\n"
    );
    assert_eq!(read(&wt.join(".envrc")), expected("sibling", ".envrc"));
}

#[test]
fn per_branch_databases_render_both_templates() {
    let ws = Workspace::new();
    let (wt, steps, vars) = ws.worktree(&config("databases.toml"), &fixtures("databases"));
    assert_eq!(wt, ws.main.parent().unwrap().join("shop-feature_Login_2"));
    assert_eq!(apply(&prepare(&steps, &vars).unwrap()), Ok(Vec::new()));
    assert_eq!(read(&wt.join(".envrc")), expected("databases", ".envrc"));
    assert_eq!(
        read(&wt.join("config/database.yml")),
        expected("databases", "config/database.yml")
    );
    assert_eq!(
        read(&wt.join("config/app.yml")),
        "name: shop\n",
        "the checkout is kept"
    );
}

#[test]
fn an_ignored_envrc_is_copied_as_it_is() {
    let ws = Workspace::new();
    let (wt, steps, vars) = ws.worktree(&config("ignored-envrc.toml"), &fixtures("none"));
    assert_eq!(apply(&prepare(&steps, &vars).unwrap()), Ok(Vec::new()));
    let copied = std::fs::symlink_metadata(wt.join(".envrc")).unwrap();
    assert!(copied.is_file(), "a copy, not a link");
    assert_eq!(read(&wt.join(".envrc")), "export SECRET=1\n");
}

#[test]
fn missing_sources_and_existing_targets_are_warnings() {
    let ws = Workspace::new();
    let mut config = WorktreeConfig::default();
    config.copies = vec!["nope.txt".into(), "README".into()];
    config.symlinks = vec!["gone".into(), "config/app.yml".into()];
    let (wt, steps, vars) = ws.worktree(&config, &fixtures("none"));
    let warnings = apply(&prepare(&steps, &vars).unwrap()).unwrap();
    assert_eq!(
        warnings,
        [
            format!(
                "copy: {} not found, skipped",
                ws.main.join("nope.txt").display()
            ),
            format!(
                "copy: {} is in the checkout already, kept",
                wt.join("README").display()
            ),
            format!(
                "link: {} not found, skipped",
                ws.main.join("gone").display()
            ),
            format!(
                "link: {} is in the checkout already, kept",
                wt.join("config/app.yml").display()
            ),
        ]
    );
    assert!(!wt.join("nope.txt").exists());
    assert!(std::fs::symlink_metadata(wt.join("gone")).is_err());
    assert!(
        std::fs::symlink_metadata(wt.join("config/app.yml"))
            .unwrap()
            .is_file()
    );
}

#[test]
fn a_copied_directory_keeps_its_files_and_links() {
    let ws = Workspace::new();
    let shared = ws.main.join("shared");
    std::fs::create_dir_all(shared.join("deep")).unwrap();
    std::fs::write(shared.join("deep/file"), "x\n").unwrap();
    std::os::unix::fs::symlink("deep/file", shared.join("alias")).unwrap();
    let mut config = WorktreeConfig::default();
    config.copies = vec!["shared".into()];
    let (wt, steps, vars) = ws.worktree(&config, &fixtures("none"));
    assert_eq!(apply(&prepare(&steps, &vars).unwrap()), Ok(Vec::new()));
    assert_eq!(read(&wt.join("shared/deep/file")), "x\n");
    assert_eq!(
        std::fs::read_link(wt.join("shared/alias")).unwrap(),
        PathBuf::from("deep/file")
    );
}

#[test]
fn a_bad_template_is_an_error_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let vars = Vars::for_task(
        &Task::new(TaskId::from(1), "T"),
        Path::new("/ws/app"),
        None,
        &std::collections::BTreeMap::new(),
    );
    let missing = dir.path().join("missing.template");
    let render = |source: &Path| {
        vec![Step::Render {
            source: source.to_path_buf(),
            target: dir.path().join("out"),
        }]
    };
    let err = prepare(&render(&missing), &vars).unwrap_err();
    assert!(
        matches!(&err, ProvisionError::Template { path, .. } if *path == missing),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .starts_with(&format!("template {}: ", missing.display()))
    );
    let unknown = dir.path().join("unknown.template");
    std::fs::write(&unknown, "{{nope}}\n").unwrap();
    assert_eq!(
        prepare(&render(&unknown), &vars),
        Err(ProvisionError::Template {
            path: unknown.clone(),
            message: "unknown placeholder {{nope}}".into()
        })
    );
    assert!(!dir.path().join("out").exists());
}

#[test]
fn a_write_the_filesystem_refuses_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file"), "").unwrap();
    let target = dir.path().join("file/below");
    let err = apply(&[Ready::Write {
        target: target.clone(),
        text: "x".into(),
    }])
    .unwrap_err();
    assert!(
        matches!(&err, ProvisionError::Write { path, .. } if *path == target),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .starts_with(&format!("{}: ", target.display()))
    );
}
