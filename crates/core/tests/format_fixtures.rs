//! Fixture tests for the markdown format: every file under `fixtures/` parses
//! to the expected task, renders back byte for byte and re-parses to the same
//! task. The fixtures were produced by the original script's own functions.

use std::path::PathBuf;

use chrono::NaiveDate;
use tasq_core::clock::{FixedClock, When};
use tasq_core::format::{self, Document, FormatError, Newline, Parsed};
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Session, Status, Tag, TaskId, Workflow, Worktree,
};

const FIXTURES: &[&str] = &[
    "minimal",
    "full",
    "done",
    "no-tags-section",
    "no-status-tag",
    "crlf",
    "extra-sections",
    "source",
];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn parse(text: &str) -> Parsed {
    format::parse(text, TaskId::from(7), &Workflow::default()).expect("fixture parses")
}

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

fn tag(s: &str) -> Tag {
    Tag::new(s).unwrap()
}

#[test]
fn every_fixture_renders_back_byte_for_byte() {
    for name in FIXTURES {
        let text = fixture(name);
        let parsed = parse(&text);
        assert_eq!(format::render(&parsed.document), text, "{name}");
        assert_eq!(parsed.document.to_string(), text, "{name} via Display");
    }
}

#[test]
fn every_fixture_reparses_to_the_same_result() {
    for name in FIXTURES {
        let text = fixture(name);
        let once = parse(&text);
        let twice = parse(&format::render(&once.document));
        assert_eq!(once, twice, "{name}");
        assert_eq!(
            format::project(&once.document, TaskId::from(7), &Workflow::default()),
            once.task,
            "{name} via project"
        );
    }
}

#[test]
fn minimal() {
    let Parsed { document, task } = parse(&fixture("minimal"));
    assert_eq!(task.id, TaskId::from(7));
    assert_eq!(task.title, "Minimal task");
    assert!(!task.done);
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.priority, Priority::B);
    assert_eq!(task.tags, Vec::new());
    assert_eq!(
        task.progress,
        vec![ProgressEntry::new(
            at("2026-10-04 10:15"),
            "created via tasks create"
        )]
    );
    assert_eq!(
        (task.due, task.description, task.project),
        (None, None, None)
    );
    assert_eq!(document.newline(), Newline::Lf);
    assert!(document.ends_with_newline());
    assert_eq!(document.line_count(), 9);
    let names: Vec<&str> = document.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Tags", "Progress"]);
}

#[test]
fn full() {
    let task = parse(&fixture("full")).task;
    assert_eq!(task.title, "Full task");
    assert!(!task.done);
    assert_eq!(task.status, Some(Status::IN_PROGRESS));
    assert_eq!(task.priority, Priority::A);
    assert_eq!(task.tags, vec![tag("gitlab"), tag("review-request")]);
    assert_eq!(
        task.description.as_deref(),
        Some("Rewrite the tasks script in Rust.\nSecond paragraph of the description.")
    );
    assert_eq!(task.project, Some(PathBuf::from("/home/pau/code/tasks")));
    assert_eq!(task.due, NaiveDate::from_ymd_opt(2026, 10, 10));
    assert_eq!(
        task.related,
        vec![
            Link::new("https://gitlab.example.invalid/group/project/-/issues/42"),
            Link::new("https://example.invalid/docs/spec"),
        ]
    );
    assert_eq!(
        task.merge_requests,
        vec![
            Link::labelled(
                "https://gitlab.example.invalid/group/project/-/merge_requests/123",
                "Add parser"
            ),
            Link::labelled(
                "https://gitlab.example.invalid/group/project/-/merge_requests/124",
                "Add writer"
            ),
        ]
    );
    assert_eq!(
        task.worktrees,
        vec![
            Worktree::on_branch("/home/pau/code/tasks-wt/feature-a", "feature-a"),
            Worktree::new("/home/pau/code/tasks-wt/detached"),
        ]
    );
    assert_eq!(
        task.sessions,
        vec![
            Session {
                at: at("2026-10-04 10:20"),
                id: "abc-123".into(),
                launcher: None,
                description: Some("first session".into()),
            },
            Session {
                at: at("2026-10-04 12:00"),
                id: "def-456".into(),
                launcher: None,
                description: None,
            },
        ]
    );
    assert_eq!(
        task.progress,
        vec![
            ProgressEntry::dated(
                NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
                "legacy note without time"
            ),
            ProgressEntry::new(at("2026-10-04 10:15"), "created from the issue"),
            ProgressEntry::new(at("2026-10-04 11:40"), "parser done"),
        ]
    );
    assert_eq!(task.progress[0].at, When::Date(task.progress[0].at.date()));
    assert_eq!(task.latest_progress().unwrap().note, "parser done");
    assert_eq!(task.origin, None);
}

#[test]
fn full_document_view() {
    let document = parse(&fixture("full")).document;
    // The structured view keeps the unknown section and the subsection.
    let names: Vec<(u8, &str)> = document
        .sections()
        .iter()
        .map(|s| (s.level, s.name))
        .collect();
    assert_eq!(
        names,
        [
            (2, "Description"),
            (2, "Project"),
            (2, "Due"),
            (2, "Related"),
            (2, "Tags"),
            (2, "Worktrees"),
            (2, "Sessions"),
            (2, "Progress"),
            (2, "Notes"),
        ]
    );
    let related = document.section("Related").unwrap();
    assert_eq!(related.heading, 15);
    let subs = related.subsections();
    assert_eq!(subs.len(), 1);
    assert_eq!(
        (subs[0].level, subs[0].name, subs[0].heading),
        (3, "Merge requests", 20)
    );
    assert_eq!(subs[0].body.len(), 4);
    assert_eq!(
        related.own_body(),
        [
            "",
            "- https://gitlab.example.invalid/group/project/-/issues/42",
            "- https://example.invalid/docs/spec",
            "",
        ]
    );
    let notes = document.section("Notes").unwrap();
    assert_eq!(
        notes.trimmed_body(),
        ["Something the script never wrote.", "", "- a list item"]
    );
    assert_eq!(notes.body.len(), 4);
}

