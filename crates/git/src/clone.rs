//! `tasq git clone <url>`: fills an empty store from a branch of a remote,
//! with the store's identity in the repository config and the branch
//! tracking the remote's, so that the background job syncs it from then on.
//!
//! Like init, clone never merges two task histories: a store with task
//! files of its own is refused, and so is a branch the remote does not
//! have, before the store is touched. The store directory may hold other
//! files (`.tasq/`); they are kept. A failure after `git init` removes the
//! `.git` it made.

use std::path::{Path, PathBuf};

use crate::git::{Git, RemoteBranch, TASK_FILES};
use crate::hook::is_repository;
use crate::init::{Identity, InitError, create, git_init, nearest_existing, or_undo};
use crate::tasq::Store;

/// What `tasq git clone` was asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// The remote.
    pub url: String,
    /// The branch, remote and local; the store name when `None`.
    pub branch: Option<String>,
    /// The identity, flags and `[plugin.git]` defaults merged.
    pub identity: Identity,
}

/// What clone did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The store directory.
    pub store: PathBuf,
    /// The remote.
    pub url: String,
    /// The branch.
    pub branch: String,
    /// How many task files the branch holds.
    pub files: usize,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let noun = if self.files == 1 { "file" } else { "files" };
        writeln!(
            f,
            "cloned branch {} of {} into {}: {} task {noun}",
            self.branch,
            self.url,
            self.store.display(),
            self.files
        )
    }
}

/// Fills `store` from the remote branch `options` name.
pub fn clone(git: &Git, store: &Store, options: &Options) -> Result<Report, InitError> {
    let dir = &store.location;
    let url = &options.url;
    if is_repository(dir) {
        return Err(InitError::Repository(dir.clone()));
    }
    if has_task_files(dir)? {
        return Err(InitError::HasTaskFiles {
            store: dir.clone(),
            url: url.clone(),
        });
    }
    let branch = options.branch.clone().unwrap_or_else(|| store.name.clone());
    match git.remote_branch(nearest_existing(dir), url, &branch) {
        RemoteBranch::Present => {}
        RemoteBranch::Absent => {
            return Err(InitError::BranchMissing {
                url: url.clone(),
                branch,
            });
        }
        RemoteBranch::Unreachable(message) => {
            return Err(InitError::Unreachable {
                url: url.clone(),
                message,
            });
        }
    }
    create(dir)?;
    let files = or_undo(dir, fetch(git, dir, &branch, options))?;
    Ok(Report {
        store: dir.clone(),
        url: url.clone(),
        branch,
        files,
    })
}

/// `git init`, the identity, the remote fetching `branch` alone, and the
/// branch checked out tracking it; the number of task files.
fn fetch(git: &Git, dir: &Path, branch: &str, options: &Options) -> Result<usize, InitError> {
    git_init(git, dir, branch)?;
    for (key, value) in options.identity.settings() {
        git.check(dir, &["config", "--local", key, &value])?;
    }
    git.check(
        dir,
        &["remote", "add", "-t", branch, "origin", &options.url],
    )?;
    git.check(dir, &["fetch", "--quiet", "origin"])?;
    let tracked = format!("origin/{branch}");
    git.check(dir, &["checkout", "--quiet", "--track", &tracked])?;
    let mut list = vec!["ls-files", "-z", "--"];
    list.extend(TASK_FILES);
    let listed = git.check(dir, &list)?;
    Ok(listed.split('\0').filter(|f| !f.is_empty()).count())
}

/// Whether `dir` holds a task file as tasq reads them: `.md` in any case,
/// no leading dot, at the top. An absent directory holds none.
fn has_task_files(dir: &Path) -> Result<bool, InitError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(InitError::Io {
                action: "could not read",
                path: dir.to_owned(),
                source,
            });
        }
    };
    Ok(entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let markdown = Path::new(&*name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        markdown && !name.starts_with('.') && entry.path().is_file()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_that_cannot_be_read_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        std::fs::write(&file, "").unwrap();
        // Below a file: not "absent", an error.
        let err = has_task_files(&file.join("store")).unwrap_err();
        assert!(
            err.to_string().starts_with(&format!(
                "could not read {}: ",
                file.join("store").display()
            )),
            "{err}"
        );
    }

    #[test]
    fn a_directory_named_like_a_task_file_is_none() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("x.md")).unwrap();
        assert!(!has_task_files(root.path()).unwrap());
        std::fs::write(root.path().join("y.Md"), "").unwrap();
        assert!(has_task_files(root.path()).unwrap());
    }
}
