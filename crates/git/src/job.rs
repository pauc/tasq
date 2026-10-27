//! `tasq-git job --store <dir>`: the background job a `post-write` or a
//! `store-open` detaches. Under `job.lock` it commits the store's task
//! files with a message built from the writes recorded since the last
//! commit, and, when the branch has an upstream and it committed or the
//! fetch is due, fetches, rebases onto the upstream under the store lock
//! and pushes. It goes round again while a hook has touched `pending`
//! meanwhile, so the last write is never left uncommitted.
//!
//! A failed fetch, rebase or push is a lasting warning of its kind, and
//! the step done (or found not needed) clears it. A push that fails is
//! retried once after another fetch and rebase: another machine pushed in
//! between.
//!
//! A rebase that stops on a conflict is aborted, so the task files never
//! hold conflict markers and the local commits stay; its warning names the
//! tasks and the commands that resolve it by hand. Nothing is pushed until
//! then: the branch stays behind its upstream. While a rebase is in
//! progress (the user resolving it), the job leaves the repository alone.
//!
//! A pull can bring in a task whose id a local one has (both machines
//! created it offline). After a rebase moved the branch, and while that
//! warning stands, the job asks `tasq doctor --json` and keeps one lasting
//! warning per `ids` failure, until they are fixed by hand.
//!
//! `tasq git sync` runs the same rounds in the foreground ([`foreground`]),
//! fetching whatever the interval, and prints one line per step.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::git::{Git, GitError, TASK_FILES};
use crate::hook::is_repository;
use crate::state::{JobLock, State};
use crate::tasq::Tasq;

/// The subject of a commit no recorded write explains (a hand edit, or a
/// write whose line was lost).
pub const UNREPORTED: &str = "update task files";

/// The start of the lasting warning a failed commit leaves.
const COMMIT_FAILED: &str = "commit failed: ";

/// The start of the lasting warning a failed fetch leaves.
const FETCH_FAILED: &str = "fetch failed: ";

/// The start of the lasting warning a failed or conflicting rebase leaves.
const REBASE: &str = "rebase ";

/// What follows [`REBASE`] for a failed rebase, before git's message.
const FAILED: &str = "failed: ";

/// What follows [`REBASE`] for a conflict, before the tasks.
const CONFLICT: &str = "conflict on ";

/// The start of the lasting warning a push that failed twice leaves.
const PUSH_FAILED: &str = "push failed: ";

/// The start of the lasting warning for an id claimed by several files.
const DUPLICATE_ID: &str = "duplicate id: ";

/// What the job is told on its command line, and its clock.
#[derive(Debug, Clone)]
pub struct Settings {
    /// How old the last fetch must be for a round that committed nothing
    /// to fetch.
    pub interval: Duration,
    /// `tasq-git` itself: the rebase runs as `<exe> rebase --store <dir>`
    /// under `tasq store lock`.
    pub exe: PathBuf,
    /// The time, for `last-fetch`.
    pub now: fn() -> SystemTime,
}

/// What the rounds did, one line per step, for `tasq git sync` to print.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// One line per step, in order.
    pub lines: Vec<String>,
    /// Whether a step failed (its line is the lasting warning it left).
    pub failed: bool,
}

impl Report {
    fn step(&mut self, line: String) {
        self.lines.push(line);
    }

    fn failure(&mut self, line: String) {
        self.lines.push(line);
        self.failed = true;
    }
}

/// Why the job stopped.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// `tasq git sync` on a store that is not a repository.
    #[error(
        "{} is not a git repository: tasq git init makes it one, or tasq git clone <url> fills \
         an empty store",
        .0.display()
    )]
    NotRepository(PathBuf),
    /// `tasq git sync` while a rebase is stopped in the store.
    #[error(
        "a rebase is in progress in {}: finish it with git rebase --continue or --abort",
        .0.display()
    )]
    Rebasing(PathBuf),
    /// A git step of the commit failed; also left as a lasting warning.
    #[error("{COMMIT_FAILED}{0}")]
    Commit(String),
    /// The plugin's own files could not be read or written.
    #[error("{action}: {source}")]
    State {
        /// What was being done.
        action: &'static str,
        /// The OS error.
        source: std::io::Error,
    },
}