#[test]
fn done() {
    let Parsed { document, task } = parse(&fixture("done"));
    assert!(task.done);
    assert!(document.is_done());
    assert_eq!(document.title(), "Done task");
    assert_eq!(document.title_line(), "# [x] Done task");
    assert_eq!(task.status, None);
    assert_eq!(task.priority, Priority::C);
    assert_eq!(task.tags, vec![tag("gitlab")]);
    assert_eq!(task.progress.len(), 2);
    assert_eq!(task.progress[1].note, "shipped");
}

#[test]
fn no_tags_section() {
    let Parsed { document, task } = parse(&fixture("no-tags-section"));
    assert_eq!(task.status, None);
    assert_eq!(task.priority, Priority::B);
    assert_eq!(task.tags, Vec::new());
    assert_eq!(
        task.description.as_deref(),
        Some("Written with nb todo add.")
    );
    assert_eq!(task.progress, Vec::new());
    assert_eq!(document.section("Tags"), None);
}

#[test]
fn no_status_tag() {
    let task = parse(&fixture("no-status-tag")).task;
    assert!(!task.done);
    assert_eq!(task.status, None);
    assert_eq!(task.priority, Priority::B);
    assert_eq!(task.tags, vec![tag("docs")]);
}

#[test]
fn crlf() {
    let text = fixture("crlf");
    assert!(text.contains("\r\n"));
    let Parsed { document, task } = parse(&text);
    assert_eq!(document.newline(), Newline::CrLf);
    assert_eq!(task.title, "Minimal task");
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.progress.len(), 1);
    // Same task as the LF file, apart from the endings.
    let lf = parse(&fixture("minimal")).task;
    assert_eq!(task, lf);
    assert_eq!(
        document.lines().collect::<Vec<_>>(),
        parse(&fixture("minimal"))
            .document
            .lines()
            .collect::<Vec<_>>()
    );
}

#[test]
fn extra_sections() {
    let Parsed { document, task } = parse(&fixture("extra-sections"));
    assert_eq!(task.status, Some(Status::WAITING));
    assert_eq!(task.due, None, "non-ISO due is reported as none");
    assert_eq!(
        document.section("Due").unwrap().trimmed_body(),
        ["next friday"]
    );
    assert_eq!(task.related, Vec::new(), "## Links is not ## Related");
    let checklist = document.section("Checklist").unwrap();
    assert_eq!(checklist.subsections().len(), 1);
    assert_eq!(checklist.subsections()[0].name, "Sub-checklist");
    assert_eq!(checklist.own_body(), ["", "- [ ] one", "- [x] two", ""]);
}

#[test]
fn source() {
    let task = parse(&fixture("source")).task;
    assert_eq!(
        task.origin,
        Some(Origin {
            source: "gitlab".into(),
            external_id: "https://gitlab.example.invalid/group/project/-/merge_requests/123".into(),
            url: Some("https://gitlab.example.invalid/group/project/-/merge_requests/123".into()),
        })
    );
    assert_eq!(task.tags, vec![tag("review-request")]);
}

#[test]
fn non_tasks_are_rejected() {
    for text in [
        "",
        "\n",
        "# Title\n",
        "[ ] Title\n",
        "# [ ]",
        "# [ ] \n",
        "# [X] Title\n",
        " # [ ] T\n",
        "#[ ] T",
    ] {
        let err = format::parse(text, TaskId::from(1), &Workflow::default()).unwrap_err();
        let FormatError::NotATask { first_line } = &err;
        assert_eq!(first_line, text.lines().next().unwrap_or(""), "{text:?}");
        assert!(err.to_string().starts_with("not a task: "));
        assert_eq!(Document::parse(text), Err(err));
    }
}

#[test]
fn title_only_files_parse() {
    for (text, trailing) in [
        ("# [ ] T", false),
        ("# [ ] T\n", true),
        ("# [x] T\r\n", true),
        ("# [ ] T\r\n\r\n", true),
    ] {
        let doc = Document::parse(text).unwrap();
        assert_eq!(doc.ends_with_newline(), trailing, "{text:?}");
        assert_eq!(doc.render(), text);
        assert_eq!(doc.title(), "T");
        assert_eq!(doc.sections(), Vec::new());
    }
    assert_eq!(Document::parse("# [ ] T").unwrap().newline(), Newline::Lf);
    assert_eq!(
        Document::parse("# [ ] T\r\n").unwrap().newline(),
        Newline::CrLf
    );
    assert_eq!(
        Document::parse("# [ ] T\r\nx").unwrap().newline(),
        Newline::CrLf
    );
    assert_eq!(
        Document::parse("# [ ] T\nx\r\n").unwrap().newline(),
        Newline::Lf
    );
}

#[test]
fn mixed_line_endings_survive() {
    let text = "# [ ] T\r\n\n## Tags\r\n\r\n#B\n";
    let doc = Document::parse(text).unwrap();
    assert_eq!(doc.render(), text);
    assert_eq!(
        doc.lines().collect::<Vec<_>>(),
        ["# [ ] T", "", "## Tags", "", "#B"]
    );
}
