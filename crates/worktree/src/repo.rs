//! The repository a directory is inside, read from disk without running
//! `git`: the files are read here, [`tasq_core::worktree::git`] interprets
//! them.

use std::fs;
use std::path::{Path, PathBuf};

use tasq_core::worktree::git;
use tasq_core::worktree::repo_config::Found;

/// A git repository as found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The main checkout: where the shared git directory lives, so a
    /// linked worktree names the repository it belongs to.
    pub main: PathBuf,
    /// The repository name from `remote.origin.url`, when there is one.
    pub name: Option<String>,
}

impl Repo {
    /// The repository `dir` is inside, if any.
    pub fn of(dir: &Path) -> Option<Self> {
        let root = git::work_tree_root(dir, Path::exists)?;
        let dot_git = root.join(".git");
        let dot_git_file = dot_git
            .is_file()
            .then(|| fs::read_to_string(&dot_git).unwrap_or_default());
        let git_dir = git::git_dir(root, dot_git_file.as_deref());
        let commondir = fs::read_to_string(git_dir.join("commondir")).ok();
        let common = git::common_dir(&git_dir, commondir.as_deref());
        let common = common.canonicalize().unwrap_or(common);
        let name = fs::read_to_string(common.join("config"))
            .ok()
            .and_then(|config| git::repo_name(&config));
        Some(Self {
            main: git::main_checkout(root, &common),
            name,
        })
    }
}

/// What `path` points at, for
/// [`tasq_core::worktree::repo_config::check`]. The repository is found
/// from `path` as written, as the worktree commands find it from a task's
/// project.
pub fn find(path: &Path) -> Found {
    if !path.is_dir() {
        return Found::Missing;
    }
    let (Some(root), Some(repo)) = (git::work_tree_root(path, Path::exists), Repo::of(path)) else {
        return Found::NotRepository;
    };
    Found::Repository {
        resolved: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        root: root.to_path_buf(),
        main: repo.main,
    }
}
