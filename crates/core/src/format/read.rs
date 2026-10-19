//! Projection of a [`Document`] onto the typed [`Task`].
//!
//! The scalar fields come from the frontmatter ([`key`]); a value that does
//! not parse (`priority: D`, `due: next friday`, `status: nope`) reads as
//! unset and stays in the file. The body is read section by section, as
//! the script's `parse_todo` awk did: a section is a line starting with
//! `## `, duplicate sections concatenate, and only the list sections are
//! read (a `## Due` or `## Tags` section in the body is an unknown section).

use std::path::PathBuf;
use std::str::FromStr;

use crate::clock::{When, parse_date, parse_timestamp};
use crate::model::{Origin, Priority, Tag, Task, Workflow};

use super::document::{Document, Frontmatter, Section, key};
use super::entry;

/// Names of the body sections the format layer understands; any other
/// `##` section is unknown and kept as it is.
pub mod section {
    /// `## Description`.
    pub const DESCRIPTION: &str = "Description";
    /// `## Related`.
    pub const RELATED: &str = "Related";
    /// `### Merge requests`, inside `## Related`.
    pub const MERGE_REQUESTS: &str = "Merge requests";
    /// `## Progress`.
    pub const PROGRESS: &str = "Progress";
    /// `## Worktrees`.
    pub const WORKTREES: &str = "Worktrees";
    /// `## Sessions`.
    pub const SESSIONS: &str = "Sessions";
}

/// Builds the typed task from the document.
pub fn project(doc: &Document, workflow: &Workflow) -> Task {
    let mut task = Task::new(doc.id(), doc.title());
    task.done = doc.is_done();
    read_frontmatter(&doc.frontmatter(), workflow, &mut task);

    for section in doc.sections() {
        match section.name {
            section::DESCRIPTION => read_description(&section, &mut task),
            section::RELATED => read_related(&section, &mut task),
            section::PROGRESS => task
                .progress
                .extend(section.body.iter().filter_map(|l| entry::parse_progress(l))),
            section::WORKTREES => task
                .worktrees
                .extend(section.body.iter().filter_map(|l| entry::parse_worktree(l))),
            section::SESSIONS => task
                .sessions
                .extend(section.body.iter().filter_map(|l| entry::parse_session(l))),
            _ => {}
        }
    }
    if task.done {
        // Done tasks carry no status in the model: `status: done` is the
        // marker, not a status.
        task.status = None;
    } else {
        // An open task was not closed; a `closed` key left on one stays in
        // the document only.
        task.closed_at = None;
    }
    task
}

/// The scalar fields, from the block. `status: done` is the done marker:
/// the task is done and has no status. Any other `status` is looked up in
/// the workflow; a name it does not know reads as no status.
fn read_frontmatter(front: &Frontmatter<'_>, workflow: &Workflow, task: &mut Task) {
    let get = |k: &str| front.get(k).filter(|v| !v.is_empty());
    task.status = get(key::STATUS).and_then(|s| workflow.parse_status(s));
    task.priority = get(key::PRIORITY)
        .and_then(|p| Priority::from_str(p).ok())
        .unwrap_or_default();
    task.rank = get(key::RANK).and_then(|r| r.parse().ok());
    task.due = get(key::DUE).and_then(|d| parse_date(d).ok());
    task.closed_at = get(key::CLOSED).and_then(|c| match parse_timestamp(c) {
        Ok(When::DateTime(at)) => Some(at),
        Ok(When::Date(_)) | Err(_) => None,
    });
    task.project = get(key::PROJECT).map(PathBuf::from);
    task.origin = get(key::SOURCE).and_then(|source| {
        Some(Origin {
            source: source.to_owned(),
            external_id: get(key::SOURCE_ID)?.to_owned(),
            url: get(key::SOURCE_URL).map(str::to_owned),
        })
    });
    for tag in get(key::TAGS).map(parse_tag_list).unwrap_or_default() {
        task.add_tag(tag);
    }
}

/// `[a, b, c]`: the items between the brackets, split on commas and
/// trimmed; items that are not valid tags are dropped. Without the
/// brackets there are no tags.
pub(super) fn parse_tag_list(value: &str) -> Vec<Tag> {
    let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
        return Vec::new();
    };
    inner
        .split(',')
        .filter_map(|item| Tag::new(item.trim()).ok())
        .collect()
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
