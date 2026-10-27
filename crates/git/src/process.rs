//! Running a program with an injected environment.

use std::ffi::{OsStr, OsString};
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
pub fn run<A: AsRef<OsStr>>(program: &Path, args: &[A], cwd: &Path, env: &Env) -> Finished {
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

/// What the background job runs `git` with: only `PATH`, `HOME` and
/// `SSH_AUTH_SOCK` of `env`, and the settings that make any prompt fail
/// instead of waiting for a terminal nobody watches. git asks nothing on
/// the terminal, and ssh (passphrase, host key) asks `false` instead of
/// the terminal, which the detached job still has. No `GIT_SSH_COMMAND`,
/// so the repository's `core.sshCommand` applies.
pub fn background_env(env: impl IntoIterator<Item = (OsString, OsString)>) -> Env {
    let mut out: Env = env
        .into_iter()
        .filter(|(key, _)| ["PATH", "HOME", "SSH_AUTH_SOCK"].contains(&key.to_str().unwrap_or("")))
        .collect();
    for (key, value) in [
        ("GIT_TERMINAL_PROMPT", "0"),
        ("SSH_ASKPASS", "false"),
        ("SSH_ASKPASS_REQUIRE", "force"),
        ("LANG", "C"),
    ] {
        out.push((key.into(), value.into()));
    }
    out
}

/// Starts `program` with `args`, detached: its own process group, so that
/// tasq ending the plugin's group (a timeout, a Ctrl-C) leaves it running,
/// and every stream on `/dev/null`, so that tasq does not wait for it.
/// The environment and working directory are this process's.

// Reason: wraps `std::process::Command::spawn`; the detach is tested
// through the hook (`tests/job.rs`).
#[mutants::skip]
pub fn spawn_detached(program: &Path, args: &[&OsStr]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_env_keeps_three_variables_and_forbids_prompts() {
        let parent = [
            ("PATH", "/bin"),
            ("HOME", "/home/me"),
            ("SSH_AUTH_SOCK", "/run/agent"),
            ("GIT_AUTHOR_NAME", "Work"),
            ("GIT_SSH_COMMAND", "ssh -i work"),
            ("LANG", "nl_BE.UTF-8"),
            ("TASQ_BIN", "/bin/tasq"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let env: Vec<(String, String)> = background_env(parent)
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect();
        let expected = [
            ("PATH", "/bin"),
            ("HOME", "/home/me"),
            ("SSH_AUTH_SOCK", "/run/agent"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("SSH_ASKPASS", "false"),
            ("SSH_ASKPASS_REQUIRE", "force"),
            ("LANG", "C"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        assert_eq!(env, expected);
    }

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
