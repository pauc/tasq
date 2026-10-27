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

/// The start of the lasting warning a failed rebase leaves.
const REBASE_FAILED: &str = "rebase failed: ";

/// The start of the lasting warning a push that failed twice leaves.
const PUSH_FAILED: &str = "push failed: ";

/// What the job is told on its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// How old the last fetch must be for a round that committed nothing
    /// to fetch.
    pub interval: Duration,
    /// `tasq-git` itself: the rebase runs as `<exe> rebase --store <dir>`
    /// under `tasq store lock`.
    pub exe: PathBuf,
}

/// Why the job stopped.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
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
    loop {
        let Some(lock) = state
            .lock_job(SystemTime::now())
            .map_err(io("could not take the job lock"))?
        else {
            break;
        };
        state
            .clear_pending()
            .map_err(io("could not clear the pending mark"))?;
        let outcome = commit(git, tasq, store, &state);
        match &outcome {
            Ok(_) => state.set_warning(COMMIT_FAILED, None),
            Err(JobError::Commit(message)) => state.set_warning(COMMIT_FAILED, Some(message)),
            Err(JobError::State { .. }) => Ok(()),
        }
        .map_err(io("could not write the warnings"))?;
        let committed = outcome?;
        if committed || state.fetch_due(SystemTime::now(), settings.interval) {
            sync(git, tasq, store, &state, &lock, &settings.exe)?;
            state
                .record_fetch(SystemTime::now())
                .map_err(io("could not record the fetch"))?;
        }
        // A hook that touched `pending` while this lock was held started no
        // job: the work is this one's.
        drop(lock);
        if !state.is_pending() {
            break;
        }
    }
    Ok(())
}

fn io(action: &'static str) -> impl Fn(std::io::Error) -> JobError {
    move |source| JobError::State { action, source }
}

/// One commit of the task files, if they changed; whether it made one.
/// The writes it explains are forgotten either way: a write with no change
/// left was committed already.
fn commit(git: &Git, tasq: &Tasq, store: &Path, state: &State) -> Result<bool, JobError> {
    let writes = state.writes().map_err(io("could not read the writes"))?;
    let mut status = vec!["status", "--porcelain", "-z", "--untracked-files=all", "--"];
    status.extend(TASK_FILES);
    let changed = !git.check(store, &status).map_err(failed)?.is_empty();
    if changed {
        let mut add = vec!["add", "--all", "--"];
        add.extend(TASK_FILES);
        git.check(store, &add).map_err(failed)?;
        let text = message(&writes, |id| tasq.title(id).ok());
        git.check(store, &["commit", "--quiet", "-m", &text])
            .map_err(failed)?;
    }
    state
        .forget(&writes)
        .map_err(io("could not drop the committed writes"))?;
    Ok(changed)
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
/// set, those of the steps before it cleared; only the plugin's own files
/// failing is an error.
fn sync(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    state: &State,
    lock: &JobLock,
    exe: &Path,
) -> Result<(), JobError> {
    if git
        .check(store, &["rev-parse", "--verify", "--quiet", "@{upstream}"])
        .is_err()
    {
        return Ok(());
    }
    let rebase = [
        exe.as_os_str(),
        OsStr::new("rebase"),
        OsStr::new("--store"),
        store.as_os_str(),
    ];
    let failure = exchange(git, tasq, store, lock, &rebase).err();
    for kind in [FETCH_FAILED, REBASE_FAILED, PUSH_FAILED] {
        let message = failure
            .as_ref()
            .filter(|(failed, _)| *failed == kind)
            .map(|(_, message)| one_line(message));
        state
            .set_warning(kind, message.as_deref())
            .map_err(io("could not write the warnings"))?;
        if message.is_some() {
            break;
        }
    }
    Ok(())
}

/// The fetch, rebase and push; the warning kind of the step that failed
/// and its message. A push that fails is tried once more after another
/// fetch and rebase.
fn exchange(
    git: &Git,
    tasq: &Tasq,
    store: &Path,
    lock: &JobLock,
    rebase: &[&OsStr],
) -> Result<(), (&'static str, String)> {
    for last in [false, true] {
        refresh(lock);
        git.check(store, &["fetch", "--quiet"])
            .map_err(|err| (FETCH_FAILED, err.message))?;
        let (_, behind) = divergence(git, store).map_err(|m| (FETCH_FAILED, m))?;
        if behind > 0 {
            refresh(lock);
            tasq.locked(rebase).map_err(|m| (REBASE_FAILED, m))?;
        }
        let (ahead, behind) = divergence(git, store).map_err(|m| (REBASE_FAILED, m))?;
        // Still behind: the rebase found a write not yet committed, which
        // the next round commits and syncs.
        if ahead == 0 || behind > 0 {
            return Ok(());
        }
        refresh(lock);
        match git.check(store, &["-c", "push.default=upstream", "push", "--quiet"]) {
            Ok(_) => return Ok(()),
            Err(err) if last => return Err((PUSH_FAILED, err.message)),
            Err(_) => {}
        }
    }
    Ok(())
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

/// Stamps the job lock before a step that may take long. A lock that
/// cannot be stamped only risks a second job starting meanwhile.
fn refresh(lock: &JobLock) {
    let _ = lock.refresh(SystemTime::now());
}

/// `tasq-git rebase --store <dir>`, run by the job under the store lock:
/// rebases the branch onto its upstream, unless a task file has changes
/// not committed yet (a write that landed after the job's commit): the
/// next round commits it and rebases then. The error is git's message.
pub fn rebase(git: &Git, store: &Path) -> Result<(), String> {
    let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    status.extend(TASK_FILES);
    let dirty = git.check(store, &status).map_err(|err| err.message)?;
    if !dirty.is_empty() {
        return Ok(());
    }
    git.check(store, &["rebase", "--quiet", "@{upstream}"])
        .map(drop)
        .map_err(|err| err.message)
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
    fn no_title_or_no_id_leaves_the_line() {
        assert_eq!(message("set 4\n", titles), "set 4");
        assert_eq!(message("ui\n", titles), "ui");
        assert_eq!(message(" \n", titles), UNREPORTED);
    }
}
