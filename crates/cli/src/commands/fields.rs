//! `tasq rename`, `tasq due`, `tasq tag` and `tasq desc`: one field of a
//! task each, after creation.
//!
//! Like [`crate::commands::edit`], every command is the argument parsing,
//! one [`tasq_core::edit`] call and the printed line (or, with `--json`,
//! the task as stored). A task that changed on disk since it was read
//! fails with the store's conflict error; nothing retries.

use tasq_core::edit;

use crate::app::App;
use crate::commands::finish;
use crate::error::Result;

/// `tasq rename <id> <title> [--note TEXT]`.
pub fn rename(app: &App, id: &str, title: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    edit::rename(&mut store, &id, title, note, clock.as_ref())?;
    finish(app, &store, &id, &format!("[{id}] renamed\n"))
}
