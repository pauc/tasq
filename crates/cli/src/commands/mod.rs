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
pub mod fields;
pub mod launch;
pub mod list;
pub mod mr;
pub mod plugins;
pub mod project;
pub mod session;
pub mod store;
pub mod summary;
pub mod sync;
pub mod ui;
pub mod view;
pub mod worktree;
pub mod worktree_hooks;
pub mod worktree_nav;

/// Ends an editing command: reports the store's warnings (a commit that
/// could not start) on stderr, then prints `line`, or with `--json` the
/// task as it is now, as `{"schema": 1, "task": {...}}`.
pub fn finish(app: &App, store: &NbStore, id: &TaskId, line: &str) -> Result<()> {
    report(app, store, id, line, None)
}

/// [`finish`] for a one-field edit (`set`, `rename`, `due`, `tag`, `desc`
/// with a value): `--json` also carries `"changed"`, the names of the
/// fields written, `[]` when the edit changed nothing and wrote nothing.
pub fn finish_edit(
    app: &App,
    store: &NbStore,
    id: &TaskId,
    line: &str,
    changed: &[&str],
) -> Result<()> {
    report(app, store, id, line, Some(changed))
}

fn report(
    app: &App,
    store: &NbStore,
    id: &TaskId,
    line: &str,
    changed: Option<&[&str]>,
) -> Result<()> {
    for warning in store.warnings() {
        app.out.warn(&warning.to_string());
    }
    if app.out.json_mode() {
        let task = store.get(id)?;
        let mut fields = vec![("task", app.task_json(&task))];
        if let Some(changed) = changed {
            fields.push(("changed", changed.into()));
        }
        return app.out.json(&json::document(fields));
    }
    app.out.print(line)
}

/// `path` as an existing directory's canonical path, like the script's
/// `cd "$path" && pwd`.
pub fn existing_dir(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().filter(|p| p.is_dir())
}
