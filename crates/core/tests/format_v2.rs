//! Format v2 (ADR 0026): a frontmatter block of scalar fields, then the
//! title heading and the list sections. Every fixture under `fixtures/v2/`
//! parses, renders back byte for byte and re-parses to the same task.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests read fixtures from disk; the no-I/O rule is for the library"
)]

use std::path::PathBuf;

use chrono::NaiveDate;
use tasq_core::clock::FixedClock;
use tasq_core::format::{self, Document, FormatError, Newline, Parsed, key};
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Session, Status, Tag, TaskId, Workflow, Worktree,
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
    let path = format!("{}/tests/fixtures/v2/{name}.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn parse(text: &str) -> Parsed {
    format::parse_v2(text, None, &Workflow::default()).expect("fixture parses")
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
        assert_eq!(
            Document::parse(&text).unwrap(),
            parsed.document,
            "{name} via Document"
        );
    }
}

#[test]
fn every_fixture_reparses_to_the_same_result() {
    for name in FIXTURES {
        let text = fixture(name);
        let once = parse(&text);
        let twice = parse(&format::render(&once.document));
        assert_eq!(once, twice, "{name}");
        let id = once.task.id.clone();
        assert_eq!(
            format::project(&once.document, TaskId::from(999), &Workflow::default()),
            once.task,
            "{name} via project: the id comes from the file"
        );
        assert_eq!(
            format::parse(&text, TaskId::from(999), &Workflow::default()).unwrap(),
            once,
            "{name} via parse: the id comes from the file"
        );
        assert_eq!(once.document.id(), Some(id));
    }
}

#[test]
fn minimal() {
    let text = fixture("minimal");
    let Parsed { document, task } = parse(&text);
    assert_eq!(task.id, TaskId::from(47));
    assert_eq!(task.title, "Minimal task");
    assert!(!task.done);
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.priority, Priority::B);
    assert_eq!(task.tags, Vec::new());
    assert_eq!(task.created_at, None);
    assert_eq!(
        task.progress,
        vec![ProgressEntry::new(
            at("2026-10-04 10:15"),
            "created via tasq create"
        )]
    );
    assert_eq!(
        (
            task.due,
            task.description,
            task.project,
            task.rank,
            task.closed_at,
            task.origin
        ),
        (None, None, None, None, None, None)
    );
    assert_eq!(document.title_line(), "# Minimal task");
    assert_eq!(document.title(), "Minimal task");
    assert!(!document.is_done());
    assert_eq!(document.id(), Some(TaskId::from(47)));
    assert_eq!(document.newline(), Newline::Lf);
    assert!(document.ends_with_newline());
    // The body lines only: the block is not among them.
    assert_eq!(document.line_count(), 5);
    assert_eq!(
        document.lines().collect::<Vec<_>>(),
        [
            "# Minimal task",
            "",
            "## Progress",
            "",
            "- 2026-10-04 10:15: created via tasq create"
        ]
    );
    let names: Vec<&str> = document.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Progress"]);
    assert_eq!(document.section("Progress").unwrap().heading, 2);
    let front = document.frontmatter().unwrap();
    assert_eq!(front.lines(), ["id: 47", "status: ready"]);
    assert_eq!(
        front.entries().collect::<Vec<_>>(),
        [("id", "47"), ("status", "ready")]
    );
    assert_eq!(front.get("id"), Some("47"));
    assert_eq!(front.get("priority"), None);
}

#[test]
fn created_at_is_what_the_caller_passes() {
    let created = at("2026-10-04 10:15");
    let task = format::parse_v2(&fixture("minimal"), Some(created), &Workflow::default())
        .unwrap()
        .task;
    assert_eq!(task.created_at, Some(created));
    assert_eq!(task.id, TaskId::from(47));
}

#[test]
fn full() {
    let task = parse(&fixture("full")).task;
    assert_eq!(task.id, TaskId::from(12));
    assert_eq!(task.title, "Full task");
    assert!(!task.done);
    assert_eq!(task.status, Some(Status::IN_PROGRESS));
    assert_eq!(task.priority, Priority::A);
    assert_eq!(task.rank, Some(1024));
    assert_eq!(task.due, NaiveDate::from_ymd_opt(2026, 10, 10));
    assert_eq!(task.closed_at, None);
    assert_eq!(task.project, Some(PathBuf::from("/home/pau/code/tasks")));
    assert_eq!(
        task.origin,
        Some(Origin {
            source: "gitlab".into(),
            external_id: "!123".into(),
            url: Some("https://gitlab.example.invalid/group/project/-/merge_requests/123".into()),
        })
    );
    assert_eq!(task.tags, vec![tag("gitlab"), tag("review-request")]);
    assert_eq!(
        task.description.as_deref(),
        Some("Rewrite the tasks script in Rust.\nSecond paragraph of the description.")
    );
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
}

