//! Turning a whole-task write into edit operations on the stored document.
//!
//! [`Store::update`](tasq_core::store::Store::update) hands the store the
//! task as the caller wants it. The document on disk is edited with the
//! operations of [`tasq_core::format::ops`], each the exact rewrite the
//! original script made, so the resulting bytes are the ones the script
//! would have produced. Afterwards the document is projected back onto a
//! task and compared with the request: any field the operations could not
//! bring in line (a new title, a removed tag, a dropped progress entry, ...)
//! is reported and nothing is written.

use tasq_core::format::{self, Document, ops};
use tasq_core::model::{Task, Workflow};

/// Edits `doc` so that it describes `wanted`. On success the projection of
/// `doc` equals `wanted`. On failure the document may have been partially
/// edited (callers discard it) and the names of the fields that still differ
/// are returned.
pub fn apply(
    doc: &mut Document,
    wanted: &Task,
    workflow: &Workflow,
) -> Result<(), Vec<&'static str>> {
    let project = |doc: &Document| format::project(doc, wanted.id.clone(), workflow);
    let current = project(doc);

    if wanted.title != current.title {
        ops::set_title(doc, &wanted.title);
    }
    if wanted.done != current.done {
        if wanted.done {
            ops::set_done(doc, workflow);
        } else {
            ops::set_open(doc);
        }
    }
    if wanted.priority != current.priority {
        ops::set_priority(doc, wanted.priority, workflow);
    }
    // Priority before status so a file that gets both ends `#A #ready`, the
    // order `cmd_create` wrote. A done task carries no status (the
    // projection drops it), so there is nothing to set on one.
    let after_done = project(doc);
    if !wanted.done && wanted.status != after_done.status {
        match &wanted.status {
            Some(status) => ops::set_status(doc, status, workflow),
            None => ops::strip_status_tag(doc, workflow),
        }
    }
    if wanted.description != current.description {
        match wanted.description.as_deref() {
            Some(text) => ops::set_description(doc, text),
            None => {
                ops::clear_description(doc);
            }
        }
    }
    // Against the task as read: reopening drops `## Closed` even though the
    // projection after `set_open` already reads `None`. A stale section on
    // an open file reads as `None` too, so it is left alone.
    if wanted.closed_at != current.closed_at {
        match wanted.closed_at {
            Some(at) => ops::set_closed(doc, at),
            None => {
                ops::clear_closed(doc);
            }
        }
    }
    if wanted.due != current.due {
        match wanted.due {
            Some(due) => ops::set_due(doc, due),
            None => {
                ops::clear_due(doc);
            }
        }
    }
    if wanted.project != current.project {
        match &wanted.project {
            Some(project_dir) => ops::set_project(doc, project_dir),
            None => {
                ops::clear_project(doc);
            }
        }
    }
    if wanted.tags != current.tags {
        ops::set_tags(doc, &wanted.tags, workflow);
    }
    for link in wanted
        .related
        .iter()
        .filter(|l| !current.related.iter().any(|c| c.url == l.url))
    {
        ops::append_related(doc, link);
    }
    for link in wanted
        .merge_requests
        .iter()
        .filter(|l| !current.merge_requests.iter().any(|c| c.url == l.url))
    {
        ops::append_merge_request(doc, link);
    }
    for worktree in wanted
        .worktrees
        .iter()
        .filter(|w| !current.worktrees.iter().any(|c| c.path == w.path))
    {
        ops::append_worktree(doc, worktree);
    }
    for session in wanted
        .sessions
        .iter()
        .filter(|s| !current.sessions.iter().any(|c| c.id == s.id))
    {
        ops::append_session(doc, session);
    }
    if wanted.progress.starts_with(&current.progress) {
        for entry in &wanted.progress[current.progress.len()..] {
            ops::append_progress(doc, entry);
        }
    }

    let result = project(doc);
    let differing = differing_fields(&result, wanted);
    if differing.is_empty() {
        Ok(())
    } else {
        Err(differing)
    }
}

