//! Projection of a [`Document`] onto the typed [`Task`].
//!
//! For a v1 file it follows the script's `parse_todo` awk: a section is a
//! line starting with `## `, all `## Tags` lines are concatenated, the first
//! non-empty `## Due` line wins, and the last status or priority tag seen
//! wins. `## Closed` (tasq only) reads like `## Due`, as a `YYYY-MM-DD HH:MM`
//! timestamp, and `## Rank` (tasq only) the same way, as an integer.
//!
//! For a v2 file (ADR 0026) the scalar fields come from the frontmatter
//! ([`key`]) and only the list sections of the body are read; a `## Due` or
//! `## Tags` section in a v2 body is an unknown section. A value that does
//! not parse (`priority: D`, `due: next friday`, `status: nope`) reads as
//! unset and stays in the file, like an invalid `## Rank` does in v1.

use std::path::PathBuf;
use std::str::FromStr;

use crate::clock::{When, parse_date, parse_timestamp};
use crate::model::{Origin, Priority, Tag, Task, TaskId, Workflow};

use super::document::{Document, Frontmatter, Section};
use super::entry;

/// Names of the frontmatter keys of a v2 task file (ADR 0026).
pub mod key {
    /// `id`: the task's id, a positive integer; required.
    pub const ID: &str = "id";
    /// `status`: a workflow status name, or [`DONE_STATUS`].
    pub const STATUS: &str = "status";
    /// The `status` value of a done task.
    pub const DONE_STATUS: &str = "done";
    /// `priority`: `A`, `B` or `C`.
    pub const PRIORITY: &str = "priority";
    /// `rank`: an integer (ADR 0025).
    pub const RANK: &str = "rank";
    /// `due`: `YYYY-MM-DD`.
    pub const DUE: &str = "due";
    /// `closed`: `YYYY-MM-DD HH:MM`, read on done tasks only.
    pub const CLOSED: &str = "closed";
    /// `project`: a directory path.
    pub const PROJECT: &str = "project";
    /// `source`: the origin's source name.
    pub const SOURCE: &str = "source";
    /// `source_id`: the origin's external id.
    pub const SOURCE_ID: &str = "source_id";
    /// `source_url`: the origin's url.
    pub const SOURCE_URL: &str = "source_url";
    /// `tags`: the topic tags as a one-line list, `[a, b]`, without `#`.
    pub const TAGS: &str = "tags";
}

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
    /// `## Closed` (written only by tasq, ADR 0020).
    pub const CLOSED: &str = "Closed";
    /// `## Rank` (written only by tasq, ADR 0025).
    pub const RANK: &str = "Rank";
}

/// Builds the typed task from the document. `id` names a v1 task, whose
/// file does not know it; a v2 file carries its own and `id` is not used.
pub fn project(doc: &Document, id: TaskId, workflow: &Workflow) -> Task {
    let front = doc.frontmatter();
    let id = doc.id().unwrap_or(id);
    let mut task = Task::new(id, doc.title());
    task.done = doc.is_done();
    if let Some(front) = &front {
        read_frontmatter(front, workflow, &mut task);
    }

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
            // The scalar sections of v1 are unknown sections in a v2 body.
            _ if front.is_some() => {}
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
            section::TAGS => read_tags(&section, workflow, &mut task),
            section::SOURCE if task.origin.is_none() => {
                task.origin = section.body.iter().find_map(|l| entry::parse_origin(l));
            }
            section::CLOSED if task.closed_at.is_none() => {
                task.closed_at = first_non_empty(&section).and_then(|l| match parse_timestamp(l) {
                    Ok(When::DateTime(at)) => Some(at),
                    Ok(When::Date(_)) | Err(_) => None,
                });
            }
            section::RANK if task.rank.is_none() => {
                task.rank = first_non_empty(&section).and_then(|l| l.trim().parse().ok());
            }
            _ => {}
        }
    }
    if task.done {
        // Done tasks carry no status in the model; a stale tag left by
        // `nb todo do` without the script's strip stays in the document only.
        task.status = None;
    } else {
        // An open task was not closed; a `## Closed` left by `nb todo undo`
        // stays in the document only.
        task.closed_at = None;
    }
    task
}

/// The scalar fields of a v2 task, from the block. `status: done` is the
/// done marker: the task is done and has no status. Any other `status` is
/// looked up in the workflow; a name it does not know reads as no status.
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
        let external_id = get(key::SOURCE_ID)?;
        Some(Origin {
            source: source.to_owned(),
            external_id: external_id.to_owned(),
            url: get(key::SOURCE_URL)
                .or_else(|| entry::is_url(external_id).then_some(external_id))
                .map(str::to_owned),
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
/// tokens that are not valid tags (for example `##x`, `##A` or `##ready`),
/// yield `None`: the script matched `^#(A|B|C)$` and `^#(status)$`, so a
/// second `#` never made a priority or a status.
pub(super) fn classify(token: &str, workflow: &Workflow) -> Option<TagKind> {
    let name = token.strip_prefix('#')?;
    if name.starts_with('#') {
        return None;
    }
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
