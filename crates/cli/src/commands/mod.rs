//! One module per command. Each `run` is a core or store call plus
//! rendering, in both human and `--json` form.

use std::path::{Path, PathBuf};

use tasq_core::model::TaskId;
use tasq_core::store::Store;
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::error::Result;
use crate::json;

pub mod apply;
pub mod completions;
pub mod config;
pub mod create;
pub mod dates;
pub mod doctor;
pub mod edit;
pub mod launch;
pub mod list;
pub mod mr;
pub mod project;
pub mod session;
pub mod store;
pub mod summary;
pub mod sync;
pub mod ui;
pub mod view;
pub mod worktree;

/// Ends an editing command: prints `line`, or with `--json` the task as it
/// is now, as `{"schema": 1, "task": {...}}`.
pub fn finish(app: &App, store: &NbStore, id: &TaskId, line: &str) -> Result<()> {
    if app.out.json_mode() {
        let task = store.get(id)?;
        return app
            .out
            .json(&json::document([("task", json::to_value(&task))]));
    }
    app.out.print(line)
}

/// `path` as an existing directory's canonical path, like the script's
/// `cd "$path" && pwd`.
pub fn existing_dir(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().filter(|p| p.is_dir())
}
