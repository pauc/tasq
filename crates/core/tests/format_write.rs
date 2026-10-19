//! Writing the task file format (ADR 0026): a task becomes a canonical
//! document, and each frontmatter key is set, added in canonical position or
//! cleared on an existing document without touching any other line.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests read fixtures from disk; the no-I/O rule is for the library"
)]

use std::path::PathBuf;

use chrono::NaiveDate;
use tasq_core::clock::FixedClock;
use tasq_core::format::{self, Document, Parsed, key, ops};
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Status, Tag, Task, TaskId, Workflow,
};

const FIXTURES: &[&str] = &[
    "minimal",
    "full",
    "done",
    "ranked",
    "source",
    "crlf",
    "unknown-keys",
    "blank-lines",
    "extra-sections",
];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn parse(text: &str) -> Parsed {
    format::parse(text, None, &Workflow::default()).expect("text parses")
}

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

fn tag(s: &str) -> Tag {
    Tag::new(s).unwrap()
}

fn render_task(task: &Task) -> String {
    format::render(&Document::from_task(task))
}

// ---- fresh documents ------------------------------------------------------

#[test]
fn a_blank_task_renders_the_minimal_block() {
    let task = Task::new(TaskId::from(47), "Minimal task");
    assert_eq!(
        render_task(&task),
        "---\nid: 47\npriority: B\n---\n# Minimal task\n\n## Progress\n"
    );
}

#[test]
fn a_full_task_renders_every_key_in_canonical_order() {
    let mut task = Task::new(TaskId::from(12), "Full task");
    task.status = Some(Status::IN_PROGRESS);
    task.priority = Priority::A;
    task.rank = Some(1024);
    task.due = NaiveDate::from_ymd_opt(2026, 10, 10);
    task.project = Some(PathBuf::from("/home/pau/code/tasks"));
    task.origin = Some(Origin {
        source: "gitlab".into(),
        external_id: "!123".into(),
        url: Some("https://gitlab.example.invalid/group/project/-/merge_requests/123".into()),
    });
    task.tags = vec![tag("gitlab"), tag("review-request")];
    task.description =
        Some("Rewrite the tasks script in Rust.\nSecond paragraph of the description.".into());
    task.related = vec![
        Link::new("https://gitlab.example.invalid/group/project/-/issues/42"),
        Link::new("https://example.invalid/docs/spec"),
    ];
    task.merge_requests = vec![Link::labelled(
        "https://gitlab.example.invalid/group/project/-/merge_requests/123",
        "Add parser",
    )];
    task.worktrees = vec![tasq_core::model::Worktree::on_branch(
        "/home/pau/code/tasks-wt/feature-a",
        "feature-a",
    )];
    task.sessions = vec![tasq_core::model::Session {
        at: at("2026-10-04 10:20"),
        id: "abc-123".into(),
        launcher: None,
        description: Some("first session".into()),
    }];
    task.progress = vec![ProgressEntry::new(
        at("2026-10-04 10:15"),
        "created from the issue",
    )];
    assert_eq!(
        render_task(&task),
        concat!(
            "---\n",
            "id: 12\n",
            "status: in-progress\n",
            "priority: A\n",
            "rank: 1024\n",
            "due: 2026-10-10\n",
            "project: /home/pau/code/tasks\n",
            "source: gitlab\n",
            "source_id: !123\n",
            "source_url: https://gitlab.example.invalid/group/project/-/merge_requests/123\n",
            "tags: [gitlab, review-request]\n",
            "---\n",
            "# Full task\n",
            "\n",
            "## Description\n",
            "\n",
            "Rewrite the tasks script in Rust.\n",
            "Second paragraph of the description.\n",
            "\n",
            "## Related\n",
            "\n",
            "- https://gitlab.example.invalid/group/project/-/issues/42\n",
            "- https://example.invalid/docs/spec\n",
            "\n",
            "### Merge requests\n",
            "\n",
            "- [Add parser](https://gitlab.example.invalid/group/project/-/merge_requests/123)\n",
            "\n",
            "## Worktrees\n",
            "\n",
            "- /home/pau/code/tasks-wt/feature-a (`feature-a`)\n",
            "\n",
            "## Sessions\n",
            "\n",
            "- 2026-10-04 10:20: `abc-123` \u{2014} first session\n",
            "\n",
            "## Progress\n",
            "\n",
            "- 2026-10-04 10:15: created from the issue\n",
        )
    );
}

