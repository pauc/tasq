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

/// Where a launch records the commands it runs and the warnings it gives,
/// for debugging a launch that went wrong: stderr at `-v`, and the
/// `launch.log` file when one is configured. The default records nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trace {
    /// Echo every record to stderr (`-v`).
    pub stderr: bool,
    /// Append every record and warning to this file (`launch.log`).
    pub file: Option<PathBuf>,
}

impl Trace {
    /// Records that `program args` ran and how it finished.
    pub fn command(&self, program: &str, args: &[&str], out: &Finished) {
        self.write(&command_record(program, args, out), true);
    }

    /// Records a line of context (which task, which launcher, the outcome).
    pub fn note(&self, line: &str) {
        self.write(&format!("{line}\n"), true);
    }

    /// Prints `tasq: warning: <message>` on stderr, always, and appends it
    /// to the log file.
    #[expect(
        clippy::print_stderr,
        reason = "warnings always reach the terminal, even without `-v`"
    )]
    pub fn warn(&self, message: &str) {
        eprintln!("tasq: warning: {message}");
        self.write(&format!("warning: {message}\n"), false);
    }

    /// Appends `text` to the file, and prints it on stderr when `echo` and
    /// `-v`. A log file that cannot be written is ignored: the trace must
    /// never be what breaks a launch.
    ///
    /// Reason: wraps stderr and a file append; [`command_record`] holds the logic.
    #[mutants::skip]
    #[expect(clippy::print_stderr, reason = "`-v` traces go to the terminal")]
    fn write(&self, text: &str, echo: bool) {
        use std::io::Write;
        if echo && self.stderr {
            eprint!("{text}");
        }
        if let Some(path) = &self.file {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(text.as_bytes());
            }
        }
    }
}

/// The trace record of a finished run: the command line, shell-quoted,
/// whether it succeeded, then each non-empty stream with its lines
/// indented.
pub fn command_record(program: &str, args: &[&str], out: &Finished) -> String {
    use std::fmt::Write;
    let mut text = format!(
        "$ {}\n  {}\n",
        shell_words::join(std::iter::once(program).chain(args.iter().copied())),
        if out.success { "ok" } else { "failed" }
    );
    for (name, stream) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
        let stream = stream.trim_end();
        if stream.is_empty() {
            continue;
        }
        let _ = writeln!(text, "  {name}:");
        for line in stream.lines() {
            let _ = writeln!(text, "    {line}");
        }
    }
    text
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

/// Runs `argv` with `input` on its stdin and exactly `env` as its
/// environment, from the current directory. A program that cannot be
/// started is reported like a failed run, with the OS error as `stderr`.
///
/// Reason: wraps `std::process::Command`; nothing to assert without a process.
#[mutants::skip]
pub fn run_with_input(argv: &[String], input: &str, env: &[(String, String)]) -> Finished {
    use std::io::Write;
    use std::process::Stdio;

    let Some((program, rest)) = argv.split_first() else {
        return Finished {
            success: false,
            stdout: String::new(),
            stderr: "empty command".to_owned(),
        };
    };
    let mut command = Command::new(program);
    command
        .args(rest)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Finished {
                success: false,
                stdout: String::new(),
                stderr: format!("could not run {program}: {e}"),
            };
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        // The program may exit without reading everything; not our error.
        let _ = stdin.write_all(input.as_bytes());
    }
    match child.wait_with_output() {
        Ok(output) => Finished::from_output(&output),
        Err(e) => Finished {
            success: false,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

/// Replaces the current process with `argv[0] argv[1..]` in `cwd`, with
/// `env` plus `extra` as the environment. Returns only when the exec fails,
/// back in the caller's working directory.
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
        // `exec` changes into `cwd` in this process before `execvp` and does
        // not change back when it fails, so restore the caller's directory.
        let previous = std::env::current_dir();
        let err = command.exec();
        if let Ok(dir) = previous {
            let _ = std::env::set_current_dir(dir);
        }
        err
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
    fn command_record_quotes_the_command_and_indents_the_streams() {
        let failed = Finished {
            success: false,
            stdout: "{\"ok\":false}\n".into(),
            stderr: "error: no such pane\nhint: retry\n\n".into(),
        };
        assert_eq!(
            command_record("herdr", &["tab", "create", "--label", "Fix it"], &failed),
            "$ herdr tab create --label 'Fix it'\n  failed\n  stdout:\n    {\"ok\":false}\n  \
             stderr:\n    error: no such pane\n    hint: retry\n"
        );
        let quiet = Finished {
            success: true,
            stdout: " \n".into(),
            stderr: String::new(),
        };
        assert_eq!(command_record("tmux", &[], &quiet), "$ tmux\n  ok\n");
    }

    #[test]
    fn trace_appends_records_notes_and_warnings_to_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs/launch.log");
        let trace = Trace {
            stderr: false,
            file: Some(path.clone()),
        };
        let ok = Finished {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        };
        trace.note("task 3");
        trace.command("herdr", &["agent", "focus", "task-3"], &ok);
        trace.warn("focus failed");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "task 3\n$ herdr agent focus task-3\n  ok\nwarning: focus failed\n"
        );
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
