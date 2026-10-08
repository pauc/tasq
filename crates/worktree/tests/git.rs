//! `Git` against temporary repositories: a bare `origin`, the project
//! cloned from it, and a second clone that pushes branches the project
//! has not fetched.

use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::worktree::lifecycle::{BranchState, Start};
use tasq_worktree::{Added, Git, GitError};
use tempfile::TempDir;

struct Repos {
    _root: TempDir,
    root: PathBuf,
    /// The clone tasks work in.
    project: PathBuf,
    /// Another clone, for branches only the remote has.
    other: PathBuf,
    git: Git,
}

impl Repos {
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
        let repos = Self {
            _root: dir,
            project: root.join("ws").join("app"),
            other: root.join("other"),
            root,
            git,
        };
        let seed = repos.root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        repos.sh(&seed, &["init", "-q"]);
        std::fs::write(seed.join("README"), "hi\n").unwrap();
        repos.sh(&seed, &["add", "README"]);
        repos.sh(&seed, &["commit", "-q", "-m", "init"]);
        let origin = repos.root.join("origin.git");
        repos.sh(
            &repos.root,
            &["clone", "-q", "--bare", "seed", "origin.git"],
        );
        for clone in [&repos.project, &repos.other] {
            repos.sh(
                &repos.root,
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
        }
        repos
    }

