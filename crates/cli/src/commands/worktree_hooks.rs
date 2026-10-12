//! Worktree hooks in the CLI (plan 4.5, WT-204): which lines may run,
//! asking for approval, the trust store on disk, and running them.
//!
//! Hooks from the global configuration (or `--config`, `--set`, the
//! environment) run without asking. Hooks from a repository's own
//! `.tasq.toml`, which can arrive with a clone, run only once approved:
//! stored in the trust file, `--yes` for one run, or a yes on the terminal.
//! Unapproved, they are skipped with a warning.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use tasq_core::worktree::hooks::{self, Kind, TrustStore};
use tasq_core::worktree::vars::Vars;
use tasq_worktree::hooks::run_hook;

use crate::app::App;
use crate::commands::worktree::RepoSettings;
use crate::error::{CliError, Result};
use crate::plugins::HookEvent;

/// How lines that need approval and have none are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// `--yes`: approved for this run.
    Given,
    /// Asked on the terminal, when there is one.
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

/// Prints a hook event the way the CLI does: warnings always, a hook's
/// output at `-v`.
pub fn to_terminal(app: &App) -> impl FnMut(HookEvent) + '_ {
    |event| match event {
        HookEvent::Warning(text) => app.out.warn(&text),
        HookEvent::Output(text) => app.out.verbose(&text),
    }
}

/// One hook line, rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The line as configured.
    pub line: String,
    /// The program and its arguments.
    pub argv: Vec<String>,
}

/// The hooks of one kind for one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hooks {
    /// When they run.
    pub kind: Kind,
    /// What runs.
    pub run: Vec<Command>,
    /// Lines left out because nobody approved them.
    pub skipped: Vec<String>,
}

impl Hooks {
    /// `--dry-run` lines: what would run, and what would be skipped.
    pub fn describe(&self) -> Vec<String> {
        let key = self.kind.key();
        let run = self
            .run
            .iter()
            .map(|c| format!("run {key} hook: {}", shell_words::join(&c.argv)));
        let skipped = self
            .skipped
            .iter()
            .map(|line| format!("skip {key} hook (not approved): {line}"));
        run.chain(skipped).collect()
    }

    /// Runs the hooks in `cwd` with `env` on top of the user's. A failure
    /// of a blocking (`pre-*`) hook stops the rest and is the error; any
    /// other failure is a warning, reported with the hooks' output.
    pub fn run(
        &self,
        app: &App,
        cwd: &Path,
        env: &[(String, String)],
        report: &mut dyn FnMut(HookEvent),
    ) -> Result<()> {
        let mut full = app.env_vec();
        full.extend(env.iter().cloned());
        for command in &self.run {
            let key = self.kind.key();
            match run_hook(&command.argv, cwd, &full) {
                Ok(output) if output.is_empty() => {}
                Ok(output) => report(HookEvent::Output(format!(
                    "{key} hook {:?}: {output}",
                    command.line
                ))),
                Err(message) => {
                    let message = format!("{key} hook {:?} failed: {message}", command.line);
                    if self.kind.blocking() {
                        return Err(CliError::user(message));
                    }
                    report(HookEvent::Warning(message));
                }
            }
        }
        Ok(())
    }
}

/// The hooks of `kinds` for a worktree of the repository `settings`
/// describes, rendered with `vars`. Lines from the repository's own file
/// that are not approved yet are approved for this run
/// ([`Approval::Given`]), asked about on a terminal ([`Approval::Ask`]; a
/// yes is stored), or skipped with a warning; a dry run neither asks nor
/// stores nor warns.
pub fn prepare(
    app: &App,
    settings: &RepoSettings,
    kinds: &[Kind],
    vars: &Vars,
    approval: Approval,
    dry_run: bool,
    report: &mut dyn FnMut(HookEvent),
) -> Result<Vec<Hooks>> {
    let pending = pending(app, settings, kinds)?;
    let approved = pending.is_empty()
        || match approval {
            Approval::Given => true,
            Approval::Ask => approve(app, settings, &pending, dry_run)?,
            Approval::Skip => false,
        };
    let mut all = Vec::new();
    for &kind in kinds {
        let mut hooks = Hooks {
            kind,
            run: Vec::new(),
            skipped: Vec::new(),
        };
        for line in kind.lines(&settings.config.hooks) {
            if !approved && pending.iter().any(|(k, l)| *k == kind && l == line) {
                hooks.skipped.push(line.clone());
                continue;
            }
            let argv = hooks::argv(kind, line, vars).map_err(|e| CliError::user(e.to_string()))?;
            hooks.run.push(Command {
                line: line.clone(),
                argv,
            });
        }
        all.push(hooks);
    }
    if !approved && !dry_run {
        let file = settings.repo_file.as_deref().unwrap_or(Path::new(""));
        report(HookEvent::Warning(format!(
            "hooks from {} are not approved and were skipped: {}; approve them with \
             tasq worktree trust {}, or pass --yes",
            file.display(),
            pending
                .iter()
                .map(|(_, line)| format!("{line:?}"))
                .collect::<Vec<_>>()
                .join(", "),
            settings.main.display()
        )));
    }
    Ok(all)
}

/// The lines of `kinds` that come from the repository's file and are not
/// in the trust store.
fn pending(app: &App, settings: &RepoSettings, kinds: &[Kind]) -> Result<Vec<(Kind, String)>> {
    let Some(file) = &settings.repo_file else {
        return Ok(Vec::new());
    };
    let store = read_store(app)?;
    Ok(kinds
        .iter()
        .filter(|kind| settings.untrusted.contains(kind))
        .flat_map(|&kind| {
            store
                .pending(file, kind.lines(&settings.config.hooks))
                .into_iter()
                .map(move |line| (kind, line.to_owned()))
        })
        .collect())
}

