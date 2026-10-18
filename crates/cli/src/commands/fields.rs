//! `tasq rename`, `tasq due`, `tasq tag` and `tasq desc`: one field of a
//! task each, after creation.
//!
//! Like [`crate::commands::edit`], every command is the argument parsing,
//! one [`tasq_core::edit`] call and the printed line (or, with `--json`,
//! the task as stored and the fields written). A task that changed on disk since it was read
//! fails with the store's conflict error; nothing retries.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus};

use chrono::NaiveDate;
use tasq_core::dates::parse_day;
use tasq_core::edit::{self, Revised, TagChange};
use tasq_core::model::{Task, TaskId};
use tasq_core::store::Store;

use crate::app::App;
use crate::commands::{finish, finish_edit};
use crate::editor::configured_editor;
use crate::error::{CliError, Result};

/// `tasq rename <id> <title> [--note TEXT]`.
pub fn rename(app: &App, id: &str, title: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let revised = edit::rename(&mut store, &id, title, note, clock.as_ref())?;
    let line = if revised.changed.is_empty() {
        format!("[{id}] title unchanged\n")
    } else {
        format!("[{id}] renamed\n")
    };
    finish_edit(app, &mut store, &id, &line, &revised.changed)
}

/// `tasq due <id> <date|none> [--note TEXT]`.
pub fn due(app: &App, id: &str, date: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let clock = app.clock()?;
    let day = due_date(date, clock.today())?;
    let mut store = app.open_store()?;
    let revised = edit::due(&mut store, &id, day, note, clock.as_ref())?;
    let line = match day {
        _ if revised.changed.is_empty() => format!("[{id}] due unchanged\n"),
        None => format!("[{id}] due cleared\n"),
        Some(day) => format!("[{id}] due {day}\n"),
    };
    finish_edit(app, &mut store, &id, &line, &revised.changed)
}

/// `tasq tag <id> <tag>... [--note TEXT]`. Every word is checked before
/// the task is read.
pub fn tag(app: &App, id: &str, words: &[String], note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let workflow = app.workflow();
    let changes = words
        .iter()
        .map(|word| TagChange::parse(word, &workflow))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let done = edit::retag(&mut store, &id, &changes, note, clock.as_ref())?;
    let (line, written): (String, &[&str]) = if done.applied.is_empty() {
        (format!("[{id}] tags unchanged\n"), &[])
    } else {
        let labels: Vec<String> = done.applied.iter().map(TagChange::label).collect();
        (format!("[{id}] tags: {}\n", labels.join(" ")), &["tags"])
    };
    finish_edit(app, &mut store, &id, &line, written)
}

/// `tasq desc <id> [text|-] [--note TEXT]`: prints the description
/// without TEXT, reads it from stdin for `-` (after reading the task),
/// sets it otherwise.
pub fn desc(app: &App, id: &str, text: Option<&str>, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let Some(text) = text else {
        return print_description(app, &id);
    };
    let mut store = app.open_store()?;
    if text == "-" {
        // Read before draining stdin: in `tasq desc 31 | ... | tasq desc 31 -`
        // both ends start together, so a change made while the producer
        // runs is then a conflict instead of being overwritten.
        let task = store.get(&id)?;
        let text = read_stdin()?;
        return save_read(app, &mut store, task, &text, note);
    }
    let clock = app.clock()?;
    let revised = edit::describe(&mut store, &id, text, note, clock.as_ref())?;
    finish_edit(
        app,
        &mut store,
        &id,
        &described(&id, &revised),
        &revised.changed,
    )
}

