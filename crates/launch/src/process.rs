//! Running external programs with an injected environment.

use std::path::Path;
use std::process::{Command, Output};

/// The outcome of a finished process, with its streams as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Whether the exit status was zero.
    pub success: bool,
    /// Standard output, lossily decoded.
    pub stdout: String,
    /// Standard error, lossily decoded.
    pub stderr: String,
}

impl Finished {
    fn from_output(output: &Output) -> Self {
        Self {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

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

/// Runs `program` with `args` in `cwd`, with exactly `env` (plus `extra`)
/// as its environment. Failing to start the program at all (not on `PATH`)
/// is reported like a failed run, with the OS error as `stderr`.
///
/// Reason: wraps `std::process::Command`; nothing to assert without a process.
#[mutants::skip]
pub fn run(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    env: &[(String, String)],
    extra: &[(&str, &str)],
) -> Finished {
    let mut command = Command::new(program);
    command.args(args).env_clear();
    for (k, v) in env {
        command.env(k, v);
    }
    for (k, v) in extra {
        command.env(k, v);
    }
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    match command.output() {
        Ok(output) => Finished::from_output(&output),
        Err(e) => Finished {
            success: false,
            stdout: String::new(),
            stderr: format!("could not run {program}: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_prefers_stderr() {
        let f = Finished {
            success: false,
            stdout: "out\n".into(),
            stderr: " err \n".into(),
        };
        assert_eq!(f.message(), "err");
        let f = Finished {
            success: false,
            stdout: "out\n".into(),
            stderr: "  \n".into(),
        };
        assert_eq!(f.message(), "out");
    }
}
