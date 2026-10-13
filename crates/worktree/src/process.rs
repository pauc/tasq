//! Running `git` with an injected environment.

use std::path::Path;
use std::process::Command;

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
    /// `stderr`, else `stdout`, trimmed: the line to show when a tool fails.
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
/// environment. Failing to start the program at all (not on `PATH`) is
/// reported like a failed run, with the OS error as `stderr`.

// Reason: wraps `std::process::Command`; nothing to assert without a process.
#[mutants::skip]
pub fn run(program: &str, args: &[&str], cwd: &Path, env: &[(String, String)]) -> Finished {
    let mut command = Command::new(program);
    command.args(args).env_clear().current_dir(cwd);
    for (k, v) in env {
        command.env(k, v);
    }
    match command.output() {
        Ok(output) => Finished {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(e) => Finished {
            success: false,
            code: None,
            stdout: String::new(),
            stderr: format!("could not run {program}: {e}"),
        },
    }
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