/// What approving the pending lines takes, decided before any I/O.
#[derive(Debug, PartialEq, Eq)]
enum Approve {
    /// Nothing to approve: no repository file holds the lines.
    Yes,
    /// A dry run: never asks, never stores.
    No,
    /// Ask this on the terminal; a yes is stored.
    Ask(String),
}

/// What approving `pending`, from the repository's `file`, takes.
fn approval(file: Option<&Path>, pending: &[(Kind, String)], dry_run: bool) -> Approve {
    match file {
        None => Approve::Yes,
        Some(_) if dry_run => Approve::No,
        Some(file) => Approve::Ask(question(file, pending)),
    }
}

/// Asks on the terminal whether the pending lines may run (never on a
/// dry run); a yes approves every hook line of the repository's file and
/// stores that.
// Reason: asks the terminal and stores a yes with `trust`; `approval` decides whether to ask.
#[mutants::skip]
fn approve(
    app: &App,
    settings: &RepoSettings,
    pending: &[(Kind, String)],
    dry_run: bool,
) -> Result<bool> {
    match approval(settings.repo_file.as_deref(), pending, dry_run) {
        Approve::Yes => Ok(true),
        Approve::No => Ok(false),
        Approve::Ask(question) => {
            let yes = ask(&question)?;
            if yes {
                trust(app, settings)?;
            }
            Ok(yes)
        }
    }
}

/// The approval question for the `pending` lines of `file`.
fn question(file: &Path, pending: &[(Kind, String)]) -> String {
    let mut lines = vec![format!("{} wants to run these hooks:", file.display())];
    lines.extend(
        pending
            .iter()
            .map(|(kind, line)| format!("  {}: {line}", kind.key())),
    );
    lines.push("Run them, now and from now on? [y/N] ".to_owned());
    lines.join("\n")
}

/// Stores every hook line the repository's file sets as approved; the
/// lines, by kind. Nothing to store without such a file.
pub fn trust(app: &App, settings: &RepoSettings) -> Result<Vec<(Kind, String)>> {
    let Some(file) = &settings.repo_file else {
        return Ok(Vec::new());
    };
    let mut store = read_store(app)?;
    let mut approved = Vec::new();
    for &kind in &settings.untrusted {
        let lines = kind.lines(&settings.config.hooks);
        store.approve(file, lines);
        approved.extend(lines.iter().map(|line| (kind, line.clone())));
    }
    if !approved.is_empty() {
        write_store(app, &store)?;
    }
    Ok(approved)
}

/// Asks `question` on stderr and reads a line; `false` without a terminal.
// Reason: reads the terminal; `is_yes` reads the reply.
#[mutants::skip]
fn ask(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    eprint!("{question}");
    std::io::stderr().flush()?;
    let mut reply = String::new();
    std::io::stdin().lock().read_line(&mut reply)?;
    Ok(is_yes(&reply))
}

/// Whether a reply to a `[y/N]` question is a yes: its first non-blank
/// character is `y` or `Y`.
fn is_yes(reply: &str) -> bool {
    reply.trim_start().starts_with(['y', 'Y'])
}

/// The trust file for this environment.
fn store_file(app: &App) -> Result<PathBuf> {
    hooks::trust_file(
        app.opts.env.get("XDG_STATE_HOME").map(String::as_str),
        app.opts.home.as_deref(),
    )
    .ok_or_else(|| CliError::user("cannot tell where to keep hook approvals: HOME is not set"))
}

fn read_store(app: &App) -> Result<TrustStore> {
    let file = store_file(app)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => {
            TrustStore::parse(&text).map_err(|e| CliError::user(format!("{}: {e}", file.display())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrustStore::default()),
        Err(e) => Err(CliError::user(format!("{}: {e}", file.display()))),
    }
}

fn write_store(app: &App, store: &TrustStore) -> Result<()> {
    let file = store_file(app)?;
    let failed = |e: std::io::Error| CliError::user(format!("{}: {e}", file.display()));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    std::fs::write(&file, store.to_toml()).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_from_yes() {
        assert_eq!(Approval::from_yes(true), Approval::Given);
        assert_eq!(Approval::from_yes(false), Approval::Ask);
    }

    #[test]
    fn approval_needs_a_repository_file_and_no_dry_run_to_ask() {
        let file = Path::new("/ws/.tasq.toml");
        let pending = [(Kind::PostCreate, "direnv allow .".to_owned())];
        assert_eq!(approval(None, &pending, false), Approve::Yes);
        assert_eq!(approval(None, &pending, true), Approve::Yes);
        assert_eq!(approval(Some(file), &pending, true), Approve::No);
        assert_eq!(
            approval(Some(file), &pending, false),
            Approve::Ask(question(file, &pending))
        );
    }

    #[test]
    fn a_yes_starts_with_y() {
        for reply in ["y\n", "Y\n", "  yes\n", "Yep"] {
            assert!(is_yes(reply), "{reply:?}");
        }
        for reply in ["", "\n", "n\n", "N", "no", " ny", "x y"] {
            assert!(!is_yes(reply), "{reply:?}");
        }
    }

    #[test]
    fn the_question_lists_the_pending_lines() {
        let pending = [
            (Kind::PreCreate, "make setup".to_owned()),
            (Kind::PostCreate, "direnv allow .".to_owned()),
        ];
        assert_eq!(
            question(Path::new("/ws/.tasq.toml"), &pending),
            "/ws/.tasq.toml wants to run these hooks:\n  pre-create: make setup\n  \
             post-create: direnv allow .\nRun them, now and from now on? [y/N] "
        );
    }
}