/// Commits and syncs the writes of `store` until none is pending. Another
/// job holding the lock, or a store that is not a repository, is no work.
pub fn run(git: &Git, tasq: &Tasq, store: &Path, settings: &Settings) -> Result<(), JobError> {
    if !is_repository(store) {
        return Ok(());
    }
    let state = State::of(store);
    let Some(lock) = take_lock(&state)? else {
        return Ok(());
    };
    rounds(
        git,
        tasq,
        store,
        &state,
        lock,
        settings,
        &mut Report::default(),
    )
}

/// `tasq git sync`: the rounds of [`run`] in the foreground, once a running
/// job is done, fetching whatever `settings.interval` says; what each step
/// did. A store that is not a repository, or one with a rebase stopped in
/// it, is an error.
pub fn foreground(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    settings: &Settings,
) -> Result<Report, JobError> {
    if !is_repository(store) {
        return Err(JobError::NotRepository(store.to_owned()));
    }
    if rebasing(git, store) {
        return Err(JobError::Rebasing(store.to_owned()));
    }
    let state = State::of(store);
    let lock = state
        .wait_job()
        .map_err(io("could not take the job lock"))?;
    let mut report = Report::default();
    rounds(git, tasq, store, &state, lock, settings, &mut report)?;
    Ok(report)
}

fn take_lock(state: &State) -> Result<Option<JobLock>, JobError> {
    state.lock_job().map_err(io("could not take the job lock"))
}

/// Rounds under `lock`, then under the lock taken again, while a hook
/// touched `pending` meanwhile.
fn rounds(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    state: &State,
    mut lock: JobLock,
    settings: &Settings,
    report: &mut Report,
) -> Result<(), JobError> {
    loop {
        state
            .clear_pending()
            .map_err(io("could not clear the pending mark"))?;
        // A rebase in progress is the user resolving a conflict: the
        // writes wait for it, uncommitted, and nothing is synced.
        if !rebasing(git, store) {
            round(git, tasq, store, state, settings, report)?;
        }
        // A hook that touched `pending` while this lock was held started no
        // job: the work is this one's.
        drop(lock);
        if !state.is_pending() {
            return Ok(());
        }
        let Some(next) = take_lock(state)? else {
            return Ok(());
        };
        lock = next;
    }
}

/// One commit, and the sync when it committed or the fetch is due.
fn round(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    state: &State,
    settings: &Settings,
    report: &mut Report,
) -> Result<(), JobError> {
    let outcome = commit(git, tasq, store, state);
    match &outcome {
        Ok(_) => state.set_warning(COMMIT_FAILED, None),
        Err(JobError::Commit(message)) => state.set_warning(COMMIT_FAILED, Some(message)),
        Err(_) => Ok(()),
    }
    .map_err(io("could not write the warnings"))?;
    let committed = outcome?;
    match &committed {
        Some(subject) => report.step(format!("committed: {subject}")),
        None => report.step("nothing to commit".to_owned()),
    }
    let mut pulled = false;
    if committed.is_some() || state.fetch_due((settings.now)(), settings.interval) {
        let before = head(git, store);
        sync(git, tasq, store, state, &settings.exe, report)?;
        state
            .record_fetch((settings.now)())
            .map_err(io("could not record the fetch"))?;
        pulled = head(git, store) != before;
    }
    let standing = state
        .has_warning(DUPLICATE_ID)
        .map_err(io("could not read the warnings"))?;
    if pulled || standing {
        duplicates(tasq, state, report)?;
    }
    Ok(())
}

/// The commit `HEAD` names, if git can say.
fn head(git: &Git, store: &Path) -> Option<String> {
    git.check(store, &["rev-parse", "HEAD"]).ok()
}

/// One [`DUPLICATE_ID`] warning per `ids` failure of `tasq doctor`,
/// `<detail>; fix: <fix>`; the warnings are left as they were when doctor
/// gives no answer.
fn duplicates(tasq: &Tasq, state: &State, report: &mut Report) -> Result<(), JobError> {
    let Some(failures) = tasq.id_failures() else {
        return Ok(());
    };
    let lines: Vec<String> = failures
        .iter()
        .map(|check| match &check.fix {
            Some(fix) => format!("{}; fix: {fix}", check.detail),
            None => check.detail.clone(),
        })
        .collect();
    for line in &lines {
        report.failure(format!("{DUPLICATE_ID}{line}"));
    }
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    state
        .set_warnings(DUPLICATE_ID, &lines)
        .map_err(io("could not write the warnings"))
}

