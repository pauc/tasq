//! `tasq session <id> <session-id> [desc]`.

use tasq_core::model::Session;
use tasq_core::store::Store;

use crate::app::App;
use crate::commands::finish;
use crate::error::{CliError, Result};

/// Records a session unless its id is already tracked, and prints how to
/// resume it.
pub fn run(
    app: &App,
    id: &str,
    session_id: &str,
    description: Option<&str>,
    launcher: Option<&str>,
) -> Result<()> {
    let id = App::task_id(id)?;
    let session_id = session_id.trim();
    if session_id.is_empty() || session_id.contains('`') {
        return Err(CliError::user(
            "the session id must not be empty or contain backticks",
        ));
    }
    let launcher = launcher.unwrap_or(&app.config().launch.default);
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let mut task = store.get(&id)?;
    if task.sessions.iter().any(|s| s.id == session_id) {
        return finish(
            app,
            &store,
            &id,
            &format!("[{id}] session already tracked: {session_id}\n"),
        );
    }
    let mut session = Session::new(clock.now(), session_id);
    session.description = description
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_owned);
    task.add_session(session);
    store.update(&task)?;
    let hint =
        resume_hint(launcher, session_id).map_or_else(String::new, |h| format!(" (resume: {h})"));
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] session: {session_id}{hint}\n"),
    )
}

/// How to get back into a session of `launcher`, when the launcher has a
/// resume command. (Phase 4 moves this onto the `Launcher` trait.)
pub fn resume_hint(launcher: &str, session_id: &str) -> Option<String> {
    match launcher {
        "claude" => Some(format!("claude --resume {session_id}")),
        "tmux" => Some(format!("tmux attach -t {session_id}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints() {
        assert_eq!(
            resume_hint("claude", "abc").as_deref(),
            Some("claude --resume abc")
        );
        assert_eq!(
            resume_hint("tmux", "w1").as_deref(),
            Some("tmux attach -t w1")
        );
        assert_eq!(resume_hint("shell", "x"), None);
        assert_eq!(resume_hint("herdr", "x"), None);
    }
}
