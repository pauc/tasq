//! Reports (plan T-601): what was worked on during a day, from the
//! progress notes, either raw or distilled by a [`Summarizer`].
//!
//! Collecting and rendering the notes is pure and lives here;
//! the summarizer that runs a command (an LLM) lives in `tasq-launch`,
//! because this crate spawns nothing. [`RawSummarizer`] is the no-dependency
//! default: it returns the notes as they are.

use std::fmt::Write as _;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::clock::format_date;
use crate::model::{Task, TaskId};
use crate::query::compare_ids;

/// The notes one task received on the day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNotes {
    /// The task.
    pub id: TaskId,
    /// Its title.
    pub title: String,
    /// Whether it is done now.
    pub done: bool,
    /// Its notes of the day, oldest first, without timestamps.
    pub notes: Vec<String>,
}

/// Everything logged on one day, one entry per task that has notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaySummary {
    /// The day.
    pub day: NaiveDate,
    /// Tasks with notes that day, in id order.
    pub tasks: Vec<TaskNotes>,
}

impl DaySummary {
    /// The notes of `day` across `tasks` (open and done), in id order.
    /// Tasks without a note that day are left out, and so are empty notes.
    pub fn collect(tasks: &[Task], day: NaiveDate) -> Self {
        let mut tasks: Vec<TaskNotes> = tasks
            .iter()
            .filter_map(|task| {
                let notes: Vec<String> = task
                    .progress
                    .iter()
                    .filter(|entry| entry.at.date() == day)
                    .map(|entry| entry.note.trim().to_owned())
                    .filter(|note| !note.is_empty())
                    .collect();
                (!notes.is_empty()).then(|| TaskNotes {
                    id: task.id.clone(),
                    title: task.title.clone(),
                    done: task.done,
                    notes,
                })
            })
            .collect();
        tasks.sort_by(|a, b| compare_ids(&a.id, &b.id));
        Self { day, tasks }
    }

    /// Whether nothing was logged that day.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// `Friday 2026-10-02`: the weekday name and the ISO date, the heading
    /// the original script used.
    pub fn header(&self) -> String {
        day_header(self.day)
    }

    /// The notes as the original script's `summary_raw` printed them: one
    /// bullet per task, `- [id] Title (done) — note` when the task has a
    /// single note, else the title line followed by one indented bullet per
    /// note. Ends with a newline; empty when nothing was logged.
    pub fn raw(&self) -> String {
        let mut out = String::new();
        for task in &self.tasks {
            let done = if task.done { " (done)" } else { "" };
            match task.notes.as_slice() {
                [note] => {
                    let _ = writeln!(out, "- [{}] {}{done} — {note}", task.id, task.title);
                }
                notes => {
                    let _ = writeln!(out, "- [{}] {}{done}", task.id, task.title);
                    for note in notes {
                        let _ = writeln!(out, "    - {note}");
                    }
                }
            }
        }
        out
    }
}

/// `Friday 2026-10-02`: the weekday name and the ISO date.
pub fn day_header(day: NaiveDate) -> String {
    format!("{} {}", day.format("%A"), format_date(day))
}

/// Turns a day's notes into the text `tasq summary` shows.
pub trait Summarizer {
    /// Name for messages (`raw`, `llm`).
    fn name(&self) -> &str;

    /// The summary text, without a heading. The caller adds the day header
    /// and renders the result as markdown.
    fn summarize(&self, summary: &DaySummary) -> Result<String, ReportError>;
}

/// The summarizer that changes nothing: the notes as [`DaySummary::raw`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawSummarizer;

impl Summarizer for RawSummarizer {
    fn name(&self) -> &'static str {
        "raw"
    }

    fn summarize(&self, summary: &DaySummary) -> Result<String, ReportError> {
        Ok(summary.raw())
    }
}

