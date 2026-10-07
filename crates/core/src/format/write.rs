//! Writing a whole [`Task`] as a new [`Document`], in the shape and order
//! `cmd_create` used.

use crate::model::Task;

use super::document::{DONE_PREFIX, Document, OPEN_PREFIX};
use super::entry;
use super::ops;
use super::read::section;

impl Document {
    /// Renders a task as a fresh file, section by section in the order
    /// `cmd_create` wrote them (Description, Project, Due, Related, Tags,
    /// Progress), then the sections the script added with later commands
    /// (merge requests, Worktrees, Sessions) through the same operations the
    /// commands used. `## Source` goes after `## Due`, then `## Closed` on a
    /// done task that has a `closed_at`.
    ///
    /// Only `## Tags` and `## Progress` are always present; `## Tags` holds
    /// topic tags, then the priority, then the status (omitted on done tasks).
    pub fn from_task(task: &Task, workflow: &crate::model::Workflow) -> Self {
        let marker = if task.done { DONE_PREFIX } else { OPEN_PREFIX };
        let mut lines = vec![format!("{marker}{}", task.title)];
        let mut section = |name: &str, body: &[String]| {
            lines.push(String::new());
            lines.push(format!("## {name}"));
            if !body.is_empty() {
                lines.push(String::new());
                lines.extend(body.iter().cloned());
            }
        };
        if let Some(desc) = task.description.as_deref().filter(|d| !d.is_empty()) {
            section(section::DESCRIPTION, &[desc.to_owned()]);
        }
        if let Some(project) = &task.project {
            section(section::PROJECT, &[project.display().to_string()]);
        }
        if let Some(due) = task.due {
            section(section::DUE, &[crate::clock::format_date(due)]);
        }
        if let Some(origin) = &task.origin {
            section(section::SOURCE, &[entry::format_origin(origin)]);
        }
        if let Some(at) = task.closed_at.filter(|_| task.done) {
            section(section::CLOSED, &[crate::clock::format_timestamp(at)]);
        }
        if !task.related.is_empty() {
            let related: Vec<String> = task.related.iter().map(entry::format_link).collect();
            section(section::RELATED, &related);
        }
        let mut tags: Vec<String> = task.tags.iter().map(crate::model::Tag::to_hash).collect();
        tags.push(task.priority.to_hash().to_owned());
        if let Some(status) = task.status.as_ref().filter(|_| !task.done) {
            tags.push(status.to_hash());
        }
        section(section::TAGS, &[tags.join(" ")]);
        let progress: Vec<String> = task.progress.iter().map(entry::format_progress).collect();
        section(section::PROGRESS, &progress);

        let mut text = lines.join("\n");
        text.push('\n');
        let mut doc = Self::parse(&text).expect("a rendered title line always parses");
        for mr in &task.merge_requests {
            ops::append_merge_request(&mut doc, mr);
        }
        for worktree in &task.worktrees {
            ops::append_worktree(&mut doc, worktree);
        }
        for session in &task.sessions {
            ops::append_session(&mut doc, session);
        }
        let _ = workflow;
        doc
    }
}
