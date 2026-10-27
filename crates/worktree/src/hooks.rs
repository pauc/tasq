//! Worktree hooks (plan 4.5, WT-204): rendering them and running them.
//!
//! Every hook comes from the user's config directory, so every hook runs
//! (ADR-0031). The decisions are [`tasq_core::worktree::hooks`]'s.

use std::path::Path;

use tasq_core::worktree::hooks::{HookError, HookEvent, Hooks, Kind};
use tasq_core::worktree::vars::Vars;
use thiserror::Error;

use crate::process::{Finished, run};
use crate::settings::RepoSettings;

/// Why hooks could not be prepared or a blocking one failed.
#[derive(Debug, Error)]
pub enum HookFailure {
    /// A line does not render.
    #[error(transparent)]
    Line(#[from] HookError),
    /// A blocking (`pre-*`) hook failed: the message says which and why.
    #[error("{0}")]
    Blocking(String),
}

/// The hooks of `kinds` for a worktree of the repository `settings`
/// describes, rendered with `vars`.
///
/// # Errors
///
/// [`HookFailure::Line`] for a line that does not render.
pub fn prepare<const N: usize>(
    settings: &RepoSettings,
    kinds: [Kind; N],
    vars: &Vars,
) -> Result<[Hooks; N], HookFailure> {
    let mut failed = None;
    let all = kinds.map(|kind| {
        Hooks::render(kind, &settings.config.hooks, vars).unwrap_or_else(|e| {
            failed.get_or_insert(e);
            Hooks {
                kind,
                run: Vec::new(),
            }
        })
    });
    match failed {
        Some(e) => Err(e.into()),
        None => Ok(all),
    }
}

/// Runs `hooks` in `cwd` with exactly `env`. A failure of a blocking
/// (`pre-*`) hook stops the rest and is the error; any other failure is a
/// warning, reported with the hooks' output.
///
/// # Errors
///
/// [`HookFailure::Blocking`] when a blocking hook fails.
pub fn run_all(
    hooks: &Hooks,
    cwd: &Path,
    env: &[(String, String)],
    report: &mut dyn FnMut(HookEvent),
) -> Result<(), HookFailure> {
    let key = hooks.kind.key();
    for command in &hooks.run {
        match run_hook(&command.argv, cwd, env) {
            Ok(output) if output.is_empty() => {}
            Ok(output) => report(HookEvent::Output(format!(
                "{key} hook {:?}: {output}",
                command.line
            ))),
            Err(message) => {
                let message = format!("{key} hook {:?} failed: {message}", command.line);
                if hooks.kind.blocking() {
                    return Err(HookFailure::Blocking(message));
                }
                report(HookEvent::Warning(message));
            }
        }
    }
    Ok(())
}

/// Runs `argv` in `cwd` with exactly `env`. `Ok` carries the output
/// (stdout, then stderr, trimmed); `Err` says why it failed: the exit
/// status and what the hook printed, or why it could not start.
pub fn run_hook(argv: &[String], cwd: &Path, env: &[(String, String)]) -> Result<String, String> {
    let Some((program, rest)) = argv.split_first() else {
        return Err("empty command".to_owned());
    };
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    let finished = run(program, &words, cwd, env);
    if finished.success {
        Ok(output(&finished))
    } else {
        Err(failure(&finished))
    }
}

/// Everything a successful hook printed, trimmed.
fn output(finished: &Finished) -> String {
    [finished.stdout.trim(), finished.stderr.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Why a hook failed, as one message.
fn failure(finished: &Finished) -> String {
    let message = finished.message();
    match (finished.code, message.is_empty()) {
        (Some(code), true) => format!("exit status {code}"),
        (Some(code), false) => format!("exit status {code}: {message}"),
        (None, true) => "did not run to completion".to_owned(),
        (None, false) => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(code: Option<i32>, stdout: &str, stderr: &str) -> Finished {
        Finished {
            success: code == Some(0),
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn outputs_and_failures() {
        assert_eq!(output(&finished(Some(0), " out \n", "")), "out");
        assert_eq!(output(&finished(Some(0), "out\n", "err\n")), "out\nerr");
        assert_eq!(output(&finished(Some(0), "", "")), "");
        assert_eq!(failure(&finished(Some(3), "", "")), "exit status 3");
        assert_eq!(
            failure(&finished(Some(3), "", "boom\n")),
            "exit status 3: boom"
        );
        assert_eq!(
            failure(&finished(None, "", "")),
            "did not run to completion"
        );
        assert_eq!(
            failure(&finished(None, "", "could not run x: not found")),
            "could not run x: not found"
        );
    }

    #[test]
    fn runs_in_the_directory_with_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let env = vec![("PATH".to_owned(), path), ("X".to_owned(), "y".to_owned())];
        let sh = |script: &str| {
            run_hook(
                &["sh".to_owned(), "-c".to_owned(), script.to_owned()],
                dir.path(),
                &env,
            )
        };
        assert_eq!(
            sh("echo $X; pwd"),
            // `pwd` prints the physical path (macOS: /var is /private/var).
            Ok(format!(
                "y\n{}",
                dir.path().canonicalize().unwrap().display()
            ))
        );
        assert_eq!(
            sh("echo no >&2; exit 2"),
            Err("exit status 2: no".to_owned())
        );
        assert_eq!(
            run_hook(&[], dir.path(), &env),
            Err("empty command".to_owned())
        );
        let missing = run_hook(&["/definitely/not/here".to_owned()], dir.path(), &env);
        assert!(
            missing
                .as_ref()
                .is_err_and(|e| e.starts_with("could not run /definitely/not/here")),
            "{missing:?}"
        );
    }
}
