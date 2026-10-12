//! `tasq rename`, `tasq due`, `tasq tag` and `tasq desc`: one field of a
//! task each, after creation.
//!
//! Like [`crate::commands::edit`], every command is the argument parsing,
//! one [`tasq_core::edit`] call and the printed line (or, with `--json`,
//! the task as stored). A task that changed on disk since it was read
//! fails with the store's conflict error; nothing retries.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus};

use chrono::NaiveDate;
use tasq_core::dates::parse_day;
use tasq_core::edit::{self, TagChange};
use tasq_core::model::{Task, TaskId};
use tasq_core::store::Store;

use crate::app::App;
use crate::commands::finish;
use crate::editor::configured_editor;
use crate::error::{CliError, Result};

/// `tasq rename <id> <title> [--note TEXT]`.
pub fn rename(app: &App, id: &str, title: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    edit::rename(&mut store, &id, title, note, clock.as_ref())?;
    finish(app, &store, &id, &format!("[{id}] renamed\n"))
}

/// `tasq due <id> <date|none> [--note TEXT]`.
pub fn due(app: &App, id: &str, date: &str, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let clock = app.clock()?;
    let day = due_date(date, clock.today())?;
    let mut store = app.open_store()?;
    edit::due(&mut store, &id, day, note, clock.as_ref())?;
    let line = day.map_or_else(
        || format!("[{id}] due cleared\n"),
        |day| format!("[{id}] due {day}\n"),
    );
    finish(app, &store, &id, &line)
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
    edit::retag(&mut store, &id, &changes, note, clock.as_ref())?;
    let labels: Vec<String> = changes.iter().map(TagChange::label).collect();
    finish(
        app,
        &store,
        &id,
        &format!("[{id}] tags: {}\n", labels.join(" ")),
    )
}

/// `tasq desc <id> [text|-] [--note TEXT]`: prints the description
/// without TEXT, reads it from stdin for `-`, sets it otherwise.
pub fn desc(app: &App, id: &str, text: Option<&str>, note: Option<&str>) -> Result<()> {
    let id = App::task_id(id)?;
    let Some(text) = text else {
        return print_description(app, &id);
    };
    let text = match text {
        "-" => read_stdin()?,
        text => text.to_owned(),
    };
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let task = edit::describe(&mut store, &id, &text, note, clock.as_ref())?;
    finish(app, &store, &id, &described(&id, &task))
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
    let before = task
        .description
        .as_ref()
        .map_or_else(String::new, |text| format!("{text}\n"));
    let mut file = tempfile::Builder::new()
        .prefix("tasq-desc-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(before.as_bytes())?;
    file.flush()?;
    let program = &editor[0];
    let status = run_editor(&editor, file.path())
        .map_err(|e| CliError::user(format!("could not run {program}: {e}")))?;
    if !status.success() {
        return Err(CliError::user(format!(
            "{program} exited with {status}; the description was not changed"
        )));
    }
    let after = std::fs::read_to_string(file.path())?;
    if edit::description_text(&after) == task.description {
        return finish(app, &store, &id, &format!("[{id}] description unchanged\n"));
    }
    let clock = app.clock()?;
    let task = edit::describe_read(&mut store, task, &after, note, clock.as_ref())?;
    finish(app, &store, &id, &described(&id, &task))
}

// Spawns the user's editor on a terminal and waits; nothing to assert.
#[mutants::skip]
fn run_editor(editor: &[String], path: &Path) -> std::io::Result<ExitStatus> {
    Command::new(&editor[0])
        .args(&editor[1..])
        .arg(path)
        .status()
}

/// The line `desc` prints after a write.
fn described(id: &TaskId, task: &Task) -> String {
    let what = if task.description.is_some() {
        "updated"
    } else {
        "cleared"
    };
    format!("[{id}] description {what}\n")
}

/// `tasq desc <id>`: the description and a newline, nothing when there is
/// none; with `--json`, the task.
fn print_description(app: &App, id: &TaskId) -> Result<()> {
    let store = app.open_store()?;
    let task = store.get(id)?;
    let text = task
        .description
        .map_or_else(String::new, |text| format!("{text}\n"));
    finish(app, &store, id, &text)
}

/// Stdin until EOF, one trailing newline dropped (`-` as in `cat -`).
fn read_stdin() -> Result<String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text.strip_suffix('\n').unwrap_or(&text).to_owned())
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