/// Names of the fields in which `a` and `b` differ, in declaration order.
pub fn differing_fields(a: &Task, b: &Task) -> Vec<&'static str> {
    let checks: [(&'static str, bool); 16] = [
        ("id", a.id != b.id),
        ("title", a.title != b.title),
        ("done", a.done != b.done),
        ("status", a.status != b.status),
        ("priority", a.priority != b.priority),
        ("due", a.due != b.due),
        ("description", a.description != b.description),
        ("project", a.project != b.project),
        ("tags", a.tags != b.tags),
        ("related", a.related != b.related),
        ("merge_requests", a.merge_requests != b.merge_requests),
        ("worktrees", a.worktrees != b.worktrees),
        ("sessions", a.sessions != b.sessions),
        ("progress", a.progress != b.progress),
        ("origin", a.origin != b.origin),
        ("closed_at", a.closed_at != b.closed_at),
    ];
    checks
        .into_iter()
        .filter_map(|(name, differs)| differs.then_some(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tasq_core::clock::{Clock, FixedClock};
    use tasq_core::model::{Link, Priority, ProgressEntry, Session, Status, Tag, TaskId, Worktree};

    const TEXT: &str = "# [ ] Title\n\n## Tags\n\n#gitlab #B #ready\n\n## Progress\n\n- 2026-10-01 09:00: created\n";

    fn parse(text: &str) -> (Document, Task) {
        let p = format::parse(text, TaskId::from(1), &Workflow::default()).unwrap();
        (p.document, p.task)
    }

    fn apply_to(text: &str, edit: impl FnOnce(&mut Task)) -> Result<String, Vec<&'static str>> {
        let (mut doc, mut task) = parse(text);
        edit(&mut task);
        apply(&mut doc, &task, &Workflow::default())?;
        Ok(format::render(&doc))
    }

    #[test]
    fn unchanged_task_leaves_the_document_alone() {
        assert_eq!(apply_to(TEXT, |_| {}).unwrap(), TEXT);
    }

    #[test]
    fn status_and_priority_rewrite_the_tags_line() {
        let out = apply_to(TEXT, |t| {
            t.set_status(Status::BLOCKED);
            t.set_priority(Priority::A);
        })
        .unwrap();
        assert_eq!(
            out,
            "# [ ] Title\n\n## Tags\n\n#gitlab #A #blocked\n\n## Progress\n\n- 2026-10-01 09:00: created\n"
        );
    }

    #[test]
    fn clearing_the_status_strips_the_tag() {
        let out = apply_to(TEXT, Task::clear_status).unwrap();
        assert!(out.contains("\n#gitlab #B\n"), "{out}");
    }

    #[test]
    fn marking_done_flips_the_title_and_strips_the_status() {
        let out = apply_to(TEXT, Task::mark_done).unwrap();
        assert_eq!(
            out,
            "# [x] Title\n\n## Tags\n\n#gitlab #B\n\n## Progress\n\n- 2026-10-01 09:00: created\n"
        );
    }

    #[test]
    fn done_with_a_status_cannot_be_expressed() {
        // A done task has no status in the model (the projection drops the
        // tag), so asking for one is reported rather than written.
        assert_eq!(apply_to(TEXT, |t| t.done = true), Err(vec!["status"]));
    }

    #[test]
    fn reopening_a_done_task() {
        let done = "# [x] Title\n\n## Tags\n\n#gitlab #B\n";
        let out = apply_to(done, |t| {
            t.done = false;
            t.set_status(Status::READY);
        })
        .unwrap();
        assert_eq!(out, "# [ ] Title\n\n## Tags\n\n#gitlab #B #ready\n");
        let out = apply_to(done, |t| t.done = false).unwrap();
        assert_eq!(out, "# [ ] Title\n\n## Tags\n\n#gitlab #B\n");
    }

    #[test]
    fn closing_writes_closed_and_reopening_removes_it() {
        let clock = FixedClock::at("2026-10-07 14:32");
        let out = apply_to(TEXT, |t| t.close(&clock)).unwrap();
        assert_eq!(
            out,
            "# [x] Title\n\n## Closed\n\n2026-10-07 14:32\n\n## Tags\n\n#gitlab #B\n\n## Progress\n\n- 2026-10-01 09:00: created\n"
        );
        let reopened = apply_to(&out, |t| {
            t.done = false;
            t.closed_at = None;
            t.set_status(Status::READY);
        })
        .unwrap();
        assert_eq!(reopened, TEXT);
        // A stale section on an open file reads as `None`, so it is left.
        let stale = "# [ ] T\n\n## Closed\n\n2026-10-07 14:32\n\n## Tags\n\n#B\n";
        assert_eq!(apply_to(stale, |_| {}).unwrap(), stale);
        // An open task cannot carry a closing time.
        assert_eq!(
            apply_to(TEXT, |t| t.closed_at = Some(clock.now())),
            Err(vec!["closed_at"])
        );
    }

    #[test]
    fn appends_progress_lists_and_project() {
        let at = FixedClock::at("2026-10-04 10:15").0;
        let out = apply_to(TEXT, |t| {
            t.progress.push(ProgressEntry::new(at, "more"));
            t.progress.push(ProgressEntry::new(at, "and more"));
            t.add_worktree(Worktree::on_branch("/w", "b"));
            t.add_session(Session {
                at,
                id: "s1".into(),
                launcher: None,
                description: Some("d".into()),
            });
            t.add_merge_request(Link::labelled("https://mr/1", "MR"));
            t.add_related(Link::new("https://rel"));
            t.project = Some(PathBuf::from("/proj"));
        })
        .unwrap();
        let (_, back) = parse(&out);
        assert_eq!(back.progress.len(), 3);
        assert_eq!(back.progress[2].note, "and more");
        assert_eq!(back.worktrees, vec![Worktree::on_branch("/w", "b")]);
        assert_eq!(back.sessions[0].id, "s1");
        assert_eq!(
            back.merge_requests,
            vec![Link::labelled("https://mr/1", "MR")]
        );
        assert_eq!(back.related, vec![Link::new("https://rel")]);
        assert_eq!(back.project, Some(PathBuf::from("/proj")));
        assert!(
            out.starts_with("# [ ] Title\n\n## Project\n\n/proj\n\n## Tags\n"),
            "{out}"
        );
        assert!(out.contains("## Related\n\n- https://rel\n\n### Merge requests\n\n- [MR](https://mr/1)\n\n## Worktrees\n"), "{out}");
    }

    #[test]
    fn replacing_the_project_rewrites_the_section() {
        let text = "# [ ] T\n\n## Project\n\n/old\n\n## Tags\n\n#B\n";
        let out = apply_to(text, |t| t.project = Some(PathBuf::from("/new"))).unwrap();
        assert_eq!(out, "# [ ] T\n\n## Project\n\n/new\n\n## Tags\n\n#B\n");
    }

    #[test]
    fn changes_without_an_operation_are_reported() {
        assert_eq!(
            apply_to(TEXT, |t| t.progress.clear()),
            Err(vec!["progress"])
        );
        assert_eq!(
            apply_to(TEXT, |t| t.progress[0].note = "edited".into()),
            Err(vec!["progress"])
        );
        assert_eq!(
            apply_to(TEXT, |t| t.description = Some("\nleading blank\n".into())),
            Err(vec!["description"]),
            "the projection trims blank lines, so they cannot round-trip"
        );
        assert_eq!(
            apply_to(TEXT, |t| t.id = TaskId::from(9)),
            Ok(TEXT.to_owned()),
            "id is not in the file"
        );
    }

    #[test]
    fn the_form_fields_rewrite_their_sections() {
        let today = FixedClock::at("2026-10-04 10:15").today();
        let out = apply_to(TEXT, |t| {
            t.title = "Other".into();
            t.due = Some(today);
            t.tags = vec![Tag::new("new").unwrap()];
        })
        .unwrap();
        assert_eq!(
            out,
            "# [ ] Other\n\n## Due\n\n2026-10-04\n\n## Tags\n\n#new #B #ready\n\n## Progress\n\n- 2026-10-01 09:00: created\n"
        );
        assert_eq!(
            apply_to(TEXT, |t| t.tags.clear()).unwrap(),
            "# [ ] Title\n\n## Tags\n\n#B #ready\n\n## Progress\n\n- 2026-10-01 09:00: created\n",
            "removing a topic tag"
        );
        let out = apply_to(TEXT, |t| t.description = Some("Why.\n\nHow.".into())).unwrap();
        assert_eq!(
            out,
            "# [ ] Title\n\n## Description\n\nWhy.\n\nHow.\n\n## Tags\n\n#gitlab #B #ready\n\n## Progress\n\n- 2026-10-01 09:00: created\n"
        );
        let described = "# [ ] T\n\n## Description\n\nOld.\n\n## Tags\n\n#B\n";
        assert_eq!(
            apply_to(described, |t| t.description = None).unwrap(),
            "# [ ] T\n\n## Tags\n\n#B\n"
        );
        assert_eq!(
            apply_to(described, |t| t.description = Some(String::new())),
            Err(vec!["description"]),
            "an empty description reads back as none, so it is reported; callers pass None"
        );
        let with_both = "# [ ] T\n\n## Project\n\n/old\n\n## Due\n\n2026-01-01\n\n## Tags\n\n#B\n";
        assert_eq!(
            apply_to(with_both, |t| {
                t.project = None;
                t.due = None;
            })
            .unwrap(),
            "# [ ] T\n\n## Tags\n\n#B\n"
        );
    }

    #[test]
    fn differing_fields_names_every_field() {
        let a = Task::new(TaskId::from(1), "T");
        let mut b = a.clone();
        assert_eq!(differing_fields(&a, &b), Vec::<&str>::new());
        b.id = TaskId::from(2);
        b.title = "U".into();
        b.done = true;
        b.set_status(Status::READY);
        b.set_priority(Priority::A);
        b.due = Some(FixedClock::at("2026-10-04 10:15").today());
        b.description = Some("d".into());
        b.project = Some("/p".into());
        b.add_tag(Tag::new("t").unwrap());
        b.add_related(Link::new("u"));
        b.add_merge_request(Link::new("m"));
        b.add_worktree(Worktree::new("/w"));
        b.add_session(Session {
            at: FixedClock::at("2026-10-04 10:15").0,
            id: "s".into(),
            launcher: None,
            description: None,
        });
        b.log("n", &FixedClock::at("2026-10-04 10:15"));
        b.origin = Some(tasq_core::model::Origin {
            source: "s".into(),
            external_id: "e".into(),
            url: None,
        });
        b.closed_at = Some(FixedClock::at("2026-10-04 10:15").0);
        assert_eq!(
            differing_fields(&a, &b),
            vec![
                "id",
                "title",
                "done",
                "status",
                "priority",
                "due",
                "description",
                "project",
                "tags",
                "related",
                "merge_requests",
                "worktrees",
                "sessions",
                "progress",
                "origin",
                "closed_at",
            ]
        );
    }
}