/// `tasq desc <id> --edit [--note TEXT]`: the description in `$VISUAL`
/// or `$EDITOR`. The task is read before the editor starts and written
/// with that read, so a change made meanwhile is the store's conflict.
pub fn desc_edit(app: &App, id: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let editor = configured_editor(&app.opts.env)
        .ok_or_else(|| CliError::user("no editor: set $VISUAL or $EDITOR"))?;
    let mut store = app.open_store()?;
    let task = store.get(&id)?;
    let before = as_lines(task.description.as_deref());
    let mut file = tempfile::Builder::new()
        .prefix("tasq-desc-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(before.as_bytes())?;
    file.flush()?;
    let program = &editor[0];
    let status = run_editor(&editor, file.path())
        .map_err(|e| CliError::user(format!("could not run {program}: {e}")))?;
    let after = std::fs::read_to_string(file.path())?;
    // Worth keeping when a save fails: the text says something new.
    let edited = edit::description_text(&after) != task.description;
    let saved = if status.success() {
        save_read(app, &mut store, task, &after, note)
    } else {
        Err(CliError::user(format!(
            "{program} exited with {status}; the description was not changed"
        )))
    };
    match saved {
        Err(e) if edited => {
            let (_, path) = file.keep().map_err(|e| CliError::Internal(e.into()))?;
            Err(kept_in(e, &path))
        }
        other => other,
    }
}

/// Writes `after` as the description of `task`, a read from before stdin
/// was drained or the editor started (`desc -`, `desc --edit`).
fn save_read(
    app: &App,
    store: &mut dyn Store,
    task: Task,
    after: &str,
    note: Option<&str>,
) -> Result<()> {
    let id = task.id.clone();
    let clock = app.clock()?;
    let revised = edit::describe_read(store, task, after, note, clock.as_ref())?;
    finish_edit(app, store, &id, &described(&id, &revised), &revised.changed)
}

/// `error` with where the edited text was kept, so a failed save never
/// loses what was typed in the editor.
fn kept_in(error: CliError, path: &Path) -> CliError {
    let kept = format!("the edited text is kept in {}", path.display());
    match error {
        CliError::User(message) => CliError::User(format!("{message}; {kept}")),
        CliError::Internal(e) => CliError::Internal(e.context(kept)),
        silent @ CliError::Silent(_) => silent,
    }
}

/// Reason: spawns the user's editor on a terminal and waits; nothing to assert.
#[mutants::skip]
fn run_editor(editor: &[String], path: &Path) -> std::io::Result<ExitStatus> {
    Command::new(&editor[0])
        .args(&editor[1..])
        .arg(path)
        .status()
}

/// The line `desc` prints: what happened to the description.
fn described(id: &TaskId, revised: &Revised) -> String {
    let what = if revised.changed.is_empty() {
        "unchanged"
    } else if revised.task.description.is_some() {
        "updated"
    } else {
        "cleared"
    };
    format!("[{id}] description {what}\n")
}

/// `tasq desc <id>`: the description and a newline, nothing when there is
/// none; with `--json`, the task.
fn print_description(app: &App, id: &TaskId) -> Result<()> {
    let mut store = app.open_store()?;
    let task = store.get(id)?;
    finish(app, &mut store, id, &as_lines(task.description.as_deref()))
}

/// A description as lines of text: a newline after it, nothing for none.
fn as_lines(description: Option<&str>) -> String {
    description.map_or_else(String::new, |text| format!("{text}\n"))
}

/// Stdin until EOF (`-` as in `cat -`), as is: the trailing newline and
/// any blank lines at either end go in [`edit::description_text`].
fn read_stdin() -> Result<String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

/// Interprets `tasq due`'s DATE: `none` (any case) clears, anything else
/// goes through [`parse_day`], the parser of `create --due`.
pub fn due_date(text: &str, today: NaiveDate) -> Result<Option<NaiveDate>> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    parse_day(text, today).map(Some).map_err(|_| {
        CliError::user(format!(
            "invalid due date {text:?} (expected YYYY-MM-DD, today, tomorrow, yesterday or none)"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        assert_eq!(due_date("none", today).unwrap(), None);
        assert_eq!(due_date(" None ", today).unwrap(), None);
        assert_eq!(
            due_date("tomorrow", today).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 8)
        );
        assert_eq!(
            due_date("2026-12-01", today).unwrap(),
            NaiveDate::from_ymd_opt(2026, 12, 1)
        );
        assert_eq!(
            due_date("nope", today).unwrap_err().to_string(),
            "invalid due date \"nope\" (expected YYYY-MM-DD, today, tomorrow, yesterday or none)"
        );
    }
}
