//! The worktree managers against a temporary git repository and a fake
//! provisioning tool (`mkwt`) standing in for gwm.

use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::work::{WorkError, WorktreeManager};
use tasq_launch::{CommandManager, GitManager, current_branch};
use tempfile::TempDir;

/// A temp tree with `home/.gitconfig`, `bin/git` and a committed repository
/// at `ws/proj`.
struct Repo {
    root: TempDir,
    project: PathBuf,
    bin: PathBuf,
    env: Vec<(String, String)>,
}

impl Repo {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let home = root_path.join("home");
        let bin = root_path.join("bin");
        let project = root_path.join("ws").join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = t\n\temail = t@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join("git"))
            .find(|p| p.is_file())
            .expect("git on PATH");
        std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
        let env = vec![
            ("HOME".to_owned(), home.display().to_string()),
            ("PATH".to_owned(), bin.display().to_string()),
        ];
        let repo = Self {
            root,
            project,
            bin,
            env,
        };
        repo.git(&["init", "-q"]);
        std::fs::write(repo.project.join("README"), "hi\n").unwrap();
        repo.git(&["add", "README"]);
        repo.git(&["commit", "-q", "-m", "init"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.project)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn workspace(&self) -> PathBuf {
        self.project.parent().unwrap().to_path_buf()
    }

    /// Installs a fake worktree tool `mkwt` in `bin` whose argv and working
    /// directory go to `mkwt.log`.
    fn fake_tool(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let log = self.root.path().join("mkwt.log");
        let script = self.bin.join("mkwt");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\nprintf '%s\\n' \"$*\" >> '{}'\nprintf 'cwd=%s\\n' \"$PWD\" >> '{}'\n{body}\n",
                log.display(),
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Another test thread forking while the script was open for writing
        // makes the first exec fail with ETXTBSY; probe until it runs.
        for _ in 0..200 {
            if Command::new(&script)
                .env("FAKE_PROBE", "1")
                .status()
                .is_ok_and(|s| s.success())
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("fake mkwt never became runnable");
    }

    fn tool_log(&self) -> String {
        std::fs::read_to_string(self.root.path().join("mkwt.log")).unwrap_or_default()
    }
}

#[test]
fn git_manager_creates_a_new_branch_next_to_the_project() {
    let repo = Repo::new();
    let manager = GitManager {
        env: repo.env.clone(),
    };
    assert_eq!(manager.name(), "git");
    let created = manager.create(&repo.project, "feature/x").unwrap();
    assert_eq!(created.path, repo.workspace().join("proj-feature-x"));
    assert_eq!(created.branch, "feature/x");
    assert_eq!(created.messages, Vec::<String>::new());
    assert_eq!(
        current_branch(&created.path, &repo.env).as_deref(),
        Some("feature/x")
    );
    // Again: the directory exists, so it is reused rather than failing.
    let again = manager.create(&repo.project, "feature/x").unwrap();
    assert_eq!(again.path, created.path);
    assert_eq!(
        again.messages,
        vec![format!(
            "reusing existing worktree {}",
            created.path.display()
        )]
    );
}

#[test]
fn git_manager_checks_out_an_existing_branch_without_creating_it() {
    let repo = Repo::new();
    repo.git(&["branch", "existing"]);
    let manager = GitManager {
        env: repo.env.clone(),
    };
    let created = manager.create(&repo.project, "existing").unwrap();
    assert_eq!(
        current_branch(&created.path, &repo.env).as_deref(),
        Some("existing")
    );
}

#[test]
fn git_manager_errors() {
    let repo = Repo::new();
    let manager = GitManager {
        env: repo.env.clone(),
    };
    let missing = repo.workspace().join("nope");
    assert_eq!(
        manager.create(&missing, "b"),
        Err(WorkError::ProjectMissing(missing))
    );
    let err = manager.create(&repo.project, "bad..name").unwrap_err();
    match err {
        WorkError::Tool {
            tool,
            branch,
            message,
        } => {
            assert_eq!((tool.as_str(), branch.as_str()), ("git", "bad..name"));
            assert_ne!(message, "");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn command_manager_fills_the_template_and_reads_the_last_line() {
    let repo = Repo::new();
    let wt = repo.root.path().canonicalize().unwrap().join("wt");
    repo.fake_tool(&format!(
        "[ \"$1\" = --fresh ] && shift\n/bin/mkdir -p '{}/'\"$1\"\necho 'Linked .envrc'\necho\necho '{}/'\"$1\"",
        wt.display(),
        wt.display()
    ));
    let manager = CommandManager {
        template: "mkwt {new:--fresh} {branch} {project}".to_owned(),
        env: repo.env.clone(),
    };
    assert_eq!(manager.name(), "command");
    let created = manager.create(&repo.project, "feature/x").unwrap();
    assert_eq!(created.path, wt.join("feature/x"));
    assert_eq!(created.branch, "feature/x");
    assert_eq!(created.messages, vec!["Linked .envrc".to_owned()]);
    assert_eq!(
        repo.tool_log(),
        format!(
            "--fresh feature/x {}\ncwd={}\n",
            repo.project.display(),
            repo.project.display()
        )
    );
    // An existing branch gets no {new} text.
    repo.git(&["branch", "existing"]);
    manager.create(&repo.project, "existing").unwrap();
    assert!(
        repo.tool_log().ends_with(&format!(
            "existing {}\ncwd={}\n",
            repo.project.display(),
            repo.project.display()
        )),
        "{}",
        repo.tool_log()
    );
}

#[test]
fn command_manager_errors() {
    let repo = Repo::new();
    let env = repo.env.clone();
    let missing = repo.workspace().join("nope");
    let manager = CommandManager {
        template: "mkwt {branch}".to_owned(),
        env: env.clone(),
    };
    assert_eq!(
        manager.create(&missing, "b"),
        Err(WorkError::ProjectMissing(missing))
    );
    repo.fake_tool("echo 'no such workspace' >&2\nexit 1");
    assert_eq!(
        manager.create(&repo.project, "b"),
        Err(WorkError::Tool {
            tool: "mkwt".into(),
            branch: "b".into(),
            message: "no such workspace".into()
        })
    );
    repo.fake_tool("echo 'did things'");
    assert_eq!(
        manager.create(&repo.project, "b"),
        Err(WorkError::NoPath {
            tool: "mkwt".into(),
            output: "did things".into()
        })
    );
    repo.fake_tool("exit 0");
    assert_eq!(
        manager.create(&repo.project, "b"),
        Err(WorkError::NoPath {
            tool: "mkwt".into(),
            output: String::new()
        })
    );
    // A program that is not on PATH fails like a failed run.
    let absent = CommandManager {
        template: "no-such-tool {branch}".to_owned(),
        env: env.clone(),
    };
    match absent.create(&repo.project, "b").unwrap_err() {
        WorkError::Tool { tool, message, .. } => {
            assert_eq!(tool, "no-such-tool");
            assert!(message.contains("could not run no-such-tool"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let bad = CommandManager {
        template: "mkwt {nope}".to_owned(),
        env,
    };
    assert!(matches!(
        bad.create(&repo.project, "b").unwrap_err(),
        WorkError::BadCommand { .. }
    ));
}

#[test]
fn current_branch_outside_a_repository_is_none() {
    let repo = Repo::new();
    assert_eq!(current_branch(&repo.workspace(), &repo.env), None);
    assert_eq!(
        current_branch(&repo.project, &repo.env).as_deref(),
        Some("main")
    );
    assert_eq!(
        current_branch(Path::new("/definitely/not/here"), &repo.env),
        None
    );
}
