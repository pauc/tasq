//! `tasq project <id> [path]`.

use std::path::Path;

use tasq_core::store::Store;

use crate::app::App;
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};
use crate::json;

/// Shows the tracked project directory, or records `path`.
pub fn run(app: &App, id: &str, path: Option<&Path>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let Some(path) = path else {
        let task = store.get(&id)?;
        if app.out.json_mode() {
            return app.out.json(&json::document([
                ("task", json::to_value(&task)),
                (
                    "default_project",
                    json::to_value(&app.config().work.default_project),
                ),
            ]));
        }
        let line = match &task.project {
            Some(project) => format!("{}\n", project.display()),
            None => format!(
                "no project tracked (default: {})\n",
                app.config()
                    .work
                    .default_project
                    .as_ref()
                    .map_or_else(|| "unset".to_owned(), |p| p.display().to_string())
            ),
        };
        return app.out.print(&line);
    };
    let dir = existing_dir(path)
        .ok_or_else(|| CliError::user(format!("project path not found: {}", path.display())))?;
    let mut task = store.get(&id)?;
    task.project = Some(dir.clone());
    store.update(&task)?;
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] project: {}\n", dir.display()),
    )
}