#[test]
fn a_done_task_writes_status_done_and_closed() {
    let mut task = Task::new(TaskId::from(3), "Done task");
    task.priority = Priority::C;
    task.tags = vec![tag("gitlab")];
    task.close(&FixedClock::at("2026-10-05 09:00"));
    task.progress = vec![
        ProgressEntry::new(at("2026-10-04 10:15"), "created via tasq create"),
        ProgressEntry::new(at("2026-10-05 09:00"), "shipped"),
    ];
    assert_eq!(render_task(&task), fixture("done"));
    // A status on a done task is not written (the model drops it); a
    // `closed_at` on an open task is not written either.
    task.status = Some(Status::READY);
    assert_eq!(render_task(&task), fixture("done"));
    task.done = false;
    task.status = None;
    assert_eq!(
        render_task(&task),
        "---\nid: 3\npriority: C\ntags: [gitlab]\n---\n# Done task\n\n## Progress\n\n- 2026-10-04 10:15: created via tasq create\n- 2026-10-05 09:00: shipped\n"
    );
}

#[test]
fn source_url_is_written_only_when_there_is_one() {
    let mut task = Task::new(TaskId::from(21), "Synced from GitLab");
    task.status = Some(Status::READY);
    task.tags = vec![tag("review-request")];
    task.origin = Some(Origin {
        source: "gitlab".into(),
        external_id: "https://gitlab.example.invalid/group/project/-/merge_requests/123".into(),
        url: Some("https://gitlab.example.invalid/group/project/-/merge_requests/123".into()),
    });
    task.progress = vec![ProgressEntry::new(
        at("2026-10-04 10:15"),
        "created via tasq sync",
    )];
    // A url equal to the id is still written.
    assert_eq!(
        render_task(&task),
        fixture("source").replacen("status: ready\n", "status: ready\npriority: B\n", 1)
    );
    task.origin.as_mut().unwrap().url = None;
    let text = render_task(&task);
    assert!(!text.contains("source_url"), "{text}");
    assert_eq!(parse(&text).task.origin, task.origin);
}

#[test]
fn an_empty_description_is_not_written() {
    let mut task = Task::new(TaskId::from(1), "T");
    task.description = Some(String::new());
    assert_eq!(
        render_task(&task),
        "---\nid: 1\npriority: B\n---\n# T\n\n## Progress\n"
    );
}

#[test]
fn every_fixture_task_round_trips_through_a_fresh_document() {
    let wf = Workflow::default();
    for name in FIXTURES {
        let expected = parse(&fixture(name)).task;
        let text = render_task(&expected);
        let parsed = format::parse(&text, None, &wf).unwrap();
        assert_eq!(parsed.task, expected, "{name}:\n{text}");
        assert_eq!(format::render(&parsed.document), text, "{name}");
        assert!(
            text.starts_with(&format!("---\nid: {}\n", expected.id)),
            "{name}"
        );
        assert!(!text.contains("## Tags"), "{name}");
    }
}

#[test]
fn a_fresh_document_knows_its_created_at_only_through_parse() {
    let mut task = Task::new(TaskId::from(1), "T");
    task.created_at = Some(at("2026-10-04 10:15"));
    let text = render_task(&task);
    assert!(!text.contains("created"), "{text}");
    let parsed = format::parse(&text, task.created_at, &Workflow::default()).unwrap();
    assert_eq!(parsed.task, task);
}

// ---- the key primitives ----------------------------------------------------

