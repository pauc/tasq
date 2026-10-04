//! `tasq session <id> <session-id> [desc]`.

use tasq_core::model::Session;
use tasq_core::store::Store;

use tasq_launch::launcher_for;

use crate::app::App;
use crate::commands::finish;
use crate::commands::launch::launch_settings;
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
    let launcher_name = launcher.unwrap_or(&app.config().launch.default);
    let launcher = launcher_for(launcher_name, &launch_settings(app)?)
        .map_err(|e| CliError::user(e.to_string()))?;
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
    let hint = launcher
        .resume_hint(session_id)
        .map_or_else(String::new, |h| format!(" (resume: {h})"));
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] session: {session_id}{hint}\n"),
    )
}
