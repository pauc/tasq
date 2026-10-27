//! `git`, with the environment every call gets.

use std::path::Path;

use crate::process::{self, Env, Finished};

/// The exit code of `git ls-remote --exit-code` when the remote answered
/// and has no matching ref.
const NO_MATCHING_REFS: i32 = 2;

/// The pathspecs of a store's task files, as tasq reads them: `.md` in
/// any case, at the top of the store, no leading dot (editor locks, and
/// `.tasq/` below it).
pub const TASK_FILES: [&str; 2] = [":(glob,icase)*.md", ":(exclude,glob).*"];

/// A git command that exited with an error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("git {command} failed: {message}")]
pub struct GitError {
    /// The subcommand (`commit`, `push`, ...).
    pub command: String,
    /// What git printed, trimmed.
    pub message: String,
}

/// What `git ls-remote` says about one branch of a remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteBranch {
    /// The remote answered and has no such branch.
    Absent,
    /// The branch exists, so it has at least one commit.
    Present,
    /// The remote could not be asked; git's message.
    Unreachable(String),
}

/// Runs `git` with exactly the environment it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    /// The environment of every `git` process.
    pub env: Env,
}

impl Git {
    /// `git <args>` in `dir`, whatever its outcome.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Finished {
        process::run(Path::new("git"), args, dir, &self.env)
    }

    /// `git <args>` in `dir`; its stdout, or an error naming the
    /// subcommand (`args[0]`) and git's message.
    pub fn check(&self, dir: &Path, args: &[&str]) -> Result<String, GitError> {
        let out = self.run(dir, args);
        if out.success {
            Ok(out.stdout)
        } else {
            Err(GitError {
                command: args.first().copied().unwrap_or_default().to_owned(),
                message: out.message(),
            })
        }
    }

    /// Whether `url` has a branch `branch`, asked from `dir` (any existing
    /// directory: the URL is given, no repository is needed).
    pub fn remote_branch(&self, dir: &Path, url: &str, branch: &str) -> RemoteBranch {
        let heads = format!("refs/heads/{branch}");
        let out = self.run(dir, &["ls-remote", "--exit-code", "--heads", url, &heads]);
        if out.success {
            RemoteBranch::Present
        } else if out.code == Some(NO_MATCHING_REFS) {
            RemoteBranch::Absent
        } else {
            RemoteBranch::Unreachable(out.message())
        }
    }
}