/// `text` with the block line `from` replaced by `to`; the rest untouched.
fn with_line(text: &str, from: &str, to: &str) -> String {
    let (before, after) = text.split_once(from).unwrap();
    assert!(!after.contains(from), "{from:?} occurs twice");
    format!("{before}{to}{after}")
}

#[test]
fn set_key_replaces_the_line_of_a_present_key() {
    let text = fixture("full");
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, key::STATUS, "blocked");
    assert_eq!(
        format::render(&doc),
        with_line(&text, "status: in-progress\n", "status: blocked\n")
    );
    // The id is written once and never changed.
    ops::set_key(&mut doc, key::ID, "13");
    assert_eq!(doc.id(), TaskId::from(12));
    assert!(format::render(&doc).starts_with("---\nid: 12\nstatus: blocked\n"));
    ops::set_key(&mut doc, key::TAGS, "[x]");
    assert!(format::render(&doc).contains("source_url: https://gitlab.example.invalid/group/project/-/merge_requests/123\ntags: [x]\n---\n"));
}

#[test]
fn set_key_adds_a_missing_key_after_the_last_known_key_before_it() {
    let text = fixture("minimal"); // id, status
    let add = |k: &str, v: &str| {
        let mut doc = Document::parse(&text).unwrap();
        ops::set_key(&mut doc, k, v);
        format::render(&doc)
    };
    // Every key after `status` lands right after the status line.
    for (k, v) in [
        (key::PRIORITY, "A"),
        (key::RANK, "7"),
        (key::DUE, "2026-10-10"),
        (key::CLOSED, "2026-10-10 10:00"),
        (key::PROJECT, "/p"),
        (key::SOURCE, "gitlab"),
        (key::SOURCE_ID, "!1"),
        (key::SOURCE_URL, "https://gl.invalid/1"),
        (key::TAGS, "[a, b]"),
    ] {
        assert_eq!(
            add(k, v),
            with_line(
                &text,
                "status: ready\n",
                &format!("status: ready\n{k}: {v}\n")
            ),
            "{k}"
        );
    }
    // A key before `status` lands after `id`.
    let mut doc = Document::parse("---\nstatus: ready\nid: 2\n---\n# T\n").unwrap();
    ops::set_key(&mut doc, key::PRIORITY, "C");
    assert_eq!(
        format::render(&doc),
        "---\nstatus: ready\nid: 2\npriority: C\n---\n# T\n",
        "after the last known key that precedes it, wherever that is"
    );
    let mut doc = Document::parse("---\nid: 2\n---\n# T\n").unwrap();
    ops::set_key(&mut doc, key::STATUS, "ready");
    ops::set_key(&mut doc, key::DUE, "2026-01-01");
    ops::set_key(&mut doc, key::PRIORITY, "A");
    assert_eq!(
        format::render(&doc),
        "---\nid: 2\nstatus: ready\npriority: A\ndue: 2026-01-01\n---\n# T\n"
    );
}

#[test]
fn set_key_keeps_unknown_keys_and_blank_lines_in_place() {
    let text = fixture("blank-lines"); // "", id, "", status, "  ", priority
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, key::RANK, "5");
    assert_eq!(
        format::render(&doc),
        with_line(&text, "priority: B\n", "priority: B\nrank: 5\n")
    );
    ops::set_key(&mut doc, key::STATUS, "ready");
    assert_eq!(
        format::render(&doc),
        with_line(
            &with_line(&text, "priority: B\n", "priority: B\nrank: 5\n"),
            "status: blocked\n",
            "status: ready\n"
        )
    );
    let text = fixture("unknown-keys"); // owner, id, estimate, note, ..., status, status
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, key::PRIORITY, "A");
    assert_eq!(
        format::render(&doc),
        with_line(&text, "status: ready\n", "status: ready\npriority: A\n"),
        "after the last line of a known key, unknown lines between"
    );
}

#[test]
fn set_key_on_a_duplicated_key_keeps_one_line() {
    let text = fixture("unknown-keys"); // status: waiting, status: ready
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, key::STATUS, "blocked");
    assert_eq!(
        format::render(&doc),
        text.replace("status: waiting\nstatus: ready\n", "status: blocked\n")
    );
    assert_eq!(
        parse(&format::render(&doc)).task.status,
        Some(Status::BLOCKED)
    );
}

