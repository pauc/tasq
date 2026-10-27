//! Running a program with an injected environment.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// An environment: every variable a child process gets, and nothing else.
pub type Env = Vec<(OsString, OsString)>;

/// The outcome of a finished process, with its streams as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Whether the exit status was zero.
    pub success: bool,
    /// The exit code, when the process exited normally.
    pub code: Option<i32>,
    /// Standard output, lossily decoded.
    pub stdout: String,
    /// Standard error, lossily decoded.
    pub stderr: String,
}

impl Finished {
    /// `stderr`, else `stdout`, trimmed: the line to show when it failed.
    pub fn message(&self) -> String {
        let err = self.stderr.trim();
        if err.is_empty() {
            self.stdout.trim().to_owned()
        } else {
            err.to_owned()
        }
    }
}

/// Runs `program` with `args` in `cwd`, with exactly `env` as its
/// environment and no stdin. Failing to start it at all is reported like a
/// failed run, with the OS error as `stderr`.

// Reason: wraps `std::process::Command`; nothing to assert without a process.
#[mutants::skip]
pub fn run(program: &Path, args: &[&str], cwd: &Path, env: &Env) -> Finished {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(std::process::Stdio::null());
    match command.output() {
        Ok(output) => Finished {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(err) => Finished {
            success: false,
            code: None,
            stdout: String::new(),
            stderr: format!("could not run {}: {err}", program.display()),
        },
    }
}

/// `env` without the `GIT_*` variables: what a foreground git command runs
/// with, so that a direnv `GIT_DIR` or `GIT_AUTHOR_NAME` cannot redirect it
/// to another repository or identity.
pub fn without_git_vars(env: impl IntoIterator<Item = (OsString, OsString)>) -> Env {
    env.into_iter()
        .filter(|(key, _)| !key.as_encoded_bytes().starts_with(b"GIT_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_prefers_stderr() {
        let mut f = Finished {
            success: false,
            code: Some(1),
            stdout: " out \n".into(),
            stderr: " err \n".into(),
        };
        assert_eq!(f.message(), "err");
        f.stderr = "  \n".into();
        assert_eq!(f.message(), "out");
    }
}