#[test]
fn full_document_view() {
    let document = parse(&fixture("full")).document;
    // The blank line between the closing fence and the title belongs to the
    // front region: the title is still body line 0.
    assert_eq!(document.title_line(), "# Full task");
    let names: Vec<(u8, &str, usize)> = document
        .sections()
        .iter()
        .map(|s| (s.level, s.name, s.heading))
        .collect();
    assert_eq!(
        names,
        [
            (2, "Description", 2),
            (2, "Related", 7),
            (2, "Worktrees", 17),
            (2, "Sessions", 22),
            (2, "Progress", 27),
            (2, "Notes", 33),
        ]
    );
    let front = document.frontmatter().unwrap();
    assert_eq!(front.lines().len(), 10);
    assert_eq!(front.get(key::TAGS), Some("[gitlab, review-request]"));
    assert_eq!(front.get(key::SOURCE_ID), Some("!123"));
}

#[test]
fn done() {
    let Parsed { document, task } = parse(&fixture("done"));
    assert!(task.done);
    assert!(document.is_done());
    assert_eq!(document.title(), "Done task");
    assert_eq!(task.status, None);
    assert_eq!(task.priority, Priority::C);
    assert_eq!(task.closed_at, Some(at("2026-10-05 09:00")));
    assert_eq!(task.tags, vec![tag("gitlab")]);
    assert_eq!(task.progress.len(), 2);
    assert_eq!(task.progress[1].note, "shipped");
}

#[test]
fn ranked() {
    let task = parse(&fixture("ranked")).task;
    assert_eq!(task.rank, Some(1024));
    assert_eq!(task.due, NaiveDate::from_ymd_opt(2026, 10, 10));
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.priority, Priority::C);
    assert_eq!(task.tags, vec![tag("gitlab")]);
}

