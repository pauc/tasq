//! `tasq-git job --store <dir>`: the background job a `post-write`
//! detaches. Under `job.lock` it commits the store's task files with a
//! message built from the writes recorded since the last commit, and goes
//! round again while a hook has touched `pending` meanwhile, so the last
//! write is never left uncommitted.

use std::path::Path;
use std::time::SystemTime;

use crate::git::{Git, GitError, TASK_FILES};
use crate::hook::is_repository;
use crate::state::State;
use crate::tasq::Tasq;

/// The subject of a commit no recorded write explains (a hand edit, or a
/// write whose line was lost).
pub const UNREPORTED: &str = "update task files";

/// The start of the lasting warning a failed commit leaves.
const COMMIT_FAILED: &str = "commit failed: ";

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

/// Commits the writes of `store` until none is pending. Another job
/// holding the lock, or a store that is not a repository, is no work.
pub fn run(git: &Git, tasq: &Tasq, store: &Path) -> Result<(), JobError> {
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
            Ok(()) => state.set_warning(COMMIT_FAILED, None),
            Err(JobError::Commit(message)) => state.set_warning(COMMIT_FAILED, Some(message)),
            Err(JobError::State { .. }) => Ok(()),
        }
        .map_err(io("could not write the warnings"))?;
        outcome?;
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

/// One commit of the task files, if they changed. The writes it explains
/// are forgotten either way: a write with no change left was committed
/// already.
fn commit(git: &Git, tasq: &Tasq, store: &Path, state: &State) -> Result<(), JobError> {
    let writes = state.writes().map_err(io("could not read the writes"))?;
    let mut status = vec!["status", "--porcelain", "-z", "--untracked-files=all", "--"];
    status.extend(TASK_FILES);
    if !git.check(store, &status).map_err(failed)?.is_empty() {
        let mut add = vec!["add", "--all", "--"];
        add.extend(TASK_FILES);
        git.check(store, &add).map_err(failed)?;
        let text = message(&writes, |id| tasq.title(id).ok());
        git.check(store, &["commit", "--quiet", "-m", &text])
            .map_err(failed)?;
    }
    state
        .forget(&writes)
        .map_err(io("could not drop the committed writes"))
}

/// git's message on one line, as a warning line must be.
fn failed(err: GitError) -> JobError {
    let message = err.message;
    let lines: Vec<&str> = message
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    JobError::Commit(lines.join("; "))
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
