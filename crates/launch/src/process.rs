//! Running external programs with an injected environment.

use std::path::{Path, PathBuf};
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

/// The value of `name` in `env`, when set and non-empty.
pub fn env_var<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// The first `PATH` entry of `env` holding an executable file called `name`.
pub fn which(env: &[(String, String)], name: &str) -> Option<PathBuf> {
    let path = env_var(env, "PATH")?;
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
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

/// Replaces the current process with `argv[0] argv[1..]` in `cwd`, with
/// `env` plus `extra` as the environment. Returns only when the exec fails.
/// On non-Unix hosts the program is run as a child and waited for.
///
/// Reason: wraps `exec`; a test cannot observe a replaced process.
#[mutants::skip]
pub fn exec(
    argv: &[String],
    cwd: &Path,
    env: &[(String, String)],
    extra: &[(String, String)],
) -> std::io::Error {
    let Some((program, rest)) = argv.split_first() else {
        return std::io::Error::other("empty command");
    };
    let mut command = Command::new(program);
    command.args(rest).current_dir(cwd).env_clear();
    for (k, v) in env {
        command.env(k, v);
    }
    for (k, v) in extra {
        command.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec()
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => e,
        }
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

    #[test]
    fn env_lookup_ignores_empty_values() {
        let env = vec![
            ("A".to_owned(), "1".to_owned()),
            ("EMPTY".to_owned(), String::new()),
        ];
        assert_eq!(env_var(&env, "A"), Some("1"));
        assert_eq!(env_var(&env, "EMPTY"), None);
        assert_eq!(env_var(&env, "B"), None);
    }

    #[test]
    fn which_searches_the_injected_path_only() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::write(&tool, "").unwrap();
        let env = vec![(
            "PATH".to_owned(),
            format!("/nonexistent:{}", dir.path().display()),
        )];
        assert_eq!(which(&env, "tool"), Some(tool));
        assert_eq!(which(&env, "other"), None);
        assert_eq!(which(&[], "tool"), None);
    }
}
