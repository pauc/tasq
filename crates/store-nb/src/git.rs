//! Running `git` inside the notebook, with an injected environment, for
//! the native bookkeeper and the diagnostics.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

/// Why running `git` failed.
#[derive(Debug, Error)]
pub enum GitError {
    /// `git` could not be started (not installed, no `PATH`).
    #[error("cannot run git: {source}")]
    Spawn {
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// `git` exited with a failure status.
    #[error("git {args} failed{}: {stderr}", status.map(|s| format!(" with status {s}")).unwrap_or_default())]
    Failed {
        /// The arguments, space separated.
        args: String,
        /// The exit status, when the process exited normally.
        status: Option<i32>,
        /// Trimmed standard error.
        stderr: String,
    },
}

/// A notebook directory to run `git -C <dir>` in, with the environment the
/// process gets (`HOME` for the identity, `PATH` to find `git`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
}

impl Git {
    /// Git commands for the repository at `dir`.
    pub fn new(dir: impl Into<PathBuf>, env: Vec<(String, String)>) -> Self {
        Self {
            dir: dir.into(),
            env,
        }
    }

    /// The directory commands run in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether `dir` is the top of a git repository (has a `.git` entry).
    /// Nested notebooks inside a bigger repository are deliberately not
    /// detected: nb only commits notebooks that are repositories themselves.
    pub fn is_repository(&self) -> bool {
        self.dir.join(".git").exists()
    }

    /// Runs `git -C <dir> <args>` and returns its standard output. Standard
    /// input is closed so git never prompts.
    ///
    /// Reason: spawns a process; its effects are asserted by the integration
    /// tests on a temporary repository.
    #[mutants::skip]
    pub fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| GitError::Spawn { source })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(GitError::Failed {
                args: args.join(" "),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    /// Whether the working tree has uncommitted changes (`git status
    /// --porcelain` prints something).
    pub fn is_dirty(&self) -> Result<bool, GitError> {
        self.run(&["status", "--porcelain"])
            .map(|out| !out.trim().is_empty())
    }

    /// The configured remotes (`git remote`), one name per entry.
    pub fn remotes(&self) -> Result<Vec<String>, GitError> {
        self.run(&["remote"]).map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
    }

    /// `user.name` and `user.email` as git resolves them here, `None` for a
    /// value that is not set.
    pub fn identity(&self) -> Result<(Option<String>, Option<String>), GitError> {
        Ok((
            self.config_value("user.name")?,
            self.config_value("user.email")?,
        ))
    }

    /// `git config --get <key>`; `None` when the key is unset (git exits 1).
    fn config_value(&self, key: &str) -> Result<Option<String>, GitError> {
        match self.run(&["config", "--get", key]) {
            Ok(value) => {
                let value = value.trim();
                Ok((!value.is_empty()).then(|| value.to_owned()))
            }
            Err(GitError::Failed {
                status: Some(1), ..
            }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &Path) -> Vec<(String, String)> {
        vec![
            ("HOME".to_owned(), home.display().to_string()),
            ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ]
    }

    fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let git = Git::new(&work, env(&home));
        assert!(!git.is_repository());
        git.run(&["init", "-q"]).unwrap();
        assert!(git.is_repository());
        (dir, git)
    }

    #[test]
    fn error_messages() {
        let e = GitError::Failed {
            args: "push".into(),
            status: Some(128),
            stderr: "no remote".into(),
        };
        assert_eq!(e.to_string(), "git push failed with status 128: no remote");
        let e = GitError::Failed {
            args: "x".into(),
            status: None,
            stderr: String::new(),
        };
        assert_eq!(e.to_string(), "git x failed: ");
        let e = GitError::Spawn {
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        };
        assert_eq!(e.to_string(), "cannot run git: gone");
    }

    #[test]
    fn new_keeps_the_directory() {
        let g = Git::new("/repo", Vec::new());
        assert_eq!(g.dir(), Path::new("/repo"));
    }

    #[test]
    fn dirty_remotes_and_identity_on_a_fresh_repository() {
        let (_tmp, git) = repo();
        assert!(!(git.is_dirty().unwrap()));
        std::fs::write(git.dir().join("f"), "x").unwrap();
        assert!(git.is_dirty().unwrap());
        assert_eq!(git.remotes().unwrap(), Vec::<String>::new());
        git.run(&["remote", "add", "origin", "/nowhere"]).unwrap();
        git.run(&["remote", "add", "backup", "/elsewhere"]).unwrap();
        assert_eq!(git.remotes().unwrap(), vec!["backup", "origin"]);
        // HOME has no .gitconfig: both unset.
        assert_eq!(git.identity().unwrap(), (None, None));
        git.run(&["config", "user.name", "Tasq Tests"]).unwrap();
        assert_eq!(
            git.identity().unwrap(),
            (Some("Tasq Tests".to_owned()), None)
        );
        git.run(&["config", "user.email", "t@example.invalid"])
            .unwrap();
        assert_eq!(
            git.identity().unwrap(),
            (
                Some("Tasq Tests".to_owned()),
                Some("t@example.invalid".to_owned())
            )
        );
    }

    #[test]
    fn failures_carry_git_stderr() {
        let (_tmp, git) = repo();
        let err = git.run(&["no-such-command"]).unwrap_err();
        let text = err.to_string();
        assert!(
            text.starts_with("git no-such-command failed with status 1: git:"),
            "{text}"
        );
        // A non-repository directory: status fails with 128, not "unset".
        let plain = Git::new(git.dir().join("missing"), Vec::new());
        assert!(matches!(
            plain.config_value("user.name").unwrap_err(),
            GitError::Failed { .. }
        ));
    }
}
