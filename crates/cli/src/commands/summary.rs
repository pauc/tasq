//! `tasq summary [DAY] [--raw]`: the day's progress notes, raw or distilled
//! (plan T-601).
//!
//! The notes are collected in core ([`DaySummary::collect`]); the
//! summarizer comes from `[report.summary]` through
//! [`tasq_launch::summarizer_for`], and `--raw` always picks the raw one.
//! Raw output is the bold day header over the notes, as the script printed
//! it; a distilled summary is `## <header>` plus the command's output,
//! shown the way `tasq view` shows markdown.

use std::io::IsTerminal;

use serde_json::Value;
use tasq_core::dates::{last_working_day, parse_past_day};
use tasq_core::query::Filter;
use tasq_core::report::DaySummary;
use tasq_core::store::Store;
use tasq_launch::summarizer::DEFAULT_TEMPLATE;
use tasq_launch::summarizer_for;

use crate::app::App;
use crate::commands::view::show_markdown;
use crate::error::{CliError, Result};
use crate::json;

/// Runs `summary`.
pub fn run(app: &App, day: Option<&str>, raw: bool) -> Result<()> {
    let today = app.clock()?.today();
    let day = match day {
        None => last_working_day(today),
        Some(text) => parse_past_day(text, today)?,
    };
    let store = app.open_store()?;
    let tasks = store.list(&Filter::default().any_done())?;
    let summary = DaySummary::collect(&tasks, day);
    let header = summary.header();

    let config = &app.config().report.summary;
    let template = match &config.prompt_file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| {
            CliError::user(format!(
                "report.summary.prompt_file {}: {e}",
                file.display()
            ))
        })?,
        None => DEFAULT_TEMPLATE.to_owned(),
    };
    let summarizer = summarizer_for(config, template, app.env_vec(), raw);
    let is_raw = summarizer.name() == "raw";

    if summary.is_empty() {
        if app.out.json_mode() {
            return app.out.json(&document(&summary, summarizer.name(), None));
        }
        return app.out.print(&format!("Nothing logged on {header}.\n"));
    }
    if is_raw {
        if app.out.json_mode() {
            return app.out.json(&document(&summary, "raw", None));
        }
        let style = app.out.style();
        return app
            .out
            .print(&format!("{}\n{}", style.bold(&header), summary.raw()));
    }
    if std::io::stderr().is_terminal() {
        eprintln!(
            "{}",
            app.out
                .style()
                .dim(&format!("summarizing with {}…", program(&config.command)))
        );
    }
    let text = summarizer.summarize(&summary)?;
    if app.out.json_mode() {
        return app
            .out
            .json(&document(&summary, summarizer.name(), Some(&text)));
    }
    show_markdown(app, &format!("## {header}\n\n{text}\n"))
}

/// The first word of `report.summary.command`, for the progress line.
fn program(command: &str) -> String {
    shell_words::split(command)
        .ok()
        .and_then(|argv| argv.into_iter().next())
        .unwrap_or_else(|| command.to_owned())
}

/// The `--json` document: the day, its header, the summarizer used, the
/// per-task notes, the raw text and the distilled text (`null` when raw).
fn document(summary: &DaySummary, summarizer: &str, text: Option<&str>) -> Value {
    json::document([
        ("day", json::to_value(&summary.day)),
        ("header", Value::from(summary.header())),
        ("summarizer", Value::from(summarizer)),
        ("tasks", json::to_value(&summary.tasks)),
        ("notes", Value::from(summary.raw())),
        ("summary", text.map_or(Value::Null, Value::from)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_is_the_first_word() {
        assert_eq!(program("claude -p"), "claude");
        assert_eq!(program("'my llm' --x"), "my llm");
        assert_eq!(program("bad 'quote"), "bad 'quote");
    }
}
