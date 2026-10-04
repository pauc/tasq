//! Projection of a [`Document`] onto the typed [`Task`].
//!
//! Follows the script's `parse_todo` awk: a section is a line starting with
//! `## `, all `## Tags` lines are concatenated, the first non-empty `## Due`
//! line wins, and the last status or priority tag seen wins.

use std::path::PathBuf;
use std::str::FromStr;

use crate::clock::parse_date;
use crate::model::{Priority, Tag, Task, TaskId, Workflow};

use super::document::{Document, Section};
use super::entry;

/// Names of the sections the format layer understands.
pub mod section {
    /// `## Description`.
    pub const DESCRIPTION: &str = "Description";
    /// `## Project`.
    pub const PROJECT: &str = "Project";
    /// `## Due`.
    pub const DUE: &str = "Due";
    /// `## Related`.
    pub const RELATED: &str = "Related";
    /// `### Merge requests`, inside `## Related`.
    pub const MERGE_REQUESTS: &str = "Merge requests";
    /// `## Tags`.
    pub const TAGS: &str = "Tags";
    /// `## Progress`.
    pub const PROGRESS: &str = "Progress";
    /// `## Worktrees`.
    pub const WORKTREES: &str = "Worktrees";
    /// `## Sessions`.
    pub const SESSIONS: &str = "Sessions";
    /// `## Source` (written only by tasq).
    pub const SOURCE: &str = "Source";
}

/// Builds the typed task from the document.
pub fn project(doc: &Document, id: TaskId, workflow: &Workflow) -> Task {
    let mut task = Task::new(id, doc.title());
    task.done = doc.is_done();

    for section in doc.sections() {
        match section.name {
            section::DESCRIPTION => read_description(&section, &mut task),
            section::PROJECT => {
                if task.project.is_none() {
                    task.project = first_non_empty(&section).map(PathBuf::from);
                }
            }
            section::DUE => {
                if task.due.is_none() {
                    task.due = first_non_empty(&section).and_then(|l| parse_date(l).ok());
                }
            }
            section::RELATED => read_related(&section, &mut task),
            section::TAGS => read_tags(&section, workflow, &mut task),
            section::PROGRESS => task
                .progress
                .extend(section.body.iter().filter_map(|l| entry::parse_progress(l))),
            section::WORKTREES => task
                .worktrees
                .extend(section.body.iter().filter_map(|l| entry::parse_worktree(l))),
            section::SESSIONS => task
                .sessions
                .extend(section.body.iter().filter_map(|l| entry::parse_session(l))),
            section::SOURCE if task.origin.is_none() => {
                task.origin = section.body.iter().find_map(|l| entry::parse_origin(l));
            }
            _ => {}
        }
    }
    if task.done {
        // Done tasks carry no status in the model; a stale tag left by
        // `nb todo do` without the script's strip stays in the document only.
        task.status = None;
    }
    task
}

fn first_non_empty<'a>(section: &Section<'a>) -> Option<&'a str> {
    section.body.iter().copied().find(|l| !l.trim().is_empty())
}

fn read_description(section: &Section<'_>, task: &mut Task) {
    let body = section.trimmed_body();
    if body.is_empty() {
        return;
    }
    let text = body.join("\n");
    match &mut task.description {
        Some(existing) => {
            existing.push('\n');
            existing.push_str(&text);
        }
        None => task.description = Some(text),
    }
}

fn read_related(section: &Section<'_>, task: &mut Task) {
    task.related.extend(
        section
            .own_body()
            .iter()
            .filter_map(|l| entry::parse_link(l)),
    );
    for sub in section.subsections() {
        if sub.name == section::MERGE_REQUESTS {
            task.merge_requests
                .extend(sub.body.iter().filter_map(|l| entry::parse_link(l)));
        }
    }
}

/// Classifies one `#token` from the tags line.
pub(super) enum TagKind {
    /// A workflow status.
    Status(crate::model::Status),
    /// `#A`, `#B` or `#C`.
    Priority(Priority),
    /// Anything else that is a valid tag.
    Topic(Tag),
}

/// Classifies a whitespace-separated token. Tokens without a leading `#`, and
/// tokens that are not valid tags (for example `##x`), yield `None`.
pub(super) fn classify(token: &str, workflow: &Workflow) -> Option<TagKind> {
    let name = token.strip_prefix('#')?;
    if let Some(status) = workflow.parse_status(name) {
        return Some(TagKind::Status(status));
    }
    if let Ok(priority) = Priority::from_str(name) {
        return Some(TagKind::Priority(priority));
    }
    Tag::new(name).ok().map(TagKind::Topic)
}

fn read_tags(section: &Section<'_>, workflow: &Workflow, task: &mut Task) {
    for token in section.body.iter().flat_map(|l| l.split_whitespace()) {
        match classify(token, workflow) {
            Some(TagKind::Status(s)) => task.status = Some(s),
            Some(TagKind::Priority(p)) => task.priority = p,
            Some(TagKind::Topic(t)) => {
                task.add_tag(t);
            }
            None => {}
        }
    }
}
