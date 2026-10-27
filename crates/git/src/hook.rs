//! `tasq-git hook`: the answer to one event.
//!
//! For a store that is a repository, `post-write` records the writes for
//! the commit message, touches `pending` and, unless a job is running
//! already (it will see `pending`), detaches `tasq-git job`. `store-open`
//! does the same, recording nothing, once the last fetch is older than
//! `[plugin.git] interval`. Either returns at once. An interval that is
//! not whole seconds is the event's error, after the work was done with
//! the default one.

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::doctor;
use crate::git::Git;
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
    /// What to do about it, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
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
    /// `[plugin.git] interval` is not a whole number of seconds; its JSON.
    #[error("[plugin.git] interval must be a whole number of seconds, not {0}")]
    Interval(String),
    /// The writes could not be recorded.
    #[error("could not record the writes: {0}")]
    State(io::Error),
    /// The job could not be started.
    #[error("could not start the job: {0}")]
    Spawn(io::Error),
}

/// How long after a fetch `store-open` starts the next one, when
/// `[plugin.git] interval` does not say.
pub const INTERVAL: Duration = Duration::from_secs(60);

/// Handles the event in `stdin` and returns what to print on stdout:
/// nothing, or for `doctor` the checks as a JSON array and a newline.
/// `spawn` detaches the job for a store directory and the fetch interval;
/// `now` judges whether a fetch is due and dates the last fetch for
/// `doctor`; `git` answers `doctor`'s questions.
pub fn run(
    stdin: &str,
    git: &Git,
    now: SystemTime,
    spawn: impl FnOnce(&Path, Duration) -> io::Result<()>,
) -> Result<String, HookError> {
    let payload = payload::parse(stdin)?;
    if is_repository(&payload.store.location) {
        match payload.event.as_str() {
            "post-write" => post_write(&payload, spawn)?,
            "store-open" => store_open(&payload, now, spawn)?,
            _ => {}
        }
    }
    Ok(answer(&payload, git, now))
}

/// `[plugin.git] interval`, whole seconds, [`INTERVAL`] when unset.
pub fn interval(
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<Duration, HookError> {
    match config.get("interval") {
        None => Ok(INTERVAL),
        Some(value) => value
            .as_u64()
            .map(Duration::from_secs)
            .ok_or_else(|| HookError::Interval(value.to_string())),
    }
}

/// `[plugin.git] interval`, or [`INTERVAL`] and the error to report once
/// the event's work is done.
fn interval_or_default(payload: &Payload) -> (Duration, Result<(), HookError>) {
    match interval(&payload.config) {
        Ok(interval) => (interval, Ok(())),
        Err(err) => (INTERVAL, Err(err)),
    }
}

fn store_open(
    payload: &Payload,
    now: SystemTime,
    spawn: impl FnOnce(&Path, Duration) -> io::Result<()>,
) -> Result<(), HookError> {
    let (interval, valid) = interval_or_default(payload);
    let store = &payload.store.location;
    let state = State::of(store);
    if state.fetch_due(now, interval) {
        start(&state, store, interval, spawn)?;
    }
    valid
}

fn post_write(
    payload: &Payload,
    spawn: impl FnOnce(&Path, Duration) -> io::Result<()>,
) -> Result<(), HookError> {
    let (interval, valid) = interval_or_default(payload);
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
    start(&state, store, interval, spawn)?;
    valid
}

/// Touches `pending` and detaches a job unless a live one holds the lock:
/// that one goes round again for `pending`.
fn start(
    state: &State,
    store: &Path,
    interval: Duration,
    spawn: impl FnOnce(&Path, Duration) -> io::Result<()>,
) -> Result<(), HookError> {
    state.touch_pending().map_err(HookError::State)?;
    if !state.job_running() {
        spawn(store, interval).map_err(HookError::Spawn)?;
    }
    Ok(())
}

/// What the plugin prints for `payload`: for `doctor`, the repository's
/// checks ([`doctor::checks`]), or one saying sync is off for a store that
/// is not a repository. Other events print nothing.
pub fn answer(payload: &Payload, git: &Git, now: SystemTime) -> String {
    let location = &payload.store.location;
    if payload.event != "doctor" {
        return String::new();
    }
    let checks = if is_repository(location) {
        doctor::checks(git, location, now)
    } else {
        vec![Check {
            name: "sync",
            status: "ok",
            message: format!(
                "store sync off: {} is not a git repository",
                location.display()
            ),
            fix: None,
        }]
    };
    let mut out = serde_json::to_string(&checks).expect("a check serialises");
    out.push('\n');
    out
}

/// Whether the store directory is a git repository: it holds `.git`, a
/// directory or a `gitdir:` file.
pub fn is_repository(store: &Path) -> bool {
    store.join(".git").exists()
}