#[test]
fn source() {
    let task = parse(&fixture("source")).task;
    // Unlike the v1 `## Source` line, a url-shaped `source_id` is not the
    // url: only `source_url` is (the writer always writes it).
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
fn crlf() {
    let text = fixture("crlf");
    assert!(text.contains("\r\n"));
    let Parsed { document, task } = parse(&text);
    assert_eq!(document.newline(), Newline::CrLf);
    assert_eq!(task, parse(&fixture("minimal")).task);
    assert_eq!(
        document.frontmatter().unwrap().lines(),
        ["id: 47", "status: ready"]
    );
    assert_eq!(
        document.lines().collect::<Vec<_>>(),
        parse(&fixture("minimal"))
            .document
            .lines()
            .collect::<Vec<_>>()
    );
}

#[test]
fn unknown_keys() {
    let Parsed { document, task } = parse(&fixture("unknown-keys"));
    assert_eq!(task.id, TaskId::from(5));
    assert_eq!(task.status, Some(Status::WAITING), "the first key wins");
    assert_eq!(task.priority, Priority::B);
    let front = document.frontmatter().unwrap();
    assert_eq!(front.get("owner"), Some("pau"));
    assert_eq!(front.get("estimate"), Some("3h"));
    assert_eq!(front.get("note"), Some("a: b"), "split on the first colon");
    assert_eq!(front.get("not an entry"), None);
    assert_eq!(front.get("not"), None);
    assert_eq!(front.get(""), None, "a line starting with ':' is no entry");
    assert_eq!(
        front.entries().map(|(k, _)| k).collect::<Vec<_>>(),
        ["owner", "id", "estimate", "note", "status", "status"]
    );
    assert_eq!(front.lines().len(), 8);
}

#[test]
fn blank_lines() {
    let Parsed { document, task } = parse(&fixture("blank-lines"));
    assert_eq!(task.id, TaskId::from(6));
    assert_eq!(task.status, Some(Status::BLOCKED));
    assert_eq!(task.title, "Task with blank lines in the block");
    assert_eq!(
        document.frontmatter().unwrap().lines(),
        ["", "id: 6", "", "status: blocked", "  ", "priority: B"]
    );
    // A whitespace-only line between the fence and the title is blank too.
    assert!(fixture("blank-lines").contains("---\n\n  \n# Task"));
    assert_eq!(
        document.title_line(),
        "# Task with blank lines in the block"
    );
    assert_eq!(document.line_count(), 5);
}

#[test]
fn extra_sections() {
    let Parsed { document, task } = parse(&fixture("extra-sections"));
    assert_eq!(task.status, Some(Status::WAITING));
    // The v1 scalar sections are unknown sections in a v2 body.
    assert_eq!(task.due, None);
    assert_eq!(task.project, None);
    assert_eq!(task.priority, Priority::B);
    assert_eq!(task.tags, Vec::new());
    assert_eq!(task.origin, None);
    assert_eq!(task.closed_at, None);
    assert_eq!(task.rank, None);
    assert_eq!(task.related, Vec::new(), "## Links is not ## Related");
    assert_eq!(task.progress.len(), 1);
    assert_eq!(
        document.section("Tags").unwrap().trimmed_body(),
        ["#A #ready #ignored"]
    );
    let checklist = document.section("Checklist").unwrap();
    assert_eq!(checklist.subsections().len(), 1);
    assert_eq!(checklist.own_body(), ["", "- [ ] one", "- [x] two", ""]);
}

#[test]
fn status_done_is_the_only_done_marker() {
    let t = |front: &str| parse(&format!("---\nid: 1\n{front}---\n# T\n")).task;
    let done = t("status: done\nclosed: 2026-10-07 14:32\n");
    assert!(done.done);
    assert_eq!(done.status, None);
    assert_eq!(done.closed_at, Some(at("2026-10-07 14:32")));
    // An open task is not closed, whatever `closed` says.
    let open = t("status: ready\nclosed: 2026-10-07 14:32\n");
    assert!(!open.done);
    assert_eq!(open.status, Some(Status::READY));
    assert_eq!(open.closed_at, None);
    // A `# [x]` title is a title, not a marker.
    let marked = parse("---\nid: 1\n---\n# [x] T\n").task;
    assert!(!marked.done);
    assert_eq!(marked.title, "[x] T");
    // The title line keeps the text after `# ` verbatim.
    assert_eq!(parse("---\nid: 1\n---\n#  T \n").task.title, " T ");
}

#[test]
fn status_uses_the_given_workflow() {
    let wf = Workflow::new(vec![
        Status::new("todo").unwrap(),
        Status::new("doing").unwrap(),
    ]);
    let text = "---\nid: 1\nstatus: doing\n---\n# T\n";
    let t = format::parse_v2(text, None, &wf).unwrap().task;
    assert_eq!(t.status, Some(Status::new("doing").unwrap()));
    let t = parse(text).task;
    assert_eq!(t.status, None, "an unknown status reads as none and stays");
    assert_eq!(
        parse("---\nid: 1\nstatus: ready\n---\n# T\n").task.status,
        Some(Status::READY)
    );
}

#[test]
fn scalar_values_that_do_not_parse_read_as_unset() {
    let t = |front: &str| parse(&format!("---\nid: 1\n{front}\n---\n# T\n")).task;
    assert_eq!(t("priority: D").priority, Priority::B);
    assert_eq!(t("priority: a").priority, Priority::B);
    assert_eq!(t("priority: C").priority, Priority::C);
    assert_eq!(t("priority:").priority, Priority::B);
    assert_eq!(t("rank: first").rank, None);
    assert_eq!(t("rank: 1.5").rank, None);
    assert_eq!(t("rank: -2048").rank, Some(-2048));
    assert_eq!(t("due: next friday").due, None);
    assert_eq!(t("due: 2026-10-10 10:00").due, None);
    assert_eq!(
        t("due: 2026-10-10").due,
        NaiveDate::from_ymd_opt(2026, 10, 10)
    );
    assert_eq!(t("status: done\nclosed: 2026-10-07").closed_at, None);
    assert_eq!(t("status: done\nclosed: yesterday").closed_at, None);
    assert_eq!(t("status: done\nclosed:").closed_at, None);
    assert_eq!(t("project:").project, None);
    assert_eq!(t("project:  /a b ").project, Some(PathBuf::from("/a b")));
    assert_eq!(t("status:").status, None);
}

#[test]
fn tags_are_a_bracketed_list_without_hashes() {
    let tags = |value: &str| {
        parse(&format!("---\nid: 1\ntags: {value}\n---\n# T\n"))
            .task
            .tags
    };
    assert_eq!(
        tags("[gitlab, review-request]"),
        vec![tag("gitlab"), tag("review-request")]
    );
    assert_eq!(
        tags("[ gitlab ,review-request ]"),
        vec![tag("gitlab"), tag("review-request")]
    );
    assert_eq!(tags("[gitlab]"), vec![tag("gitlab")]);
    assert_eq!(tags("[]"), Vec::new());
    assert_eq!(tags("[ , ]"), Vec::new());
    assert_eq!(
        tags("[a, a, b]"),
        vec![tag("a"), tag("b")],
        "duplicates collapse"
    );
    assert_eq!(tags("[#a, b]"), vec![tag("b")], "a hash is not a tag");
    assert_eq!(
        tags("[ready, A]"),
        vec![tag("ready"), tag("A")],
        "no status or priority in the list"
    );
    assert_eq!(
        tags("gitlab, review-request"),
        Vec::new(),
        "no brackets, no tags"
    );
    assert_eq!(tags("[gitlab"), Vec::new());
    assert_eq!(tags(""), Vec::new());
}

#[test]
fn origin_needs_a_source_and_an_id() {
    let origin = |front: &str| {
        parse(&format!("---\nid: 1\n{front}\n---\n# T\n"))
            .task
            .origin
    };
    assert_eq!(
        origin("source: gitlab\nsource_id: !1\nsource_url: https://gl.invalid/1"),
        Some(Origin {
            source: "gitlab".into(),
            external_id: "!1".into(),
            url: Some("https://gl.invalid/1".into()),
        })
    );
    assert_eq!(
        origin("source: slack\nsource_id: C123/p456"),
        Some(Origin {
            source: "slack".into(),
            external_id: "C123/p456".into(),
            url: None,
        })
    );
    assert_eq!(
        origin("source: http\nsource_id: http://x.invalid/1"),
        Some(Origin {
            source: "http".into(),
            external_id: "http://x.invalid/1".into(),
            url: None,
        }),
        "a url-shaped id is not the url: absent means unset"
    );
    assert_eq!(origin("source: gitlab"), None);
    assert_eq!(origin("source: gitlab\nsource_id:"), None);
    assert_eq!(
        origin("source_id: !1\nsource_url: https://gl.invalid/1"),
        None
    );
    assert_eq!(origin("source:\nsource_id: !1"), None);
}

fn err(text: &str) -> FormatError {
    format::parse_v2(text, None, &Workflow::default()).unwrap_err()
}

#[test]
fn an_unclosed_block_or_a_missing_id_is_an_error_naming_the_line() {
    assert_eq!(
        err("---\nid: 1\n# T\n"),
        FormatError::UnterminatedFrontmatter
    );
    assert_eq!(
        err("---\nid: 1\n# T\n").to_string(),
        "line 1: the frontmatter has no closing '---'"
    );
    assert_eq!(err("---"), FormatError::UnterminatedFrontmatter);
    assert_eq!(err("---\n"), FormatError::UnterminatedFrontmatter);
    assert_eq!(err("---\n---"), FormatError::MissingId { line: 2 });
    assert_eq!(
        err("---\nstatus: ready\n\n---\n# T\n"),
        FormatError::MissingId { line: 4 }
    );
    assert_eq!(
        err("---\nstatus: ready\n\n---\n# T\n").to_string(),
        "line 4: the frontmatter closes without an 'id' key"
    );
}

#[test]
fn an_invalid_id_is_an_error_naming_its_line() {
    for value in [
        "x",
        "",
        "0",
        "-1",
        "+1",
        "007",
        "1.0",
        "1 0",
        "99999999999999999999",
    ] {
        let text = format!("---\nstatus: ready\nid:{value}\n---\n# T\n");
        assert_eq!(
            err(&text),
            FormatError::InvalidId {
                line: 3,
                value: value.to_owned()
            },
            "{value:?}"
        );
    }
    // The value is trimmed, like every value.
    assert_eq!(
        format::parse_v2("---\nid:  7 \n---\n# T\n", None, &Workflow::default())
            .unwrap()
            .task
            .id,
        TaskId::from(7)
    );
    assert_eq!(
        err("---\nid: x\n---\n# T\n").to_string(),
        "line 2: id \"x\" is not a positive integer"
    );
    // The first `id` line is the one checked.
    assert_eq!(
        err("---\nid: x\nid: 1\n---\n# T\n"),
        FormatError::InvalidId {
            line: 2,
            value: "x".into()
        }
    );
    assert_eq!(
        format::parse_v2("---\nid: 1\nid: x\n---\n# T\n", None, &Workflow::default())
            .unwrap()
            .task
            .id,
        TaskId::from(1)
    );
}

#[test]
fn a_missing_title_is_an_error_naming_the_line() {
    assert_eq!(
        err("---\nid: 1\n---\n"),
        FormatError::MissingTitle {
            line: 4,
            found: String::new()
        }
    );
    assert_eq!(
        err("---\nid: 1\n---"),
        FormatError::MissingTitle {
            line: 4,
            found: String::new()
        }
    );
    assert_eq!(
        err("---\nid: 1\n---\n\n\n"),
        FormatError::MissingTitle {
            line: 6,
            found: String::new()
        }
    );
    assert_eq!(
        err("---\nid: 1\n---\n\n## Tags\n"),
        FormatError::MissingTitle {
            line: 5,
            found: "## Tags".into()
        }
    );
    assert_eq!(
        err("---\nid: 1\n---\n\n## Tags\n").to_string(),
        "line 5: expected the title line '# Title' after the frontmatter, found \"## Tags\""
    );
    for title in ["#", "# ", "#T", " # T", "Title"] {
        assert_eq!(
            err(&format!("---\nid: 1\n---\n{title}\n")),
            FormatError::MissingTitle {
                line: 4,
                found: title.into()
            },
            "{title:?}"
        );
    }
    // The block is checked before the body, so a file with neither an id
    // nor a title reports the id.
    assert_eq!(err("---\n---\n"), FormatError::MissingId { line: 2 });
    assert_eq!(
        err("---\nid: x\n---\n"),
        FormatError::InvalidId {
            line: 2,
            value: "x".into()
        }
    );
}

#[test]
fn a_v1_file_is_not_a_v2_file_and_vice_versa() {
    let wf = Workflow::default();
    let v1 = "# [ ] T\n\n## Tags\n\n#B #ready\n";
    assert_eq!(
        format::parse_v2(v1, None, &wf).unwrap_err(),
        FormatError::NotFrontmatter {
            first_line: "# [ ] T".into()
        }
    );
    assert_eq!(
        format::parse_v2(v1, None, &wf).unwrap_err().to_string(),
        "not a v2 task file: first line \"# [ ] T\" is not '---'"
    );
    // `Document::parse` reads both; the v1 one does not know its id.
    let doc = Document::parse(v1).unwrap();
    assert_eq!(doc.id(), None);
    assert_eq!(doc.frontmatter(), None);
    // Any other markdown file is not a v2 task file either.
    for text in [
        "# T\n",
        "# Notes\n\nsome notes\n",
        "",
        "\n",
        "--- \n",
        "---x\n",
    ] {
        assert_eq!(
            format::parse_v2(text, None, &wf).unwrap_err(),
            FormatError::NotFrontmatter {
                first_line: text.lines().next().unwrap_or("").into()
            },
            "{text:?}"
        );
    }
    // `Document::parse` still says what such a file is not.
    assert_eq!(
        Document::parse("# T\n").unwrap_err(),
        FormatError::NotATask {
            first_line: "# T".into()
        }
    );
    assert_eq!(
        format::parse("--- \nid: 1\n---\n# T\n", TaskId::from(1), &wf).unwrap_err(),
        FormatError::NotATask {
            first_line: "--- ".into()
        }
    );
}

/// Every before/after file of the list-section operation scenarios
/// (`format_ops.rs`) is a task file: all of them survive a parse/render
/// round trip.
#[test]
fn every_ops_fixture_round_trips() {
    let dir = format!("{}/tests/fixtures/v2/ops", env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let once = parse(&text);
        assert_eq!(format::render(&once.document), text, "{}", path.display());
        let twice = parse(&format::render(&once.document));
        assert_eq!(once, twice, "{}", path.display());
        count += 1;
    }
    assert!(count > 60, "expected the ops fixtures, found {count} files");
}

#[test]
fn constants() {
    assert_eq!(format::FENCE, "---");
    assert_eq!(format::TITLE_PREFIX, "# ");
    assert_eq!(key::ID, "id");
    assert_eq!(key::STATUS, "status");
    assert_eq!(key::DONE_STATUS, "done");
    assert_eq!(key::PRIORITY, "priority");
    assert_eq!(key::RANK, "rank");
    assert_eq!(key::DUE, "due");
    assert_eq!(key::CLOSED, "closed");
    assert_eq!(key::PROJECT, "project");
    assert_eq!(key::SOURCE, "source");
    assert_eq!(key::SOURCE_ID, "source_id");
    assert_eq!(key::SOURCE_URL, "source_url");
    assert_eq!(key::TAGS, "tags");
}