    fn sh(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
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

    /// Commits a file on a new branch in `other` and pushes it.
    fn push_from_other(&self, branch: &str) -> String {
        self.sh(&self.other, &["checkout", "-q", "-b", branch]);
        std::fs::write(self.other.join(branch.replace('/', "_")), "x\n").unwrap();
        self.sh(&self.other, &["add", "."]);
        self.sh(&self.other, &["commit", "-q", "-m", branch]);
        self.sh(&self.other, &["push", "-q", "origin", branch]);
        self.sh(&self.other, &["rev-parse", "HEAD"])
    }

    fn wt(&self, name: &str) -> PathBuf {
        self.root.join("ws").join(name)
    }

    fn upstream(&self, dir: &Path) -> Option<String> {
        let out = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
            .current_dir(dir)
            .env_clear()
            .envs(self.git.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
}

#[test]
fn a_local_branch_is_checked_out() {
    let r = Repos::new();
    r.sh(&r.project, &["branch", "feat/local"]);
    assert_eq!(
        r.git.branch_state(&r.project, "feat/local", true),
        Ok(BranchState::Local)
    );
    let path = r.wt("app-local");
    assert_eq!(
        r.git.add(&r.project, &path, "feat/local", &Start::Local),
        Ok(Added::default())
    );
    assert_eq!(r.git.current_branch(&path).as_deref(), Some("feat/local"));
}

#[test]
fn a_branch_only_the_remote_has_is_fetched_and_tracked() {
    let r = Repos::new();
    let tip = r.push_from_other("feat/remote");
    assert_eq!(
        r.git.branch_state(&r.project, "feat/remote", false),
        Ok(BranchState::Remote("origin/feat/remote".into()))
    );
    assert!(
        !r.sh(&r.project, &["branch", "-r"])
            .contains("origin/feat/remote"),
        "a dry run fetches nothing"
    );
    assert_eq!(
        r.git.branch_state(&r.project, "feat/remote", true),
        Ok(BranchState::Remote("origin/feat/remote".into()))
    );
    let path = r.wt("app-remote");
    r.git
        .add(
            &r.project,
            &path,
            "feat/remote",
            &Start::Track("origin/feat/remote".into()),
        )
        .unwrap();
    assert_eq!(r.git.current_branch(&path).as_deref(), Some("feat/remote"));
    assert_eq!(r.upstream(&path).as_deref(), Some("origin/feat/remote"));
    assert_eq!(r.sh(&path, &["rev-parse", "HEAD"]), tip);
}

#[test]
fn a_new_branch_starts_from_the_base_without_tracking_it() {
    let r = Repos::new();
    assert_eq!(
        r.git.branch_state(&r.project, "feat/new", true),
        Ok(BranchState::Absent)
    );
    // The project's own commits do not move origin/HEAD.
    std::fs::write(r.project.join("local"), "x\n").unwrap();
    r.sh(&r.project, &["add", "local"]);
    r.sh(&r.project, &["commit", "-q", "-m", "local only"]);
    let path = r.wt("app-new");
    assert_eq!(
        r.git.add(
            &r.project,
            &path,
            "feat/new",
            &Start::New("origin/HEAD".into())
        ),
        Ok(Added::default())
    );
    assert_eq!(r.git.current_branch(&path).as_deref(), Some("feat/new"));
    assert_eq!(r.upstream(&path), None, "never tracks the base");
    assert_eq!(
        r.sh(&path, &["rev-parse", "HEAD"]),
        r.sh(&r.project, &["rev-parse", "origin/main"])
    );
}

#[test]
fn without_origin_head_a_new_branch_starts_from_head() {
    let r = Repos::new();
    r.sh(&r.project, &["remote", "set-head", "origin", "--delete"]);
    let path = r.wt("app-head");
    let added = r
        .git
        .add(&r.project, &path, "b", &Start::New("origin/HEAD".into()))
        .unwrap();
    assert_eq!(
        added.note.as_deref(),
        Some(
            "origin/HEAD is not set; b starts from HEAD \
             (set worktree.base, or run git remote set-head origin --auto)"
        )
    );
    assert_eq!(r.git.current_branch(&path).as_deref(), Some("b"));
    // Another base that does not resolve is git's error.
    let err = r
        .git
        .add(
            &r.project,
            &r.wt("app-nope"),
            "c",
            &Start::New("nope".into()),
        )
        .unwrap_err();
    assert!(
        matches!(&err, GitError::Failed { command, .. }
            if command == &format!("worktree add --no-track -b c {} nope", r.wt("app-nope").display())),
        "{err:?}"
    );
    assert!(!r.wt("app-nope").exists());
}

#[test]
fn offline_an_already_fetched_branch_still_counts() {
    let r = Repos::new();
    r.push_from_other("feat/seen");
    r.sh(&r.project, &["fetch", "-q"]);
    r.sh(
        &r.project,
        &["remote", "set-url", "origin", "/definitely/not/here.git"],
    );
    assert_eq!(
        r.git.branch_state(&r.project, "feat/seen", true),
        Ok(BranchState::Remote("origin/feat/seen".into()))
    );
    assert_eq!(
        r.git.branch_state(&r.project, "feat/never", true),
        Ok(BranchState::Absent)
    );
    r.sh(&r.project, &["remote", "remove", "origin"]);
    assert_eq!(
        r.git.branch_state(&r.project, "feat/never", true),
        Ok(BranchState::Absent),
        "no origin at all"
    );
}

#[test]
fn a_failure_leaves_nothing_at_the_path() {
    let r = Repos::new();
    // `main` is checked out in the project already.
    let path = r.wt("app-main");
    let err = r
        .git
        .add(&r.project, &path, "main", &Start::Local)
        .unwrap_err();
    assert!(
        matches!(&err, GitError::Failed { message, .. } if message.contains("main")),
        "{err:?}"
    );
    assert!(!path.exists());
    assert_eq!(
        r.sh(&r.project, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    // Something already at the path is never touched.
    let taken = r.wt("taken");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("keep"), "x").unwrap();
    assert_eq!(
        r.git
            .add(&r.project, &taken, "b", &Start::New("HEAD".into())),
        Err(GitError::Exists(taken.clone()))
    );
    assert!(taken.join("keep").is_file());
    let missing = r.root.join("missing");
    assert_eq!(
        r.git.add(&missing, &r.wt("x"), "b", &Start::Local),
        Err(GitError::NoRepo(missing.clone()))
    );
    assert_eq!(
        r.git.branch_state(&missing, "b", false),
        Err(GitError::NoRepo(missing))
    );
}

#[test]
fn a_failed_fetch_is_an_error() {
    let r = Repos::new();
    r.push_from_other("feat/f");
    // ls-remote answers, but the fetch cannot write the ref.
    let refs = r
        .project
        .join(".git")
        .join("refs")
        .join("remotes")
        .join("origin");
    std::fs::create_dir_all(refs.join("feat")).unwrap();
    std::fs::write(refs.join("feat").join("f.lock"), "").unwrap();
    let err = r.git.branch_state(&r.project, "feat/f", true).unwrap_err();
    assert!(
        matches!(&err, GitError::Failed { command, .. }
            if command == "fetch --quiet origin +refs/heads/feat/f:refs/remotes/origin/feat/f"),
        "{err:?}"
    );
}

#[test]
fn current_branch_outside_a_repository_is_none() {
    let r = Repos::new();
    assert_eq!(r.git.current_branch(&r.root), None);
    assert_eq!(r.git.current_branch(&r.project).as_deref(), Some("main"));
    assert_eq!(
        r.git.current_branch(Path::new("/definitely/not/here")),
        None
    );
    r.sh(&r.project, &["checkout", "-q", "--detach"]);
    assert_eq!(r.git.current_branch(&r.project), None, "detached");
}
