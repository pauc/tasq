//! Writing a whole [`Task`] as a new [`Document`]: the canonical file
//! (ADR 0026, [`Document::from_task`]).

use crate::clock::{format_date, format_timestamp};
use crate::model::Task;

use super::document::{Document, FENCE, TITLE_PREFIX, key};
use super::entry;
use super::ops;
use super::read::section;

/// Appends a `## name` section to `lines` the way `cmd_create` did: a blank
/// line, the heading, and, when there is a body, a blank line and the body.
fn push_section(lines: &mut Vec<String>, name: &str, body: &[String]) {
    lines.push(String::new());
    lines.push(format!("## {name}"));
    if !body.is_empty() {
        lines.push(String::new());
        lines.extend(body.iter().cloned());
    }
}

/// The `key: value` lines of the canonical frontmatter block of `task`, in
/// [`key::ORDER`]: `id` and `priority` always; `status` when the task has
/// one, or `done` when it is done; the other keys when set. `closed` is
/// written on a done task only, `source_url` only when the origin has one,
/// `tags` only when there are any.
fn frontmatter_lines(task: &Task) -> Vec<String> {
    let entry = |k: &str, v: &str| format!("{k}: {v}");
    let mut lines = vec![entry(key::ID, &task.id.to_string())];
    if task.done {
        lines.push(entry(key::STATUS, key::DONE_STATUS));
    } else if let Some(status) = &task.status {
        lines.push(entry(key::STATUS, status.as_str()));
    }
    lines.push(entry(key::PRIORITY, task.priority.as_str()));
    if let Some(rank) = task.rank {
        lines.push(entry(key::RANK, &rank.to_string()));
    }
    if let Some(due) = task.due {
        lines.push(entry(key::DUE, &format_date(due)));
    }
    if let Some(at) = task.closed_at.filter(|_| task.done) {
        lines.push(entry(key::CLOSED, &format_timestamp(at)));
    }
    if let Some(project) = &task.project {
        lines.push(entry(key::PROJECT, &project.display().to_string()));
    }
    if let Some(origin) = &task.origin {
        lines.push(entry(key::SOURCE, &origin.source));
        lines.push(entry(key::SOURCE_ID, &origin.external_id));
        if let Some(url) = &origin.url {
            lines.push(entry(key::SOURCE_URL, url));
        }
    }
    if !task.tags.is_empty() {
        lines.push(entry(key::TAGS, &entry::format_tag_list(&task.tags)));
    }
    lines
}

impl Document {
    /// Renders a task as a fresh file (ADR 0026): the frontmatter block
    /// (`frontmatter_lines`), `# Title`, then the list sections in the
    /// order `cmd_create` wrote them (Description, Related, Progress), then
    /// merge requests, Worktrees and Sessions through the operations the
    /// commands use. `## Progress` is always present; no scalar is ever
    /// written as a section.
    pub fn from_task(task: &Task) -> Self {
        let mut lines = vec![FENCE.to_owned()];
        lines.extend(frontmatter_lines(task));
        lines.push(FENCE.to_owned());
        lines.push(format!("{TITLE_PREFIX}{}", task.title));
        let mut section = |name: &str, body: &[String]| push_section(&mut lines, name, body);
        if let Some(desc) = task.description.as_deref().filter(|d| !d.is_empty()) {
            section(section::DESCRIPTION, &[desc.to_owned()]);
        }
        if !task.related.is_empty() {
            let related: Vec<String> = task.related.iter().map(entry::format_link).collect();
            section(section::RELATED, &related);
        }
        let progress: Vec<String> = task.progress.iter().map(entry::format_progress).collect();
        section(section::PROGRESS, &progress);
        let mut text = lines.join("\n");
        text.push('\n');
        let mut doc = Self::parse(&text).expect("a rendered block and title line always parse");
        for mr in &task.merge_requests {
            ops::append_merge_request(&mut doc, mr);
        }
        for worktree in &task.worktrees {
            ops::append_worktree(&mut doc, worktree);
        }
        for session in &task.sessions {
            ops::append_session(&mut doc, session);
        }
        doc
    }
}
