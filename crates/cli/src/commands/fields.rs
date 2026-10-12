//! `tasq rename`, `tasq due`, `tasq tag` and `tasq desc`: one field of a
//! task each, after creation.
//!
//! Like [`crate::commands::edit`], every command is the argument parsing,
//! one [`tasq_core::edit`] call and the printed line (or, with `--json`,
//! the task as stored). A task that changed on disk since it was read
//! fails with the store's conflict error; nothing retries.

use chrono::NaiveDate;
use tasq_core::dates::parse_day;
use tasq_core::edit::{self, TagChange};

use crate::app::App;
use crate::commands::finish;
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
