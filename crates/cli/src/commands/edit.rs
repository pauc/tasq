//! `tasq set`, `tasq log` and `tasq done`: the small edits of the script.
//!
//! Each one reads the task, changes the model and hands the whole task back
//! to [`Store::update`] (or [`Store::set_done`]); the store works out the
//! file edits. `--json` prints the task as it is after the change.

use tasq_core::model::{Priority, Status, Task, TaskId, Workflow};
use tasq_core::store::Store;
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::error::{CliError, Result};
use crate::json;

/// What `tasq set` was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetValue {
    /// A workflow status.
    Status(Status),
    /// `A`, `B` or `C`.
    Priority(Priority),
}

impl SetValue {
    /// Interprets VALUE: a priority letter (with or without `#`) or a status
    /// of `workflow`.
    pub fn parse(value: &str, workflow: &Workflow) -> Result<Self> {
        if let Ok(priority) = value.parse::<Priority>() {
            return Ok(Self::Priority(priority));
        }
        if let Some(status) = workflow.parse_status(value) {
            return Ok(Self::Status(status));
        }
        Err(CliError::user(format!(
            "unknown status or priority '{value}' (statuses: {}; priorities: A B C)",
            workflow
                .statuses
                .iter()
                .map(Status::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        )))
    }

    /// The label the script printed: the status, or `priority #A`.
    pub fn label(&self) -> String {
        match self {
            Self::Status(status) => status.to_string(),
            Self::Priority(priority) => format!("priority {}", priority.to_hash()),
        }
    }

    fn apply(&self, task: &mut Task) {
        match self {
            Self::Status(status) => task.set_status(status.clone()),
            Self::Priority(priority) => task.set_priority(*priority),
        }
    }
}

/// `tasq set <id> <value> [note]`.
pub fn set(app: &App, id: &str, value: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let value = SetValue::parse(value, &app.workflow())?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let mut task = store.get(&id)?;
    value.apply(&mut task);
    if let Some(note) = note {
        task.log(note, clock.as_ref());
    }
    store.update(&task)?;
    let suffix = note.map_or_else(String::new, |n| format!(" ({n})"));
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] -> {}{suffix}\n", value.label()),
    )
}

/// `tasq log <id> <note>`.
pub fn log(app: &App, id: &str, note: &str) -> Result<()> {
    let id = App::task_id(id)?;
    if note.trim().is_empty() {
        return Err(CliError::user("the note must not be empty"));
    }
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let mut task = store.get(&id)?;
    task.log(note, clock.as_ref());
    store.update(&task)?;
    finish(app, &store, &id, &format!("[{id}] logged: {note}\n"))
}

/// `tasq done <id> [note]`: the note first, then `# [x]` with the status
/// tag removed, as the script did (`append_progress`, `nb todo do`,
/// `strip_status_tag`).
pub fn done(app: &App, id: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let mut task = store.get(&id)?;
    if let Some(note) = note {
        task.log(note, clock.as_ref());
        store.update(&task)?;
    }
    store.set_done(&id, true)?;
    finish(app, &store, &id, &format!("[{id}] done: {}\n", task.title))
}

/// Prints `line`, or the task as JSON.
fn finish(app: &App, store: &NbStore, id: &TaskId, line: &str) -> Result<()> {
    if app.out.json_mode() {
        let task = store.get(id)?;
        return app
            .out
            .json(&json::document([("task", json::to_value(&task))]));
    }
    app.out.print(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_value_parsing() {
        let wf = Workflow::default();
        assert_eq!(
            SetValue::parse("A", &wf).unwrap(),
            SetValue::Priority(Priority::A)
        );
        assert_eq!(
            SetValue::parse("#C", &wf).unwrap(),
            SetValue::Priority(Priority::C)
        );
        assert_eq!(
            SetValue::parse("#ready", &wf).unwrap(),
            SetValue::Status(Status::READY)
        );
        assert_eq!(
            SetValue::parse("nope", &wf).unwrap_err().to_string(),
            "unknown status or priority 'nope' (statuses: in-progress ready waiting blocked later; priorities: A B C)"
        );
        // Lowercase letters are tags in the file, so they are not priorities.
        assert!(SetValue::parse("a", &wf).is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(SetValue::Status(Status::BLOCKED).label(), "blocked");
        assert_eq!(SetValue::Priority(Priority::A).label(), "priority #A");
    }
}