fn io(action: &'static str) -> impl Fn(std::io::Error) -> JobError {
    move |source| JobError::State { action, source }
}

/// One commit of the task files, if they changed; its subject, if it made
/// one. The writes it explains are forgotten either way: a write with no
/// change left was committed already.
fn commit(git: &Git, tasq: &Tasq, store: &Path, state: &State) -> Result<Option<String>, JobError> {
    let writes = state.writes().map_err(io("could not read the writes"))?;
    let mut status = vec!["status", "--porcelain", "-z", "--untracked-files=all", "--"];
    status.extend(TASK_FILES);
    let changed = !git.check(store, &status).map_err(failed)?.is_empty();
    let mut subject = None;
    if changed {
        let mut add = vec!["add", "--all", "--"];
        add.extend(TASK_FILES);
        git.check(store, &add).map_err(failed)?;
        let text = message(&writes, |id| tasq.title(id).ok());
        git.check(store, &["commit", "--quiet", "-m", &text])
            .map_err(failed)?;
        subject = text.lines().next().map(str::to_owned);
    }
    state
        .forget(&writes)
        .map_err(io("could not drop the committed writes"))?;
    Ok(subject)
}

fn failed(err: GitError) -> JobError {
    let GitError { message, .. } = err;
    JobError::Commit(one_line(&message))
}

/// git's message on one line, as a warning line must be.
fn one_line(message: &str) -> String {
    let lines: Vec<&str> = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("; ")
}

/// Fetches, rebases onto the upstream when behind and pushes when ahead,
/// if the branch has an upstream. The warning of the step that failed is
/// set, those of the steps before it cleared, those after it left; a
/// rebase left for the next round stops there too, setting nothing. Only
/// the plugin's own files failing is an error.
fn sync(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    state: &State,
    exe: &Path,
    report: &mut Report,
) -> Result<(), JobError> {
    let Ok(upstream) = git.check(store, &["rev-parse", "--abbrev-ref", "@{upstream}"]) else {
        report.step("no upstream: nothing to fetch or push".to_owned());
        return Ok(());
    };
    let rebase = [
        exe.as_os_str(),
        OsStr::new("rebase"),
        OsStr::new("--store"),
        store.as_os_str(),
    ];
    let remote = Remote {
        git,
        tasq,
        store,
        rebase: &rebase,
        upstream: upstream.trim(),
    };
    let outcome = remote.exchange(report);
    let stop = match &outcome {
        Ok(Exchange::Done) => None,
        Ok(Exchange::Deferred) => Some(REBASE),
        Err((kind, _)) => Some(*kind),
    };
    for kind in [FETCH_FAILED, REBASE, PUSH_FAILED] {
        if stop == Some(kind) {
            if let Err((_, message)) = &outcome {
                let message = one_line(message);
                report.failure(format!("{kind}{message}"));
                state
                    .set_warning(kind, Some(&message))
                    .map_err(io("could not write the warnings"))?;
            }
            break;
        }
        state
            .set_warning(kind, None)
            .map_err(io("could not write the warnings"))?;
    }
    Ok(())
}

/// How far an exchange with the remote got, short of a failure.
enum Exchange {
    /// Up to date with the upstream, or as close as a push gets it.
    Done,
    /// Still behind after the rebase step: a write not yet committed held
    /// the rebase back, and the next round commits and syncs it.
    Deferred,
}

/// The branch's upstream, and what the exchange with it needs.
struct Remote<'a> {
    git: &'a Git,
    tasq: &'a Tasq,
    store: &'a Path,
    /// `tasq-git rebase --store <dir>`, run under the store lock.
    rebase: &'a [&'a OsStr],
    /// Its short name (`origin/default`), for the report.
    upstream: &'a str,
}

