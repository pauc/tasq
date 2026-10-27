//! Running a program with an injected environment.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Runs `program` as [`run`] does, killing it once it has run for
/// `limit`: then it failed, with `<program> timed out after <n> s` as
/// `stderr`. What it printed until then is dropped: a child it left
/// behind (ssh) may hold its streams open for longer.

// Reason: wraps `std::process::Command` and a clock; the timeout is tested
// through the job (`tests/sync.rs`).
#[mutants::skip]
pub fn run_within<A: AsRef<OsStr>>(
    program: &Path,
    args: &[A],
    cwd: &Path,
    env: &Env,
    limit: Duration,
) -> Finished {
    let spawned = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            return Finished {
                success: false,
                code: None,
                stdout: String::new(),
                stderr: format!("could not run {}: {err}", program.display()),
            };
        }
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Finished {
                    success: status.success(),
                    code: status.code(),
                    stdout: stdout.join().unwrap_or_default(),
                    stderr: stderr.join().unwrap_or_default(),
                };
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Finished {
                    success: false,
                    code: None,
                    stdout: String::new(),
                    stderr: format!(
                        "{} timed out after {} s",
                        program.display(),
                        limit.as_secs()
                    ),
                };
            }
        }
    }
}

/// Reads `stream` to its end on a thread of its own, so that a child
/// filling one pipe never blocks on it while the other is read.
fn drain<R: Read + Send + 'static>(stream: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// What every `git` the plugin runs is configured with, over the user's
/// own configuration: no commit signing (no pinentry or passphrase can be
/// answered in the background, and these are bookkeeping commits) and no
/// hooks (a global `pre-commit` or `pre-push` is about code, not task
/// files). As `GIT_CONFIG_COUNT` variables, so the rebase run under `tasq
/// store lock` gets them too.
const STORE_CONFIG: [(&str, &str); 2] =
    [("commit.gpgSign", "false"), ("core.hooksPath", "/dev/null")];

/// [`STORE_CONFIG`] as the environment variables git reads it from.
fn store_config() -> Env {
    let mut env: Env = vec![(
        "GIT_CONFIG_COUNT".into(),
        STORE_CONFIG.len().to_string().into(),
    )];
    for (i, (key, value)) in STORE_CONFIG.iter().enumerate() {
        env.push((format!("GIT_CONFIG_KEY_{i}").into(), (*key).into()));
        env.push((format!("GIT_CONFIG_VALUE_{i}").into(), (*value).into()));
    }
    env
}

/// What a foreground git command (`tasq git sync`, init, clone) runs with:
/// `env` without the `GIT_*` variables, so that a direnv `GIT_DIR` or
/// `GIT_AUTHOR_NAME` cannot redirect it to another repository or identity,
/// and with `STORE_CONFIG`.
pub fn foreground_env(env: impl IntoIterator<Item = (OsString, OsString)>) -> Env {
    let mut out: Env = env
        .into_iter()
        .filter(|(key, _)| !key.as_encoded_bytes().starts_with(b"GIT_"))
        .collect();
    out.extend(store_config());
    out
}

/// The variables of the user's environment the background job keeps:
/// where git finds its global configuration and ssh its agent, the session
/// bus and runtime directory a credential helper (libsecret) talks to, and
/// the proxies git honours.
const BACKGROUND_VARS: [&str; 13] = [
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "SSH_AUTH_SOCK",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "http_proxy",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "no_proxy",
    "NO_PROXY",
];

/// What the background job runs `git` with: only the `BACKGROUND_VARS` of
/// `env`, `STORE_CONFIG`, and the settings that make any prompt fail
/// instead of waiting for a terminal nobody watches. git asks nothing on
/// the terminal, and ssh (passphrase, host key) asks `false` instead of
/// the terminal, which the detached job still has. No `GIT_SSH_COMMAND`,
/// so the repository's `core.sshCommand` applies.
pub fn background_env(env: impl IntoIterator<Item = (OsString, OsString)>) -> Env {
    let mut out: Env = env
        .into_iter()
        .filter(|(key, _)| BACKGROUND_VARS.contains(&key.to_str().unwrap_or("")))
        .collect();
    out.extend(store_config());
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

/// Starts `program` with `args` and exactly `env`, detached: its own
/// process group, so that tasq ending the plugin's group (a timeout, a
/// Ctrl-C) leaves it running, and every stream on `/dev/null`, so that
/// tasq does not wait for it. The working directory is this process's.

// Reason: wraps `std::process::Command::spawn`; the detach is tested
// through the hook (`tests/job.rs`).
#[mutants::skip]
pub fn spawn_detached(program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new(program)
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
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

    fn strings(env: Env) -> Vec<(String, String)> {
        env.into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect()
    }

    fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    const NO_SIGNING_NO_HOOKS: [(&str, &str); 5] = [
        ("GIT_CONFIG_COUNT", "2"),
        ("GIT_CONFIG_KEY_0", "commit.gpgSign"),
        ("GIT_CONFIG_VALUE_0", "false"),
        ("GIT_CONFIG_KEY_1", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_1", "/dev/null"),
    ];

    #[test]
    fn the_background_env_keeps_the_config_agent_bus_and_proxies_and_forbids_prompts() {
        let kept = [
            ("PATH", "/bin"),
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "/home/me/.cfg"),
            ("SSH_AUTH_SOCK", "/run/agent"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("http_proxy", "http://p:1"),
            ("https_proxy", "http://p:2"),
            ("HTTPS_PROXY", "http://p:3"),
            ("all_proxy", "http://p:4"),
            ("ALL_PROXY", "http://p:5"),
            ("no_proxy", "localhost"),
            ("NO_PROXY", "example.invalid"),
        ];
        let mut parent = kept.to_vec();
        parent.extend([
            ("GIT_AUTHOR_NAME", "Work"),
            ("GIT_SSH_COMMAND", "ssh -i work"),
            ("GIT_CONFIG_COUNT", "9"),
            ("GNUPGHOME", "/home/me/.gnupg"),
            ("LANG", "nl_BE.UTF-8"),
            ("TASQ_BIN", "/bin/tasq"),
        ]);
        let parent = parent
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let mut expected = kept.to_vec();
        expected.extend(NO_SIGNING_NO_HOOKS);
        expected.extend([
            ("GIT_TERMINAL_PROMPT", "0"),
            ("SSH_ASKPASS", "false"),
            ("SSH_ASKPASS_REQUIRE", "force"),
            ("LANG", "C"),
        ]);
        assert_eq!(strings(background_env(parent)), owned(&expected));
    }

    #[test]
    fn the_foreground_env_drops_git_variables_and_adds_the_store_config() {
        let parent = [
            ("PATH", "/bin"),
            ("GIT_DIR", "/elsewhere"),
            ("GIT_CONFIG_COUNT", "9"),
            ("GPG_TTY", "/dev/pts/1"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let mut expected = vec![("PATH", "/bin"), ("GPG_TTY", "/dev/pts/1")];
        expected.extend(NO_SIGNING_NO_HOOKS);
        assert_eq!(strings(foreground_env(parent)), owned(&expected));
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