#[test]
fn set_key_of_an_unknown_key_goes_at_the_end_of_the_block() {
    let text = fixture("minimal");
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, "owner", "pau");
    assert_eq!(
        format::render(&doc),
        with_line(&text, "status: ready\n", "status: ready\nowner: pau\n")
    );
    ops::set_key(&mut doc, "owner", "someone else");
    assert_eq!(
        format::render(&doc),
        with_line(
            &text,
            "status: ready\n",
            "status: ready\nowner: someone else\n"
        )
    );
    assert_eq!(doc.frontmatter().get("owner"), Some("someone else"));
}

#[test]
fn set_key_uses_the_documents_line_ending() {
    let text = fixture("crlf");
    let mut doc = Document::parse(&text).unwrap();
    ops::set_key(&mut doc, key::PRIORITY, "A");
    assert_eq!(
        format::render(&doc),
        with_line(
            &text,
            "status: ready\r\n",
            "status: ready\r\npriority: A\r\n"
        )
    );
}

#[test]
fn clear_key_removes_every_line_of_the_key() {
    let text = fixture("unknown-keys");
    let mut doc = Document::parse(&text).unwrap();
    assert!(ops::clear_key(&mut doc, key::STATUS));
    assert_eq!(
        format::render(&doc),
        text.replace("status: waiting\nstatus: ready\n", "")
    );
    assert!(!ops::clear_key(&mut doc, key::STATUS));
    assert!(!ops::clear_key(&mut doc, key::PRIORITY));
    assert!(ops::clear_key(&mut doc, "owner"));
    assert_eq!(
        format::render(&doc),
        text.replace("status: waiting\nstatus: ready\n", "")
            .replace("owner: pau\n", "")
    );
    // Every known key of the full fixture, one at a time: only its line goes.
    let text = fixture("full");
    for k in key::ORDER.iter().filter(|k| **k != key::ID) {
        let mut doc = Document::parse(&text).unwrap();
        let Some(found) = text.find(&format!("\n{k}: ")) else {
            assert_eq!(*k, key::CLOSED, "the open fixture has every key but closed");
            assert!(!ops::clear_key(&mut doc, k));
            assert_eq!(format::render(&doc), text);
            continue;
        };
        assert!(ops::clear_key(&mut doc, k), "{k}");
        let line_start = found + 1;
        let line_end = line_start + text[line_start..].find('\n').unwrap() + 1;
        assert_eq!(
            format::render(&doc),
            format!("{}{}", &text[..line_start], &text[line_end..]),
            "{k}"
        );
    }
}

#[test]
fn clear_key_never_removes_the_id() {
    let text = fixture("minimal");
    let mut doc = Document::parse(&text).unwrap();
    assert!(!ops::clear_key(&mut doc, key::ID));
    assert_eq!(format::render(&doc), text);
}

#[test]
fn key_order_is_the_canonical_one() {
    assert_eq!(
        key::ORDER,
        [
            "id",
            "status",
            "priority",
            "rank",
            "due",
            "closed",
            "project",
            "source",
            "source_id",
            "source_url",
            "tags"
        ]
    );
}

// ---- the typed operations ---------------------------------------------------

/// The minimal fixture (`id: 47`, `status: ready`) after `op`.
fn minimal_after(op: impl FnOnce(&mut Document)) -> String {
    let mut doc = Document::parse(&fixture("minimal")).unwrap();
    op(&mut doc);
    format::render(&doc)
}

/// The full fixture after `op`.
fn full_after(op: impl FnOnce(&mut Document)) -> String {
    let mut doc = Document::parse(&fixture("full")).unwrap();
    op(&mut doc);
    format::render(&doc)
}

fn minimal_with(block_after_status: &str) -> String {
    with_line(
        &fixture("minimal"),
        "status: ready\n",
        &format!("status: ready\n{block_after_status}"),
    )
}