impl Remote<'_> {
    /// The fetch, rebase and push; the warning kind of the step that
    /// failed and its message. A push that fails is tried once more after
    /// another fetch and rebase.
    fn exchange(&self, report: &mut Report) -> Result<Exchange, (&'static str, String)> {
        let Self {
            git,
            tasq,
            store,
            rebase,
            upstream,
        } = *self;
        for last in [false, true] {
            git.check(store, &["fetch", "--quiet"])
                .map_err(|err| (FETCH_FAILED, err.message))?;
            report.step(format!("fetched {upstream}"));
            let (_, behind) = divergence(git, store).map_err(|m| (FETCH_FAILED, m))?;
            if behind > 0 {
                tasq.locked(rebase)
                    .map_err(|m| (REBASE, rebase_failure(&m)))?;
            }
            let (ahead, still) =
                divergence(git, store).map_err(|m| (REBASE, rebase_failure(&m)))?;
            if still > 0 {
                report.step(
                    "rebase left for the next round: a task file changed meanwhile".to_owned(),
                );
                return Ok(Exchange::Deferred);
            }
            if behind > 0 {
                report.step(format!("pulled {} from {upstream}", commits(behind)));
            }
            if ahead == 0 {
                if behind == 0 {
                    report.step(format!("up to date with {upstream}"));
                }
                return Ok(Exchange::Done);
            }
            match git.check(store, &["-c", "push.default=upstream", "push", "--quiet"]) {
                Ok(_) => {
                    report.step(format!("pushed {} to {upstream}", commits(ahead)));
                    return Ok(Exchange::Done);
                }
                Err(err) if last => return Err((PUSH_FAILED, err.message)),
                Err(err) => report.step(format!(
                    "push failed, fetching again: {}",
                    one_line(&err.message)
                )),
            }
        }
        Ok(Exchange::Done)
    }
}

/// `1 commit`, `2 commits`.
pub fn commits(n: usize) -> String {
    if n == 1 {
        "1 commit".to_owned()
    } else {
        format!("{n} commits")
    }
}

/// The rebase warning, after [`REBASE`], for what the rebase step printed:
/// a conflict as `tasq-git rebase` words it, anything else as failed.
fn rebase_failure(message: &str) -> String {
    if message.starts_with(CONFLICT) {
        message.to_owned()
    } else {
        format!("{FAILED}{message}")
    }
}

/// How many commits `HEAD` has that its upstream has not, and the other
/// way round.
fn divergence(git: &Git, store: &Path) -> Result<(usize, usize), String> {
    let out = git
        .check(
            store,
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        )
        .map_err(|err| err.message)?;
    let counts = out
        .trim()
        .split_once('\t')
        .and_then(|(ahead, behind)| Some((ahead.parse().ok()?, behind.parse().ok()?)));
    counts.ok_or_else(|| format!("git rev-list answered {out:?}"))
}

/// `tasq-git rebase --store <dir>`, run by the job under the store lock:
/// rebases the branch onto its upstream, unless a task file has changes
/// not committed yet (a write that landed after the job's commit): the
/// next round commits it and rebases then. A rebase that stops (a
/// conflict) is aborted, leaving the files and commits as before, and the
/// error is the conflict's warning ([`conflict`]); otherwise it is git's
/// message. A rebase already in progress is the user's and is left alone.
pub fn rebase(git: &Git, store: &Path) -> Result<(), String> {
    if rebasing(git, store) {
        return Err(
            "a rebase is in progress: finish it with git rebase --continue or --abort".to_owned(),
        );
    }
    let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    status.extend(TASK_FILES);
    let dirty = git.check(store, &status).map_err(|err| err.message)?;
    if !dirty.is_empty() {
        return Ok(());
    }
    let Err(failed) = git.check(store, &["rebase", "--quiet", "@{upstream}"]) else {
        return Ok(());
    };
    if !rebasing(git, store) {
        return Err(failed.message);
    }
    // Listed while the rebase is stopped, and aborted whether or not they
    // could be: a stopped rebase left behind stops every later round.
    let unmerged = git.check(store, &["diff", "--name-only", "-z", "--diff-filter=U"]);
    git.check(store, &["rebase", "--abort"])
        .map_err(|err| err.message)?;
    let unmerged = unmerged.map_err(|err| err.message)?;
    let tasks: Vec<String> = unmerged
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(|file| {
            let text = std::fs::read_to_string(store.join(file)).unwrap_or_default();
            task_id(&text).map_or_else(|| file.to_owned(), str::to_owned)
        })
        .collect();
    if tasks.is_empty() {
        return Err(failed.message);
    }
    Err(conflict(store, &tasks))
}

