//! Worktree hooks (plan 4.5, WT-204): which lines may run, asking for
//! approval, the trust store on disk, and running them.
//!
//! Hooks from the global configuration (or `--config`, `--set`, the
//! environment) run without asking. Hooks from a repository's own
//! `.tasq.toml`, which can arrive with a clone, run only once approved:
//! stored in the trust file, `--yes` for one run, or a yes to the question
//! the caller asks. Unapproved, they are skipped with a warning. The
//! decisions are [`tasq_core::worktree::hooks`]'s.

use std::io;
use std::path::{Path, PathBuf};

use tasq_core::worktree::hooks::{
    self, Approve, HookError, HookEvent, Hooks, Kind, TrustStore, approval,
};
use tasq_core::worktree::vars::Vars;
use thiserror::Error;

use crate::process::{Finished, run};
use crate::settings::RepoSettings;

/// How lines that need approval and have none are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// `--yes`: approved for this run.
    Given,
    /// Asked with [`Approver::ask`].
    Ask,
    /// Never asked (the TUI owns the terminal): skipped with a warning.
    Skip,
}

impl Approval {
    /// `Given` for `--yes`, else `Ask`.
    pub fn from_yes(yes: bool) -> Self {
        if yes { Self::Given } else { Self::Ask }
    }
}

/// How the lines that need approval get it.
#[derive(Debug, Clone)]
pub struct Approver {
    /// What to do about lines with no approval.
    pub approval: Approval,
    /// Where approvals are kept; `None` when there is nowhere to keep them.
    pub trust_file: Option<PathBuf>,
    /// Asks a yes/no question ([`Approval::Ask`]); the caller owns the
    /// terminal.
    pub ask: fn(&str) -> io::Result<bool>,
}

/// Why hooks could not be prepared or a blocking one failed.
#[derive(Debug, Error)]
pub enum HookFailure {
    /// A line does not render.
    #[error(transparent)]
    Line(#[from] HookError),
    /// A blocking (`pre-*`) hook failed: the message says which and why.
    #[error("{0}")]
    Blocking(String),
    /// There is no trust file to read approvals from.
    #[error("cannot tell where to keep hook approvals: HOME is not set")]
    NoTrustFile,
    /// The trust file cannot be read, parsed or written.
    #[error("{}: {message}", .path.display())]
    TrustFile {
        /// The trust file.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// Asking failed.
    #[error(transparent)]
    Ask(#[from] io::Error),
}

/// The hooks of `kinds` for a worktree of the repository `settings`
/// describes, rendered with `vars`. Lines from the repository's own file
/// that are not approved yet are approved for this run
/// ([`Approval::Given`]), asked about ([`Approval::Ask`]; a yes is
/// stored), or skipped with a warning; a dry run neither asks nor stores
/// nor warns.
///
/// # Errors
///
/// [`HookFailure`] for a line that does not render, an unreadable trust
/// file, or a question that cannot be asked.
pub fn prepare<const N: usize>(
    settings: &RepoSettings,
    kinds: [Kind; N],
    vars: &Vars,
    approver: &Approver,
    dry_run: bool,
    report: &mut dyn FnMut(HookEvent),
) -> Result<[Hooks; N], HookFailure> {
    let pending = pending(settings, &kinds, approver.trust_file.as_deref())?;
    let allowed = pending.is_empty()
        || match approver.approval {
            Approval::Given => true,
            Approval::Ask => approve(settings, &pending, dry_run, approver)?,
            Approval::Skip => false,
        };
    let mut failed = None;
    let all = kinds.map(|kind| {
        let skip = |line: &str| !allowed && pending.iter().any(|(k, l)| *k == kind && l == line);
        Hooks::render(kind, &settings.config.hooks, vars, skip).unwrap_or_else(|e| {
            failed.get_or_insert(e);
            Hooks {
                kind,
                run: Vec::new(),
                skipped: Vec::new(),
            }
        })
    });
    if let Some(e) = failed {
        return Err(e.into());
    }
    if !allowed && !dry_run {
        let file = settings.repo_file.as_deref().unwrap_or(Path::new(""));
        report(HookEvent::Warning(hooks::not_approved(
            file,
            &pending,
            &settings.main,
        )));
    }
    Ok(all)
}

/// The lines of `kinds` that come from the repository's file and are not
/// in the trust store.
fn pending(
    settings: &RepoSettings,
    kinds: &[Kind],
    trust_file: Option<&Path>,
) -> Result<Vec<(Kind, String)>, HookFailure> {
    let Some(file) = &settings.repo_file else {
        return Ok(Vec::new());
    };
    let store = read_store(trust_file)?;
    Ok(hooks::pending(
        &store,
        file,
        &settings.untrusted,
        kinds,
        &settings.config.hooks,
    ))
}

/// Asks whether the pending lines may run (never on a dry run); a yes
/// approves every hook line of the repository's file and stores that.
fn approve(
    settings: &RepoSettings,
    pending: &[(Kind, String)],
    dry_run: bool,
    approver: &Approver,
) -> Result<bool, HookFailure> {
    match approval(settings.repo_file.as_deref(), pending, dry_run) {
        Approve::Yes => Ok(true),
        Approve::No => Ok(false),
        Approve::Ask(question) => {
            let yes = (approver.ask)(&question)?;
            if yes {
                trust(settings, approver.trust_file.as_deref())?;
            }
            Ok(yes)
        }
    }
}

/// Stores every hook line the repository's file sets as approved, in
/// `trust_file`; the lines, by kind. Nothing to store without such a file.
///
/// # Errors
///
/// [`HookFailure::NoTrustFile`] or [`HookFailure::TrustFile`] when the
/// trust file cannot be read or written.
pub fn trust(
    settings: &RepoSettings,
    trust_file: Option<&Path>,
) -> Result<Vec<(Kind, String)>, HookFailure> {
    let Some(file) = &settings.repo_file else {
        return Ok(Vec::new());
    };
    let mut store = read_store(trust_file)?;
    let mut approved = Vec::new();
    for &kind in &settings.untrusted {
        let lines = kind.lines(&settings.config.hooks);
        store.approve(file, lines);
        approved.extend(lines.iter().map(|line| (kind, line.clone())));
    }
    if !approved.is_empty() {
        write_store(trust_file, &store)?;
    }
    Ok(approved)
}

fn read_store(trust_file: Option<&Path>) -> Result<TrustStore, HookFailure> {
    let file = trust_file.ok_or(HookFailure::NoTrustFile)?;
    let failed = |message: String| HookFailure::TrustFile {
        path: file.to_path_buf(),
        message,
    };
    match std::fs::read_to_string(file) {
        Ok(text) => TrustStore::parse(&text).map_err(|e| failed(e.to_string())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(TrustStore::default()),
        Err(e) => Err(failed(e.to_string())),
    }
}

fn write_store(trust_file: Option<&Path>, store: &TrustStore) -> Result<(), HookFailure> {
    let file = trust_file.ok_or(HookFailure::NoTrustFile)?;
    let failed = |e: io::Error| HookFailure::TrustFile {
        path: file.to_path_buf(),
        message: e.to_string(),
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    std::fs::write(file, store.to_toml()).map_err(failed)
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
