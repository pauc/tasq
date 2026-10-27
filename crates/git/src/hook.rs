//! `tasq-git hook`: the answer to one event.

use std::path::Path;

use serde::Serialize;

use crate::payload::{self, Payload, PayloadError};

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

/// Handles the event in `stdin` and returns what to print on stdout:
/// nothing, or for `doctor` the checks as a JSON array and a newline.
pub fn run(stdin: &str) -> Result<String, PayloadError> {
    let payload = payload::parse(stdin)?;
    Ok(answer(&payload))
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