#[test]
fn set_status_and_set_priority_write_their_keys() {
    assert_eq!(
        minimal_after(|d| ops::set_status(d, &Status::BLOCKED)),
        fixture("minimal").replace("status: ready", "status: blocked")
    );
    assert_eq!(
        minimal_after(|d| ops::set_priority(d, Priority::A)),
        minimal_with("priority: A\n")
    );
    assert_eq!(
        full_after(|d| ops::set_priority(d, Priority::C)),
        fixture("full").replace("priority: A", "priority: C")
    );
    // A status on a done task replaces `done`; the model never asks for it
    // (the store's compare refuses it), but the op is a plain key write.
    let mut doc = Document::parse(&fixture("done")).unwrap();
    ops::set_status(&mut doc, &Status::READY);
    assert_eq!(
        format::render(&doc),
        fixture("done").replace("status: done", "status: ready")
    );
}

#[test]
fn clear_status_removes_the_status_key_whatever_it_says() {
    assert_eq!(
        minimal_after(|d| {
            assert!(ops::clear_status(d));
        }),
        fixture("minimal").replace("status: ready\n", "")
    );
    let text = fixture("done");
    let mut doc = Document::parse(&text).unwrap();
    assert!(ops::clear_status(&mut doc));
    assert_eq!(format::render(&doc), text.replace("status: done\n", ""));
    assert!(!parse(&format::render(&doc)).task.done);
    assert!(!ops::clear_status(&mut doc), "nothing left to remove");
}

#[test]
fn set_done_writes_status_done_and_set_open_clears_it() {
    let done = minimal_after(ops::set_done);
    assert_eq!(
        done,
        fixture("minimal").replace("status: ready", "status: done")
    );
    let parsed = parse(&done);
    assert!(parsed.task.done);
    assert_eq!(parsed.task.status, None);
    assert_eq!(
        parsed.task.title, "Minimal task",
        "the title line is untouched"
    );
    let mut doc = parsed.document;
    ops::set_open(&mut doc);
    assert_eq!(
        format::render(&doc),
        fixture("minimal").replace("status: ready\n", "")
    );
    let reopened = parse(&format::render(&doc)).task;
    assert!(!reopened.done);
    assert_eq!(reopened.status, None, "reopened without a status");
    // Done on a task without a status adds the key after `id`.
    let mut doc = Document::parse("---\nid: 2\npriority: B\n---\n# T\n").unwrap();
    ops::set_done(&mut doc);
    assert_eq!(
        format::render(&doc),
        "---\nid: 2\nstatus: done\npriority: B\n---\n# T\n"
    );
    // Open on an open task changes nothing.
    let text = fixture("full");
    let mut doc = Document::parse(&text).unwrap();
    ops::set_open(&mut doc);
    assert_eq!(format::render(&doc), text);
}

#[test]
fn rank_due_closed_and_project_are_keys() {
    assert_eq!(
        minimal_after(|d| ops::set_rank(d, 1024)),
        minimal_with("rank: 1024\n")
    );
    assert_eq!(
        minimal_after(|d| ops::set_rank(d, -512)),
        minimal_with("rank: -512\n")
    );
    assert_eq!(
        minimal_after(|d| ops::set_due(d, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap())),
        minimal_with("due: 2026-11-01\n")
    );
    assert_eq!(
        minimal_after(|d| ops::set_closed(d, at("2026-10-07 14:32"))),
        minimal_with("closed: 2026-10-07 14:32\n")
    );
    assert_eq!(
        minimal_after(|d| ops::set_project(d, std::path::Path::new("/home/pau/code/x"))),
        minimal_with("project: /home/pau/code/x\n")
    );
    // Replacing keeps the line where it was.
    let text = fixture("full");
    assert_eq!(
        full_after(|d| ops::set_rank(d, 2)),
        text.replace("rank: 1024", "rank: 2")
    );
    assert_eq!(
        full_after(|d| ops::set_due(d, NaiveDate::from_ymd_opt(2027, 1, 2).unwrap())),
        text.replace("due: 2026-10-10", "due: 2027-01-02")
    );
    assert_eq!(
        full_after(|d| ops::set_project(d, std::path::Path::new("/new"))),
        text.replace("project: /home/pau/code/tasks", "project: /new")
    );
    let closed = fixture("done");
    let mut doc = Document::parse(&closed).unwrap();
    ops::set_closed(&mut doc, at("2026-10-06 08:00"));
    assert_eq!(
        format::render(&doc),
        closed.replace("closed: 2026-10-05 09:00", "closed: 2026-10-06 08:00")
    );
}

