//! `tasq set`, `tasq log`, `tasq done` and `tasq reopen`: the small edits
//! of the script, plus the inverse of `done`.
//!
//! The work is [`tasq_core::edit`], which the TUI calls too; this module
//! parses the arguments, maps the errors and prints the result (or, with
//! `--json`, the task as it is after the change).

use tasq_core::edit::{self, EditError, Value};
use tasq_core::model::{Status, Workflow};
use tasq_core::store::Store;

use crate::app::App;
use crate::commands::finish;
use crate::error::{CliError, Result};
use crate::plugins::{self, Hook};

/// Interprets VALUE: a priority letter (with or without `#`) or a status
/// of `workflow`; anything else is a user error listing both.
pub fn parse_value(value: &str, workflow: &Workflow) -> Result<Value> {
    Value::parse(value, workflow).ok_or_else(|| {
        CliError::user(format!(
            "unknown status or priority '{value}' (statuses: {}; priorities: A B C)",
            status_names(workflow)
        ))
    })
}

/// `tasq set <id> <value> [note]`.
pub fn set(app: &App, id: &str, value: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let value = parse_value(value, &app.workflow())?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    edit::set(&mut store, &id, &value, note, clock.as_ref())?;
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
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    edit::log(&mut store, &id, note, clock.as_ref())?;
    finish(app, &store, &id, &format!("[{id}] logged: {note}\n"))
}

/// `tasq done <id> [note]`: the note first, then `# [x]` with the status
/// tag removed, as the script did (`append_progress`, `nb todo do`,
/// `strip_status_tag`).
pub fn done(app: &App, id: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let task = edit::done(&mut store, &id, note, clock.as_ref())?;
    if !app.config().hooks.post_done.is_empty() {
        plugins::run_hooks(app, Hook::PostDone, &store.get(&id)?, &[])?;
    }
    finish(app, &store, &id, &format!("[{id}] done: {}\n", task.title))
}

/// Splits `tasq reopen`'s optional words into a status and a note: a first
/// word that is a status of `workflow` is the status, otherwise `default`
/// is and the word is the note. Two words where the first is not a status
/// is a user error.
pub fn reopen_args<'a>(
    first: Option<&'a str>,
    second: Option<&'a str>,
    workflow: &Workflow,
    default: &Status,
) -> Result<(Status, Option<&'a str>)> {
    match (first.map(|w| (w, workflow.parse_status(w))), second) {
        (None, _) => Ok((default.clone(), None)),
        (Some((_, Some(status))), note) => Ok((status, note)),
        (Some((word, None)), None) => Ok((default.clone(), Some(word))),
        (Some((word, None)), Some(_)) => Err(CliError::user(format!(
            "unknown status '{word}' (statuses: {})",
            status_names(workflow)
        ))),
    }
}

/// `tasq reopen <id> [status] [note]`.
pub fn reopen(app: &App, id: &str, first: Option<&str>, second: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let workflow = app.workflow();
    let (status, note) = reopen_args(
        first,
        second,
        &workflow,
        &app.config().workflow.default_status,
    )?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let task = edit::reopen(&mut store, &id, &status, note, clock.as_ref())?;
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] reopened -> {status}: {}\n", task.title),
    )
}

/// The workflow's statuses, space separated, for error messages.
fn status_names(workflow: &Workflow) -> String {
    workflow
        .statuses
        .iter()
        .map(Status::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

impl From<EditError> for CliError {
    fn from(e: EditError) -> Self {
        Self::User(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use tasq_core::model::Priority;

    use super::*;

    #[test]
    fn value_parsing() {
        let wf = Workflow::default();
        assert_eq!(parse_value("A", &wf).unwrap(), Value::Priority(Priority::A));
        assert_eq!(
            parse_value("#ready", &wf).unwrap(),
            Value::Status(Status::READY)
        );
        assert_eq!(
            parse_value("nope", &wf).unwrap_err().to_string(),
            "unknown status or priority 'nope' (statuses: in-progress ready waiting blocked later; priorities: A B C)"
        );
    }

    #[test]
    fn reopen_args_split_status_and_note() {
        let wf = Workflow::default();
        let ready = Status::READY;
        assert_eq!(
            reopen_args(None, None, &wf, &ready).unwrap(),
            (ready.clone(), None)
        );
        assert_eq!(
            reopen_args(Some("#blocked"), None, &wf, &ready).unwrap(),
            (Status::BLOCKED, None)
        );
        assert_eq!(
            reopen_args(Some("later"), Some("next quarter"), &wf, &ready).unwrap(),
            (Status::LATER, Some("next quarter"))
        );
        assert_eq!(
            reopen_args(Some("not merged"), None, &wf, &ready).unwrap(),
            (ready.clone(), Some("not merged"))
        );
        assert_eq!(
            reopen_args(Some("soon"), Some("x"), &wf, &ready)
                .unwrap_err()
                .to_string(),
            "unknown status 'soon' (statuses: in-progress ready waiting blocked later)"
        );
    }

    #[test]
    fn edit_errors_are_user_errors() {
        let e: CliError = EditError::EmptyNote.into();
        assert!(matches!(e, CliError::User(ref m) if m == "the note must not be empty"));
    }
}