/// Whether a rebase is stopped in the repository at `store` (git's
/// `rebase-merge` or `rebase-apply` directory is there).
pub fn rebasing(git: &Git, store: &Path) -> bool {
    git.check(
        store,
        &[
            "rev-parse",
            "--git-path",
            "rebase-merge",
            "--git-path",
            "rebase-apply",
        ],
    )
    .is_ok_and(|paths| paths.lines().any(|path| store.join(path).exists()))
}

/// The `id:` of a task file's frontmatter, if it has one.
pub fn task_id(text: &str) -> Option<&str> {
    let mut lines = text.lines().map(str::trim_end);
    if lines.next()? != "---" {
        return None;
    }
    lines
        .take_while(|line| *line != "---")
        .find_map(|line| line.strip_prefix("id:"))
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// The rebase warning, after `rebase `, for a conflict on `tasks` (ids,
/// or the names of files without one) in `store`.
pub fn conflict(store: &Path, tasks: &[String]) -> String {
    let noun = if tasks.len() == 1 { "task" } else { "tasks" };
    format!(
        "{CONFLICT}{noun} {}: local commits kept, nothing pushed until it is resolved: \
         cd {} && git pull --rebase, fix the files, git add them, git rebase --continue",
        tasks.join(", "),
        store.display()
    )
}

/// The commit message for the recorded `writes` (one `<command> <ids>` per
/// line): the first line and the title of its first id as the subject,
/// and every line in the body when there are several.
pub fn message(writes: &str, title: impl Fn(&str) -> Option<String>) -> String {
    let lines: Vec<&str> = writes.lines().filter(|l| !l.trim().is_empty()).collect();
    let Some(first) = lines.first() else {
        return UNREPORTED.to_owned();
    };
    let mut text = (*first).to_owned();
    if let Some(title) = first.split(' ').nth(1).and_then(title) {
        text.push_str(": ");
        text.push_str(&title);
    }
    if lines.len() > 1 {
        text.push_str("\n\n");
        text.push_str(&lines.join("\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(id: &str) -> Option<String> {
        (id == "3").then(|| "Three".to_owned())
    }

    #[test]
    fn one_write_is_a_subject_alone() {
        assert_eq!(message("log 3\n", titles), "log 3: Three");
    }

    #[test]
    fn several_writes_are_listed_in_the_body() {
        assert_eq!(
            message("log 3\n\nset 4\n", titles),
            "log 3: Three\n\nlog 3\nset 4"
        );
    }

    #[test]
    fn a_task_id_is_read_from_the_frontmatter_only() {
        assert_eq!(
            task_id("---\nstatus: ready\nid: 12\n---\n# T\n"),
            Some("12")
        );
        assert_eq!(task_id("---\r\nid:  7 \r\n---\r\n"), Some("7"));
        assert_eq!(task_id("---\nstatus: ready\n---\nid: 3\n"), None);
        assert_eq!(task_id("# T\nid: 3\n"), None);
        assert_eq!(task_id("---\nid:\n---\n"), None);
        assert_eq!(task_id(""), None);
    }

    #[test]
    fn a_conflict_names_every_task() {
        let store = Path::new("/s");
        let tail = ": local commits kept, nothing pushed until it is resolved: cd /s && \
                    git pull --rebase, fix the files, git add them, git rebase --continue";
        assert_eq!(
            conflict(store, &["3".to_owned()]),
            format!("conflict on task 3{tail}")
        );
        assert_eq!(
            conflict(store, &["3".to_owned(), "x.md".to_owned()]),
            format!("conflict on tasks 3, x.md{tail}")
        );
    }

    #[test]
    fn no_title_or_no_id_leaves_the_line() {
        assert_eq!(message("set 4\n", titles), "set 4");
        assert_eq!(message("ui\n", titles), "ui");
        assert_eq!(message(" \n", titles), UNREPORTED);
    }
}