#[test]
fn clearing_rank_due_closed_and_project_removes_their_lines() {
    let text = fixture("full");
    let mut doc = Document::parse(&text).unwrap();
    assert!(ops::clear_rank(&mut doc));
    assert!(ops::clear_due(&mut doc));
    assert!(ops::clear_project(&mut doc));
    assert!(!ops::clear_closed(&mut doc));
    assert_eq!(
        format::render(&doc),
        text.replace("rank: 1024\n", "")
            .replace("due: 2026-10-10\n", "")
            .replace("project: /home/pau/code/tasks\n", "")
    );
    assert!(!ops::clear_rank(&mut doc));
    assert!(!ops::clear_due(&mut doc));
    assert!(!ops::clear_project(&mut doc));
    let closed = fixture("done");
    let mut doc = Document::parse(&closed).unwrap();
    assert!(ops::clear_closed(&mut doc));
    assert_eq!(
        format::render(&doc),
        closed.replace("closed: 2026-10-05 09:00\n", "")
    );
}

#[test]
fn set_tags_writes_the_list_and_an_empty_list_clears_it() {
    assert_eq!(
        minimal_after(|d| ops::set_tags(d, &[tag("gitlab"), tag("review-request")])),
        minimal_with("tags: [gitlab, review-request]\n")
    );
    assert_eq!(
        minimal_after(|d| ops::set_tags(d, &[])),
        fixture("minimal"),
        "no tags and no key is a no-op"
    );
    let text = fixture("full");
    assert_eq!(
        full_after(|d| ops::set_tags(d, &[tag("x")])),
        text.replace("tags: [gitlab, review-request]", "tags: [x]")
    );
    assert_eq!(
        full_after(|d| ops::set_tags(d, &[])),
        text.replace("tags: [gitlab, review-request]\n", "")
    );
    let mut doc = Document::parse(&text).unwrap();
    ops::set_tags(&mut doc, &[tag("b"), tag("a")]);
    assert_eq!(
        parse(&format::render(&doc)).task.tags,
        vec![tag("b"), tag("a")],
        "the order is the caller's"
    );
}

#[test]
fn set_title_rewrites_the_title_line_without_a_marker() {
    assert_eq!(
        minimal_after(|d| ops::set_title(d, "Renamed")),
        fixture("minimal").replace("# Minimal task", "# Renamed")
    );
    let mut doc = Document::parse(&fixture("done")).unwrap();
    ops::set_title(&mut doc, "Still done");
    assert_eq!(
        format::render(&doc),
        fixture("done").replace("# Done task", "# Still done")
    );
    assert!(parse(&format::render(&doc)).task.done);
    // The blank line between the fence and the title stays.
    let text = fixture("full");
    assert!(text.contains("---\n\n# Full task\n"));
    assert_eq!(
        full_after(|d| ops::set_title(d, "Renamed")),
        text.replace("# Full task", "# Renamed")
    );
}