/// Why a summary could not be produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReportError {
    /// The summarizer cannot run here (its command is not installed, its
    /// configuration is incomplete).
    #[error("summarizer {summarizer}: {reason}")]
    Unavailable {
        /// The summarizer's name.
        summarizer: String,
        /// What is missing and how to fix it.
        reason: String,
    },
    /// The summarizer ran and failed.
    #[error("summarizer {summarizer} failed: {reason}")]
    Failed {
        /// The summarizer's name.
        summarizer: String,
        /// Its error output, or the exit status.
        reason: String,
    },
    /// The prompt template could not be rendered.
    #[error("summary prompt template: {0}")]
    Template(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, parse_date};
    use crate::model::ProgressEntry;

    fn day(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    fn task(id: u64, title: &str, notes: &[&str]) -> Task {
        let mut task = Task::new(TaskId::from(id), title);
        task.progress = notes
            .iter()
            .map(|n| {
                let (at, note) = n.split_once(": ").unwrap();
                match at.len() {
                    10 => ProgressEntry::dated(day(at), note),
                    _ => ProgressEntry::new(FixedClock::at(at).0, note),
                }
            })
            .collect();
        task
    }

    #[test]
    fn collects_that_days_notes_in_id_order_skipping_empty_ones() {
        let tasks = vec![
            task(
                12,
                "Later",
                &["2026-10-02 09:00: first", "2026-10-02 17:00:   "],
            ),
            task(3, "Earlier", &["2026-10-01 10:00: not that day"]),
            task(
                7,
                "Middle",
                &["2026-10-02: legacy", "2026-10-02 11:00:  spaced  "],
            ),
        ];
        let summary = DaySummary::collect(&tasks, day("2026-10-02"));
        assert_eq!(summary.day, day("2026-10-02"));
        assert_eq!(
            summary.tasks,
            vec![
                TaskNotes {
                    id: TaskId::from(7),
                    title: "Middle".into(),
                    done: false,
                    notes: vec!["legacy".into(), "spaced".into()],
                },
                TaskNotes {
                    id: TaskId::from(12),
                    title: "Later".into(),
                    done: false,
                    notes: vec!["first".into()],
                },
            ]
        );
        assert!(!summary.is_empty());
        assert_eq!(summary.header(), "Friday 2026-10-02");
    }

    #[test]
    fn done_tasks_are_included_and_marked() {
        let mut done = task(2, "Shipped", &["2026-10-03 16:30: shipped"]);
        done.done = true;
        let summary = DaySummary::collect(&[done], day("2026-10-03"));
        assert!(summary.tasks[0].done);
        assert_eq!(summary.raw(), "- [2] Shipped (done) — shipped\n");
    }

    #[test]
    fn empty_day() {
        let tasks = vec![task(1, "A", &["2026-10-01 10:00: x"])];
        let summary = DaySummary::collect(&tasks, day("2026-10-02"));
        assert!(summary.is_empty());
        assert_eq!(summary.tasks, Vec::new());
        assert_eq!(summary.raw(), "");
        assert_eq!(RawSummarizer.summarize(&summary), Ok(String::new()));
    }

    #[test]
    fn raw_matches_the_scripts_layout() {
        let summary = DaySummary {
            day: day("2026-10-05"),
            tasks: vec![
                TaskNotes {
                    id: TaskId::from(1),
                    title: "One note".into(),
                    done: false,
                    notes: vec!["did the thing".into()],
                },
                TaskNotes {
                    id: TaskId::from(10),
                    title: "Two notes".into(),
                    done: true,
                    notes: vec!["started".into(), "finished".into()],
                },
            ],
        };
        assert_eq!(
            summary.raw(),
            "- [1] One note — did the thing\n\
             - [10] Two notes (done)\n    - started\n    - finished\n"
        );
        assert_eq!(summary.header(), "Monday 2026-10-05");
        assert_eq!(RawSummarizer.name(), "raw");
        assert_eq!(RawSummarizer.summarize(&summary), Ok(summary.raw()));
    }

    #[test]
    fn headers_name_the_weekday() {
        assert_eq!(day_header(day("2026-10-04")), "Sunday 2026-10-04");
        assert_eq!(day_header(day("2026-01-01")), "Thursday 2026-01-01");
    }

    #[test]
    fn serde_shapes() {
        let summary = DaySummary {
            day: day("2026-10-02"),
            tasks: vec![TaskNotes {
                id: TaskId::from(4),
                title: "T".into(),
                done: false,
                notes: vec!["n".into()],
            }],
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert_eq!(
            json,
            "{\"day\":\"2026-10-02\",\"tasks\":[{\"id\":\"4\",\"title\":\"T\",\"done\":false,\"notes\":[\"n\"]}]}"
        );
        assert_eq!(serde_json::from_str::<DaySummary>(&json).unwrap(), summary);
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            ReportError::Unavailable {
                summarizer: "llm".into(),
                reason: "claude is not on PATH".into()
            }
            .to_string(),
            "summarizer llm: claude is not on PATH"
        );
        assert_eq!(
            ReportError::Failed {
                summarizer: "llm".into(),
                reason: "exit status 1".into()
            }
            .to_string(),
            "summarizer llm failed: exit status 1"
        );
        assert_eq!(
            ReportError::Template("unknown placeholder {{x}}".into()).to_string(),
            "summary prompt template: unknown placeholder {{x}}"
        );
    }
}
