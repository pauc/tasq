//! The git worktree manager against a temporary git repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::work::{WorkError, WorktreeManager};
use tasq_launch::{GitManager, current_branch};
use tempfile::TempDir;

/// A temp tree with `home/.gitconfig`, `bin/git` and a committed repository
/// at `ws/proj`.
struct Repo {
    /// Owns the tree; dropping it removes everything.
    _root: TempDir,
    project: PathBuf,
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
            _root: root,
            project,
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
        other @ WorkError::ProjectMissing(_) => panic!("unexpected {other:?}"),
    }
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