#[test]
fn set_origin_writes_the_three_source_keys_and_clear_origin_removes_them() {
    let origin = Origin {
        source: "gitlab".into(),
        external_id: "!7".into(),
        url: Some("https://gl.invalid/7".into()),
    };
    assert_eq!(
        minimal_after(|d| ops::set_origin(d, &origin)),
        minimal_with("source: gitlab\nsource_id: !7\nsource_url: https://gl.invalid/7\n")
    );
    let without_url = Origin {
        url: None,
        ..origin.clone()
    };
    assert_eq!(
        minimal_after(|d| ops::set_origin(d, &without_url)),
        minimal_with("source: gitlab\nsource_id: !7\n")
    );
    // Replacing: the lines stay where they are; a url that goes is removed.
    let text = fixture("full");
    assert_eq!(
        full_after(|d| ops::set_origin(d, &origin)),
        text.replace("source_id: !123", "source_id: !7").replace(
            "source_url: https://gitlab.example.invalid/group/project/-/merge_requests/123",
            "source_url: https://gl.invalid/7"
        )
    );
    assert_eq!(
        full_after(|d| ops::set_origin(d, &without_url)),
        text.replace("source_id: !123", "source_id: !7").replace(
            "source_url: https://gitlab.example.invalid/group/project/-/merge_requests/123\n",
            ""
        )
    );
    let mut doc = Document::parse(&text).unwrap();
    assert!(ops::clear_origin(&mut doc));
    assert_eq!(
        format::render(&doc),
        text.replace("source: gitlab\nsource_id: !123\nsource_url: https://gitlab.example.invalid/group/project/-/merge_requests/123\n", "")
    );
    assert!(!ops::clear_origin(&mut doc));
    assert_eq!(parse(&format::render(&doc)).task.origin, None);
    // A lone `source_url` or a lone `source` (no origin reads from either)
    // is cleared too.
    for line in ["source_url: https://x", "source: gitlab", "source_id: 1"] {
        let mut doc = Document::parse(&format!("---\nid: 1\n{line}\n---\n# T\n")).unwrap();
        assert!(ops::clear_origin(&mut doc), "{line}");
        assert_eq!(format::render(&doc), "---\nid: 1\n---\n# T\n", "{line}");
    }
}

#[test]
fn every_scalar_op_leaves_the_body_untouched() {
    // The body of the full fixture, with its unknown `## Notes` section and
    // every list, survives each scalar op; only the block changes.
    type Op = Box<dyn Fn(&mut Document)>;
    let text = fixture("full");
    let body = &text[text.find("\n# Full task").unwrap()..];
    let ops: Vec<(&str, Op)> = vec![
        (
            "set_status",
            Box::new(|d| ops::set_status(d, &Status::READY)),
        ),
        (
            "set_priority",
            Box::new(|d| ops::set_priority(d, Priority::B)),
        ),
        (
            "clear_status",
            Box::new(|d| {
                ops::clear_status(d);
            }),
        ),
        ("set_done", Box::new(ops::set_done)),
        ("set_rank", Box::new(|d| ops::set_rank(d, 1))),
        (
            "clear_rank",
            Box::new(|d| {
                ops::clear_rank(d);
            }),
        ),
        (
            "set_due",
            Box::new(|d| ops::set_due(d, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())),
        ),
        (
            "clear_due",
            Box::new(|d| {
                ops::clear_due(d);
            }),
        ),
        (
            "set_closed",
            Box::new(|d| ops::set_closed(d, at("2026-10-07 14:32"))),
        ),
        (
            "clear_closed",
            Box::new(|d| {
                ops::clear_closed(d);
            }),
        ),
        (
            "set_project",
            Box::new(|d| ops::set_project(d, std::path::Path::new("/p"))),
        ),
        (
            "clear_project",
            Box::new(|d| {
                ops::clear_project(d);
            }),
        ),
        ("set_tags", Box::new(|d| ops::set_tags(d, &[tag("t")]))),
        ("clear_tags", Box::new(|d| ops::set_tags(d, &[]))),
        (
            "clear_origin",
            Box::new(|d| {
                ops::clear_origin(d);
            }),
        ),
    ];
    for (name, op) in ops {
        let mut doc = Document::parse(&text).unwrap();
        op(&mut doc);
        let out = format::render(&doc);
        assert!(out.ends_with(body), "{name}:\n{out}");
        assert!(out.starts_with("---\nid: 12\n"), "{name}:\n{out}");
    }
}
