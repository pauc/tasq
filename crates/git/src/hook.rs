//! `tasq-git hook`: the answer to one event.
//!
//! For a store that is a repository, `post-write` records the writes for
//! the commit message, touches `pending` and, unless a job is running
//! already (it will see `pending`), detaches `tasq-git job`. It returns at
//! once either way.

use std::io;
use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

use crate::payload::{self, Payload, PayloadError};
use crate::state::State;

/// One doctor check, as `docs/plugins.md` ("Doctor checks") reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// Listed by tasq as `git/<name>`.
    pub name: &'static str,
    /// `ok`, `warn` or `fail`.
    pub status: &'static str,
    /// One line.
    pub message: String,
}

/// Why an event failed; tasq shows it as a warning naming the plugin and
/// the event.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    /// The payload was refused.
    #[error(transparent)]
    Payload(#[from] PayloadError),
    /// A `post-write` payload without one of its fields.
    #[error("the post-write event payload has no {0}")]
    Missing(&'static str),
    /// The writes could not be recorded.
    #[error("could not record the writes: {0}")]
    State(io::Error),
    /// The job could not be started.
    #[error("could not start the job: {0}")]
    Spawn(io::Error),
}

/// Handles the event in `stdin` and returns what to print on stdout:
/// nothing, or for `doctor` the checks as a JSON array and a newline.
/// `spawn` detaches the job for a store directory; `now` judges whether a
/// running job's lock is stale.
pub fn run(
    stdin: &str,
    now: SystemTime,
    spawn: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<String, HookError> {
    let payload = payload::parse(stdin)?;
    if payload.event == "post-write" && is_repository(&payload.store.location) {
        post_write(&payload, now, spawn)?;
    }
    Ok(answer(&payload))
}

fn post_write(
    payload: &Payload,
    now: SystemTime,
    spawn: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), HookError> {
    let command = payload
        .command
        .as_deref()
        .ok_or(HookError::Missing("command"))?;
    let writes = payload
        .writes
        .as_ref()
        .ok_or(HookError::Missing("writes"))?;
    let mut line = command.to_owned();
    for write in writes {
        line.push(' ');
        line.push_str(&write.id);
    }
    let store = &payload.store.location;
    let state = State::of(store);
    state.record(&line).map_err(HookError::State)?;
    state.touch_pending().map_err(HookError::State)?;
    if !state.job_running(now) {
        spawn(store).map_err(HookError::Spawn)?;
    }
    Ok(())
}

/// What the plugin prints for `payload`. Events it does not know, and every
/// event of a store that is not a repository but `doctor`, print nothing.
pub fn answer(payload: &Payload) -> String {
    let location = &payload.store.location;
    if payload.event != "doctor" || is_repository(location) {
        return String::new();
    }
    let off = Check {
        name: "sync",
        status: "ok",
        message: format!(
            "store sync off: {} is not a git repository",
            location.display()
        ),
    };
    let mut out = serde_json::to_string(&[off]).expect("a check serialises");
    out.push('\n');
    out
}

/// Whether the store directory is a git repository: it holds `.git`, a
/// directory or a `gitdir:` file.
pub fn is_repository(store: &Path) -> bool {
    store.join(".git").exists()
}
