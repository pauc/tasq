//! `tasq git init [<url>]`: makes the store a repository with one commit of
//! its task files and the store's identity in its own config, and, given a
//! URL, pushes it to a branch of that remote named after the store.
//!
//! Setup never merges two task histories: a remote branch that already
//! exists is refused before the store is touched. A failure after `git
//! init` removes the `.git` it made, so init can simply be run again.

use std::path::{Path, PathBuf};

use crate::git::{Git, GitError, RemoteBranch, TASK_FILES};
use crate::hook::is_repository;
use crate::tasq::{Store, TasqError};

/// The subject of the first commit.
const SUBJECT: &str = "tasq git init";

/// The identity the store commits and pushes as. A field left `None` is
/// not written, so git's own resolution applies to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    /// `user.name`.
    pub name: Option<String>,
    /// `user.email`.
    pub email: Option<String>,
    /// The private key for `core.sshCommand`.
    pub ssh_key: Option<String>,
}

impl Identity {
    /// This identity, each field it lacks taken from `[plugin.git]`
    /// (`name`, `email`, `ssh_key`).
    pub fn or_defaults(
        self,
        config: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, InitError> {
        let default = |key: &str| -> Result<Option<String>, InitError> {
            match config.get(key) {
                None => Ok(None),
                Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
                Some(other) => Err(InitError::NotAString {
                    key: key.to_owned(),
                    value: other.to_string(),
                }),
            }
        };
        Ok(Self {
            name: self.name.map_or_else(|| default("name"), |v| Ok(Some(v)))?,
            email: self
                .email
                .map_or_else(|| default("email"), |v| Ok(Some(v)))?,
            ssh_key: self
                .ssh_key
                .map_or_else(|| default("ssh_key"), |v| Ok(Some(v)))?,
        })
    }

    /// The `(key, value)` pairs written into the repository config.
    fn settings(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![];
        if let Some(name) = &self.name {
            out.push(("user.name", name.clone()));
        }
        if let Some(email) = &self.email {
            out.push(("user.email", email.clone()));
        }
        if let Some(key) = &self.ssh_key {
            out.push(("core.sshCommand", ssh_command(key)));
        }
        out
    }
}

/// What `tasq git init` was asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// The remote to push to, if any.
    pub url: Option<String>,
    /// The branch, local and remote; the store name when `None`.
    pub branch: Option<String>,
    /// The identity, flags and `[plugin.git]` defaults merged.
    pub identity: Identity,
}

/// Why init did nothing (or undid what it did).
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// tasq could not say which store, or what its configuration is.
    #[error(transparent)]
    Tasq(#[from] TasqError),
    /// A `[plugin.git]` default of the wrong type.
    #[error("[plugin.git] {key} must be a string, not {value}")]
    NotAString {
        /// The key.
        key: String,
        /// Its value, as JSON.
        value: String,
    },
    /// The store is a repository already.
    #[error("{} is already a git repository", .0.display())]
    Repository(PathBuf),
    /// The remote branch has commits: another store's history.
    #[error(
        "branch {branch} of {url} already has history; tasq git init never merges two task \
         histories\nfix: tasq git clone {url} --branch {branch} into an empty store, or tasq git \
         init {url} --branch <another> to push to a new branch"
    )]
    BranchHasHistory {
        /// The remote.
        url: String,
        /// The branch.
        branch: String,
    },
    /// `git ls-remote` could not ask the remote.
    #[error("could not reach {url}: {message}")]
    Unreachable {
        /// The remote.
        url: String,
        /// git's message.
        message: String,
    },
    /// A git step failed; the repository was removed again.
    #[error(transparent)]
    Git(#[from] GitError),
    /// The store directory could not be created, or the failed repository
    /// removed.
    #[error("{action} {}: {source}", .path.display())]
    Io {
        /// What was being done.
        action: &'static str,
        /// To which path.
        path: PathBuf,
        /// The OS error.
        source: std::io::Error,
    },
}

/// What init did, one line each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The store directory.
    pub store: PathBuf,
    /// The branch it is on.
    pub branch: String,
    /// How many task files the first commit holds.
    pub committed: usize,
    /// The remote it was pushed to.
    pub pushed: Option<String>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let files = if self.committed == 1 { "file" } else { "files" };
        writeln!(
            f,
            "initialized {} as a git repository on branch {}: {} task {files} committed",
            self.store.display(),
            self.branch,
            self.committed
        )?;
        if let Some(url) = &self.pushed {
            writeln!(f, "pushed branch {} to {url}", self.branch)?;
        }
        Ok(())
    }
}

/// Makes `store` a repository as `options` say.
pub fn init(git: &Git, store: &Store, options: &Options) -> Result<Report, InitError> {
    let dir = &store.location;
    if is_repository(dir) {
        return Err(InitError::Repository(dir.clone()));
    }
    let branch = options.branch.clone().unwrap_or_else(|| store.name.clone());
    if let Some(url) = &options.url {
        match git.remote_branch(nearest_existing(dir), url, &branch) {
            RemoteBranch::Absent => {}
            RemoteBranch::Present => {
                return Err(InitError::BranchHasHistory {
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
    }
    std::fs::create_dir_all(dir).map_err(|source| InitError::Io {
        action: "could not create the store directory",
        path: dir.clone(),
        source,
    })?;
    match build(git, dir, &branch, options) {
        Ok(committed) => Ok(Report {
            store: dir.clone(),
            branch,
            committed,
            pushed: options.url.clone(),
        }),
        Err(err) => {
            // Nothing to undo when git never ran.
            let made = dir.join(".git");
            if made.exists() {
                std::fs::remove_dir_all(&made).map_err(|source| InitError::Io {
                    action: "init failed and could not remove",
                    path: made,
                    source,
                })?;
            }
            Err(err.into())
        }
    }
}

/// `git init` and everything after it; returns the number of task files
/// committed.
fn build(git: &Git, dir: &Path, branch: &str, options: &Options) -> Result<usize, GitError> {
    git.check(dir, &["init", "--quiet", "--initial-branch", branch])?;
    for (key, value) in options.identity.settings() {
        git.check(dir, &["config", "--local", key, &value])?;
    }
    let mut list = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    list.extend(TASK_FILES);
    let listed = git.check(dir, &list)?;
    let files = listed.split('\0').filter(|f| !f.is_empty()).count();
    if files > 0 {
        // With no match, `git add` fails on the pathspec.
        let mut add = vec!["add", "--all", "--"];
        add.extend(TASK_FILES);
        git.check(dir, &add)?;
    }
    git.check(dir, &["commit", "--quiet", "--allow-empty", "-m", SUBJECT])?;
    if let Some(url) = &options.url {
        git.check(dir, &["remote", "add", "origin", url])?;
        git.check(
            dir,
            &["push", "--quiet", "--set-upstream", "origin", branch],
        )?;
    }
    Ok(files)
}

/// `core.sshCommand` for `key`: only that key is offered, even when the SSH
/// agent holds others. The path is single-quoted for the shell git runs it
/// with; ssh itself expands a leading `~`.
pub fn ssh_command(key: &str) -> String {
    format!(
        "ssh -i '{}' -o IdentitiesOnly=yes",
        key.replace('\'', r"'\''")
    )
}

/// `dir`, or its nearest ancestor that exists: where to run a git command
/// that needs no repository before the store directory is created.
fn nearest_existing(dir: &Path) -> &Path {
    dir.ancestors()
        .find(|d| d.is_dir())
        .unwrap_or_else(|| Path::new("/"))
}
