//! Edit operations against the original script's behaviour.
//!
//! Every `fixtures/ops/<name>.before.md` / `.after.md` pair was produced by
//! running the script's own awk functions (extracted from `original/tasks`)
//! on the before file, with the clock fixed at 2026-10-04 10:15. The table in
//! [`SCENARIOS`] applies the equivalent operation and expects the exact bytes.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests build fixtures on disk; the no-I/O rule is for the library"
)]

use std::path::Path;

use tasq_core::clock::FixedClock;
use tasq_core::format::{self, Document, ops};
use tasq_core::model::{Link, Priority, ProgressEntry, Session, Status, Tag, Workflow, Worktree};

type Op = fn(&mut Document, &Workflow) -> Option<bool>;

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

fn note(text: &str) -> ProgressEntry {
    ProgressEntry::new(at("2026-10-04 10:15"), text)
}

fn session(id: &str, desc: Option<&str>) -> Session {
    Session {
        at: at("2026-10-04 10:15"),
        id: id.into(),
        launcher: None,
        description: desc.map(str::to_owned),
    }
}

fn mr(url: &str, title: &str) -> Link {
    Link::labelled(url, title)
}

const MR1: &str = "https://gl.invalid/g/p/-/merge_requests/1";

/// The ISO date every `due_*` scenario writes.
fn due() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()
}

/// The timestamp every `closed_*` scenario writes.
fn closed() -> chrono::NaiveDateTime {
    at("2026-10-07 14:32")
}

fn xy() -> Vec<Tag> {
    vec![Tag::new("x").unwrap(), Tag::new("y").unwrap()]
}

/// Scenario name, operation, and the change flag the operation must return
/// (`None` for operations without a return value).
const SCENARIOS: &[(&str, Op, Option<bool>)] = &[
    (
        "set_status_replaces",
        |d, w| {
            ops::set_status(d, &Status::BLOCKED, w);
            None
        },
        None,
    ),
    (
        "set_priority_replaces",
        |d, w| {
            ops::set_priority(d, Priority::C, w);
            None
        },
        None,
    ),
    (
        "set_status_no_status_tag",
        |d, w| {
            ops::set_status(d, &Status::READY, w);
            None
        },
        None,
    ),
    (
        "set_status_no_tags_section",
        |d, w| {
            ops::set_status(d, &Status::READY, w);
            None
        },
        None,
    ),
    (
        "set_priority_no_tags_section",
        |d, w| {
            ops::set_priority(d, Priority::A, w);
            None
        },
        None,
    ),
    (
        "set_status_done_task",
        |d, w| {
            ops::set_status(d, &Status::READY, w);
            None
        },
        None,
    ),
    (
        "set_status_many_lines",
        |d, w| {
            ops::set_status(d, &Status::LATER, w);
            None
        },
        None,
    ),
    (
        "set_priority_many_lines",
        |d, w| {
            ops::set_priority(d, Priority::A, w);
            None
        },
        None,
    ),
    (
        "set_status_no_trailing_newline",
        |d, w| {
            ops::set_status(d, &Status::READY, w);
            None
        },
        None,
    ),
    (
        "progress_after_last",
        |d, _| {
            ops::append_progress(d, &note("second note"));
            None
        },
        None,
    ),
    (
        "progress_no_progress_section",
        |d, _| {
            ops::append_progress(d, &note("first note"));
            None
        },
        None,
    ),
    (
        "progress_empty_section",
        |d, _| {
            ops::append_progress(d, &note("first note"));
            None
        },
        None,
    ),
    (
        "progress_entries_then_text",
        |d, _| {
            ops::append_progress(d, &note("third"));
            None
        },
        None,
    ),
    (
        "progress_no_trailing_newline",
        |d, _| {
            ops::append_progress(d, &note("note"));
            None
        },
        None,
    ),
    (
        "worktree_creates_section",
        |d, _| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/a", "main"),
            ))
        },
        Some(true),
    ),
    (
        "worktree_no_branch",
        |d, _| Some(ops::append_worktree(d, &Worktree::new("/home/pau/wt/a"))),
        Some(true),
    ),
    (
        "worktree_appends",
        |d, _| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/b", "dev"),
            ))
        },
        Some(true),
    ),
    (
        "worktree_duplicate",
        |d, _| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/code/tasks-wt/detached", "other"),
            ))
        },
        Some(false),
    ),
    (
        "worktree_no_progress_section",
        |d, _| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/a", "main"),
            ))
        },
        Some(true),
    ),
    (
        "session_creates_section",
        |d, _| Some(ops::append_session(d, &session("s-1", Some("first")))),
        Some(true),
    ),
    (
        "session_no_desc",
        |d, _| Some(ops::append_session(d, &session("s-1", None))),
        Some(true),
    ),
    (
        "session_appends",
        |d, _| Some(ops::append_session(d, &session("s-2", Some("second")))),
        Some(true),
    ),
    (
        "session_duplicate",
        |d, _| Some(ops::append_session(d, &session("abc-123", Some("again")))),
        Some(false),
    ),
    (
        "mr_creates_related_and_sub",
        |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_no_progress_section",
        |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_appends",
        |d, _| {
            Some(ops::append_merge_request(
                d,
                &mr("https://gl.invalid/g/p/-/merge_requests/9", "Ninth"),
            ))
        },
        Some(true),
    ),
    (
        "mr_duplicate",
        |d, _| {
            Some(ops::append_merge_request(
                d,
                &mr(
                    "https://gitlab.example.invalid/group/project/-/merge_requests/123",
                    "Again",
                ),
            ))
        },
        Some(false),
    ),
    (
        "mr_related_exists_without_sub",
        |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_related_is_last",
        |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_sub_closed_by_other_sub",
        |d, _| {
            Some(ops::append_merge_request(
                d,
                &mr("https://gl.invalid/g/p/-/merge_requests/2", "Second"),
            ))
        },
        Some(true),
    ),
    (
        "mr_empty_sub",
        |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "project_replaces",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_replaces_duplicates",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_replaces_multiline",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_replaces_last_section",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_after_description",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_after_title",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_description_last",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_no_sections",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_title_only_no_newline",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "project_description_no_blank",
        |d, _| {
            ops::set_project(d, Path::new("/new/path"));
            None
        },
        None,
    ),
    (
        "done_minimal",
        |d, w| {
            ops::set_done(d, w);
            None
        },
        None,
    ),
    (
        "done_with_note",
        |d, w| {
            ops::append_progress(d, &note("shipped"));
            ops::set_done(d, w);
            None
        },
        None,
    ),
    (
        "done_drops_empty_tags_line",
        |d, w| {
            ops::set_done(d, w);
            None
        },
        None,
    ),
    (
        "done_no_tags_section",
        |d, w| {
            ops::set_done(d, w);
            None
        },
        None,
    ),
    (
        "done_already_done",
        |d, w| {
            ops::set_done(d, w);
            None
        },
        None,
    ),
    (
        "title_open",
        |d, _| {
            ops::set_title(d, "Renamed task");
            None
        },
        None,
    ),
    (
        "title_done",
        |d, _| {
            ops::set_title(d, "Shipped it");
            None
        },
        None,
    ),
    (
        "due_replaces",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_after_project",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_after_description",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_after_title",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_title_only_no_newline",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_project_last_no_blank",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_replaces_last_section",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "due_replaces_duplicates",
        |d, _| {
            ops::set_due(d, due());
            None
        },
        None,
    ),
    (
        "description_replaces",
        |d, _| {
            ops::set_description(d, "One line now.");
            None
        },
        None,
    ),
    (
        "description_multiline",
        |d, _| {
            ops::set_description(d, "First.\n\nThird, after a blank line.");
            None
        },
        None,
    ),
    (
        "description_inserts_after_title",
        |d, _| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_title_only_no_newline",
        |d, _| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_replaces_last_section",
        |d, _| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_replaces_duplicates",
        |d, _| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_no_blank_after",
        |d, _| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "clear_description_middle",
        |d, _| {
            let r = ops::clear_description(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_description_missing",
        |d, _| {
            let r = ops::clear_description(d);
            Some(r)
        },
        Some(false),
    ),
    (
        "clear_due_middle",
        |d, _| {
            let r = ops::clear_due(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_due_last",
        |d, _| {
            let r = ops::clear_due(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_due_missing",
        |d, _| {
            let r = ops::clear_due(d);
            Some(r)
        },
        Some(false),
    ),
    (
        "clear_due_only_section",
        |d, _| {
            let r = ops::clear_due(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_due_no_blank_before",
        |d, _| {
            let r = ops::clear_due(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "closed_replaces",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "closed_after_source",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "closed_after_due",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "closed_after_project",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "closed_after_description",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "closed_after_title",
        |d, _| {
            ops::set_closed(d, closed());
            None
        },
        None,
    ),
    (
        "clear_closed_middle",
        |d, _| {
            let r = ops::clear_closed(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_closed_last",
        |d, _| {
            let r = ops::clear_closed(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_closed_missing",
        |d, _| {
            let r = ops::clear_closed(d);
            Some(r)
        },
        Some(false),
    ),
    (
        "rank_replaces",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_closed",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_source",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_due",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_project",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_description",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "rank_after_title",
        |d, _| {
            ops::set_rank(d, 1024);
            None
        },
        None,
    ),
    (
        "clear_rank_middle",
        |d, _| {
            let r = ops::clear_rank(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_rank_last",
        |d, _| {
            let r = ops::clear_rank(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_rank_missing",
        |d, _| {
            let r = ops::clear_rank(d);
            Some(r)
        },
        Some(false),
    ),
    (
        "clear_project_middle",
        |d, _| {
            let r = ops::clear_project(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "clear_project_duplicates",
        |d, _| {
            let r = ops::clear_project(d);
            Some(r)
        },
        Some(true),
    ),
    (
        "tags_replaces",
        |d, w| {
            ops::set_tags(d, &xy(), w);
            None
        },
        None,
    ),
    (
        "tags_clears",
        |d, w| {
            ops::set_tags(d, &[], w);
            None
        },
        None,
    ),
    (
        "tags_drops_empty_line",
        |d, w| {
            ops::set_tags(d, &[], w);
            None
        },
        None,
    ),
    (
        "tags_no_hash_line",
        |d, w| {
            ops::set_tags(d, &xy(), w);
            None
        },
        None,
    ),
    (
        "tags_no_section",
        |d, w| {
            ops::set_tags(d, &xy(), w);
            None
        },
        None,
    ),
    (
        "tags_no_section_empty",
        |d, w| {
            ops::set_tags(d, &[], w);
            None
        },
        None,
    ),
    (
        "tags_many_lines",
        |d, w| {
            ops::set_tags(d, &xy(), w);
            None
        },
        None,
    ),
    (
        "tags_no_trailing_newline",
        |d, w| {
            ops::set_tags(d, &xy(), w);
            None
        },
        None,
    ),
    (
        "strip_many_lines",
        |d, w| {
            ops::strip_status_tag(d, w);
            None
        },
        None,
    ),
];

fn read(name: &str, side: &str) -> String {
    read_in("ops", name, side)
}

fn read_in(dir: &str, name: &str, side: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/{dir}/{name}.{side}.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Applies every scenario of `table` to `<dir>/<name>.before.md` and expects
/// the change flag and the bytes of `<dir>/<name>.after.md`.
fn check_scenarios(dir: &str, table: &[(&str, Op, Option<bool>)]) {
    let workflow = Workflow::default();
    for (name, op, changed) in table {
        let before = read_in(dir, name, "before");
        let after = read_in(dir, name, "after");
        let mut doc = Document::parse(&before).unwrap();
        assert_eq!(
            op(&mut doc, &workflow),
            *changed,
            "scenario {dir}/{name} return value"
        );
        assert_eq!(format::render(&doc), after, "scenario {dir}/{name}");
        assert!(
            doc.ends_with_newline() || before == after,
            "{dir}/{name}: awk terminates every line"
        );
    }
}

/// Every `<dir>/<name>.before.md` on disk is a scenario of `table`, and
/// every scenario has one.
fn check_pairs_on_disk(dir: &str, table: &[(&str, Op, Option<bool>)]) {
    let path = format!("{}/tests/fixtures/{dir}", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".before.md").map(str::to_owned))
        .collect();
    names.sort();
    let mut listed: Vec<String> = table.iter().map(|(n, _, _)| (*n).to_owned()).collect();
    listed.sort();
    assert_eq!(names, listed, "{dir}");
}

/// A scenario returning `false` leaves the file as it was; one returning
/// `true` changes it.
fn check_unchanged_are_the_duplicates(dir: &str, table: &[(&str, Op, Option<bool>)]) {
    for (name, _, changed) in table {
        let same = read_in(dir, name, "before") == read_in(dir, name, "after");
        match changed {
            Some(false) => assert!(same, "{dir}/{name} must be a no-op"),
            Some(true) => assert!(!same, "{dir}/{name} must change the file"),
            None => {}
        }
    }
}

#[test]
fn every_scenario_matches_the_script_byte_for_byte() {
    check_scenarios("ops", SCENARIOS);
}

#[test]
fn every_pair_on_disk_has_a_scenario() {
    check_pairs_on_disk("ops", SCENARIOS);
}

#[test]
fn unchanged_scenarios_are_the_duplicates() {
    check_unchanged_are_the_duplicates("ops", SCENARIOS);
}

#[test]
fn operations_keep_the_documents_line_endings() {
    let crlf = read("set_status_replaces", "before").replace('\n', "\r\n");
    let mut doc = Document::parse(&crlf).unwrap();
    ops::set_status(&mut doc, &Status::BLOCKED, &Workflow::default());
    ops::append_progress(&mut doc, &note("n"));
    ops::append_worktree(&mut doc, &Worktree::new("/w"));
    ops::append_merge_request(&mut doc, &mr(MR1, "t"));
    ops::set_project(&mut doc, Path::new("/p"));
    let out = format::render(&doc);
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    assert!(out.ends_with("\r\n"));
    let expected = read("set_status_replaces", "after").replace('\n', "\r\n");
    let mut again = Document::parse(&crlf).unwrap();
    ops::set_status(&mut again, &Status::BLOCKED, &Workflow::default());
    assert_eq!(format::render(&again), expected);
}

#[test]
fn append_to_section_is_public_for_other_list_sections() {
    let mut doc = Document::parse("# [ ] T\n\n## Progress\n\n- 2026-10-04 10:15: x\n").unwrap();
    ops::append_to_section(&mut doc, "Checklist", "- [ ] one");
    ops::append_to_section(&mut doc, "Checklist", "- [ ] two");
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Checklist\n\n- [ ] one\n- [ ] two\n\n## Progress\n\n- 2026-10-04 10:15: x\n"
    );
}

#[test]
fn append_to_section_uses_the_last_duplicate_heading() {
    let mut doc = Document::parse(
        "# [ ] T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\n- 2026-10-04 10:15: b\n",
    )
    .unwrap();
    ops::append_progress(&mut doc, &note("c"));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\n- 2026-10-04 10:15: b\n- 2026-10-04 10:15: c\n"
    );
}

#[test]
fn append_related() {
    let wf = Workflow::default();
    let text = "# [ ] T\n\n## Tags\n\n#B\n\n## Progress\n\n- 2026-10-04 10:15: x\n";
    let mut doc = Document::parse(text).unwrap();
    assert!(ops::append_related(
        &mut doc,
        &Link::new("https://a.invalid/1")
    ));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Tags\n\n#B\n\n## Related\n\n- https://a.invalid/1\n\n## Progress\n\n- 2026-10-04 10:15: x\n"
    );
    assert!(!ops::append_related(
        &mut doc,
        &Link::labelled("https://a.invalid/1", "dup")
    ));
    assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
    assert!(ops::append_related(
        &mut doc,
        &Link::labelled("https://a.invalid/2", "Two")
    ));
    let out = format::render(&doc);
    assert_eq!(
        out,
        concat!(
            "# [ ] T\n\n## Tags\n\n#B\n\n",
            "## Related\n\n- https://a.invalid/1\n- [Two](https://a.invalid/2)\n\n",
            "### Merge requests\n\n- [First](https://gl.invalid/g/p/-/merge_requests/1)\n\n",
            "## Progress\n\n- 2026-10-04 10:15: x\n"
        )
    );
    // An MR url is not a related link, and vice versa.
    assert!(ops::append_related(&mut doc, &Link::new(MR1)));
    let parsed = format::parse(
        &format::render(&doc),
        tasq_core::model::TaskId::from(1),
        &wf,
    )
    .unwrap();
    assert_eq!(parsed.task.related.len(), 3);
    assert_eq!(parsed.task.merge_requests.len(), 1);

    // No Progress section: Related is appended at the end; without a `- `
    // line the entry follows a blank line after the heading.
    let mut doc = Document::parse("# [ ] T\n\n## Related\n\ntext\n").unwrap();
    assert!(ops::append_related(
        &mut doc,
        &Link::new("https://a.invalid/1")
    ));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n- https://a.invalid/1\n\ntext\n"
    );
    let mut doc = Document::parse("# [ ] T\n").unwrap();
    assert!(ops::append_related(
        &mut doc,
        &Link::new("https://a.invalid/1")
    ));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n- https://a.invalid/1\n"
    );
    // Related is the last section and ends with an MR subsection.
    let mut doc = Document::parse(
        "# [ ] T\n\n## Related\n\n- https://a.invalid/0\n\n### Merge requests\n\n- [m](u)\n",
    )
    .unwrap();
    assert!(ops::append_related(
        &mut doc,
        &Link::new("https://a.invalid/1")
    ));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n- https://a.invalid/0\n- https://a.invalid/1\n\n### Merge requests\n\n- [m](u)\n"
    );
}

#[test]
fn set_status_on_a_tags_section_without_a_hash_line_inserts_one() {
    // Deviation from the script, which lost the tag silently.
    let mut doc =
        Document::parse("# [ ] T\n\n## Tags\n\n## Progress\n\n- 2026-10-04 10:15: x\n").unwrap();
    ops::set_status(&mut doc, &Status::READY, &Workflow::default());
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Tags\n\n#ready\n\n## Progress\n\n- 2026-10-04 10:15: x\n"
    );
    let mut doc = Document::parse("# [ ] T\n\n## Tags\n").unwrap();
    ops::set_priority(&mut doc, Priority::A, &Workflow::default());
    assert_eq!(format::render(&doc), "# [ ] T\n\n## Tags\n\n#A\n");
}

#[test]
fn set_status_uses_the_workflow_to_recognise_old_statuses() {
    let wf = Workflow::new(vec![
        Status::new("todo").unwrap(),
        Status::new("doing").unwrap(),
    ]);
    let mut doc = Document::parse("# [ ] T\n\n## Tags\n\n#ready #todo #A\n").unwrap();
    ops::set_status(&mut doc, &Status::new("doing").unwrap(), &wf);
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Tags\n\n#ready #A #doing\n",
        "ready is a topic tag here"
    );
    ops::strip_status_tag(&mut doc, &wf);
    assert_eq!(format::render(&doc), "# [ ] T\n\n## Tags\n\n#ready #A\n");
}

#[test]
fn strip_status_tag_without_tags_section_is_a_no_op_even_for_unterminated_files() {
    let mut doc = Document::parse("# [ ] T\n\n## Other\n\n#ready").unwrap();
    ops::strip_status_tag(&mut doc, &Workflow::default());
    assert_eq!(format::render(&doc), "# [ ] T\n\n## Other\n\n#ready");
    assert!(!doc.ends_with_newline());
}

#[test]
fn strip_status_tag_terminates_the_file_even_when_no_tag_line_changes() {
    // The awk rewrite reprints every line terminated, whether or not a line
    // matched: the heading alone is enough for the file to be rewritten.
    let mut doc = Document::parse("# [ ] T\n\n## Tags\n\nno tags here").unwrap();
    ops::strip_status_tag(&mut doc, &Workflow::default());
    assert_eq!(format::render(&doc), "# [ ] T\n\n## Tags\n\nno tags here\n");
    assert!(doc.ends_with_newline());
}

#[test]
fn set_done_only_touches_the_first_line_marker() {
    let mut doc = Document::parse("# [ ] # [ ] nested\n\n# [ ] not a title\n").unwrap();
    ops::set_done(&mut doc, &Workflow::default());
    assert_eq!(
        format::render(&doc),
        "# [x] # [ ] nested\n\n# [ ] not a title\n"
    );
    assert!(doc.is_done());
}

#[test]
fn set_open_only_touches_the_first_line_marker() {
    let mut doc = Document::parse("# [x] # [x] nested\n\n## Tags\n\n#A\n").unwrap();
    ops::set_open(&mut doc);
    assert_eq!(
        format::render(&doc),
        "# [ ] # [x] nested\n\n## Tags\n\n#A\n"
    );
    assert!(!doc.is_done());
    // Already open: nothing changes.
    ops::set_open(&mut doc);
    assert_eq!(
        format::render(&doc),
        "# [ ] # [x] nested\n\n## Tags\n\n#A\n"
    );
}

#[test]
fn operations_round_trip_through_the_task() {
    let wf = Workflow::default();
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/full.md",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut parsed = format::parse(&text, tasq_core::model::TaskId::from(1), &wf).unwrap();
    ops::set_status(&mut parsed.document, &Status::WAITING, &wf);
    ops::set_priority(&mut parsed.document, Priority::B, &wf);
    ops::set_project(&mut parsed.document, Path::new("/elsewhere"));
    ops::append_progress(&mut parsed.document, &note("more"));
    ops::append_worktree(&mut parsed.document, &Worktree::new("/w"));
    ops::append_session(&mut parsed.document, &session("zzz", None));
    ops::append_merge_request(&mut parsed.document, &mr(MR1, "First"));
    let task = format::project(&parsed.document, tasq_core::model::TaskId::from(1), &wf);
    let mut expected = parsed.task.clone();
    expected.status = Some(Status::WAITING);
    expected.priority = Priority::B;
    expected.project = Some("/elsewhere".into());
    expected.progress.push(note("more"));
    expected.worktrees.push(Worktree::new("/w"));
    expected.sessions.push(session("zzz", None));
    expected.merge_requests.push(mr(MR1, "First"));
    assert_eq!(task, expected);
    ops::set_done(&mut parsed.document, &wf);
    let task = format::project(&parsed.document, tasq_core::model::TaskId::from(1), &wf);
    expected.mark_done();
    assert_eq!(task, expected);
    ops::set_open(&mut parsed.document);
    let task = format::project(&parsed.document, tasq_core::model::TaskId::from(1), &wf);
    expected.done = false;
    assert_eq!(task, expected, "reopened without a status");
}

#[test]
fn set_tags_leave_double_hash_tokens_alone() {
    let wf = Workflow::default();
    let mut doc = Document::parse("# [ ] T\n\n## Tags\n\n#x ##A ##ready #A #ready\n").unwrap();
    ops::set_priority(&mut doc, Priority::C, &wf);
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Tags\n\n#x ##A ##ready #ready #C\n"
    );
    ops::strip_status_tag(&mut doc, &wf);
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Tags\n\n#x ##A ##ready #C\n"
    );
}

#[test]
fn append_to_section_takes_the_last_entry_of_any_duplicate_section() {
    // awk never resets `last`: the first section's entry wins over the
    // second section's heading when the second has no `- ` line.
    let mut doc =
        Document::parse("# [ ] T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\ntext\n")
            .unwrap();
    ops::append_progress(&mut doc, &note("c"));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Progress\n\n- 2026-10-04 10:15: a\n- 2026-10-04 10:15: c\n\n## Progress\n\ntext\n"
    );
    // A `- ` line outside the section does not count, a `###` line does not close it.
    let mut doc = Document::parse(
        "# [ ] T\n\n## Other\n\n- x\n\n## Progress\n\n### Sub\n\n- 2026-10-04 10:15: a\n\n## Tags\n\n- y\n",
    )
    .unwrap();
    ops::append_progress(&mut doc, &note("c"));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Other\n\n- x\n\n## Progress\n\n### Sub\n\n- 2026-10-04 10:15: a\n- 2026-10-04 10:15: c\n\n## Tags\n\n- y\n"
    );
}

#[test]
fn merge_request_tracking_scans_the_whole_file_like_the_awk() {
    // A `### Merge requests` under another section counts as tracked...
    let text = format!("# [ ] T\n\n## Other\n\n### Merge requests\n\n- [m]({MR1})\n");
    let mut doc = Document::parse(&text).unwrap();
    assert!(!ops::append_merge_request(&mut doc, &mr(MR1, "Again")));
    assert_eq!(format::render(&doc), text);
    // ...and is where a new entry goes, instead of creating `## Related`.
    let mut doc =
        Document::parse("# [ ] T\n\n## Other\n\n### Merge requests\n\n- [m](u)\n").unwrap();
    assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
    assert_eq!(
        format::render(&doc),
        format!("# [ ] T\n\n## Other\n\n### Merge requests\n\n- [m](u)\n- [First]({MR1})\n")
    );
    // The url outside a subsection, or in a subsection closed by `##`, is not tracked.
    let mut doc = Document::parse(&format!(
        "# [ ] T\n\n## Related\n\n- {MR1}\n\n### Merge requests\n\n### Other\n\n- [m]({MR1})\n\n## Progress\n"
    ))
    .unwrap();
    assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
    assert_eq!(
        format::render(&doc),
        format!(
            "# [ ] T\n\n## Related\n\n- {MR1}\n\n### Merge requests\n\n- [First]({MR1})\n\n### Other\n\n- [m]({MR1})\n\n## Progress\n"
        )
    );
    // Duplicate subsections: the entry follows the last `- ` line of any of them.
    let mut doc = Document::parse(
        "# [ ] T\n\n## Related\n\n### Merge requests\n\n- [a](u1)\n\n## Other\n\n### Merge requests\n\ntext\n",
    )
    .unwrap();
    assert!(ops::append_merge_request(&mut doc, &mr("u2", "b")));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n### Merge requests\n\n- [a](u1)\n- [b](u2)\n\n## Other\n\n### Merge requests\n\ntext\n"
    );
    // Partial url matches do not count: the needle is `(url)` with parentheses.
    let mut doc =
        Document::parse("# [ ] T\n\n## Related\n\n### Merge requests\n\n- [a](u12)\n").unwrap();
    assert!(ops::append_merge_request(&mut doc, &mr("u1", "b")));
    assert!(!ops::append_merge_request(&mut doc, &mr("u1", "c")));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n### Merge requests\n\n- [a](u12)\n- [b](u1)\n"
    );
}

#[test]
fn merge_request_without_a_title_uses_the_url() {
    let mut doc = Document::parse("# [ ] T\n").unwrap();
    assert!(ops::append_merge_request(&mut doc, &Link::new("u")));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Related\n\n### Merge requests\n\n- [u](u)\n"
    );
    assert!(!ops::append_merge_request(&mut doc, &Link::new("u")));
}

#[test]
fn worktree_tracking_matches_the_path_prefix_only() {
    let mut doc = Document::parse("# [ ] T\n\n## Worktrees\n\n- /a/b (`x`)\n- /c\n").unwrap();
    assert!(!ops::append_worktree(&mut doc, &Worktree::new("/a/b")));
    assert!(!ops::append_worktree(
        &mut doc,
        &Worktree::on_branch("/c", "y")
    ));
    assert!(ops::append_worktree(&mut doc, &Worktree::new("/a")));
    assert!(ops::append_worktree(&mut doc, &Worktree::new("/a/b/c")));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Worktrees\n\n- /a/b (`x`)\n- /c\n- /a\n- /a/b/c\n"
    );
}

/// Deliberate deviation from the script, which looked for `` `id` `` and
/// `- <path>` anywhere in the file: the duplicate checks only look where the
/// reader takes sessions and worktrees from, so nothing written is lost on
/// the way back (found by `format_props`: a worktree on branch `s0` hid
/// session `s0`).
#[test]
fn duplicates_are_checked_in_their_own_section_only() {
    let mut doc = Document::parse(
        "# [ ] T\n\n## Description\n\nabout `s-2`\n\n- /tmp/w is scratch\n\n\
         ## Worktrees\n\n- /a (`s-3`)\n\n## Sessions\n\n- 2026-10-04 09:00: `s-1`\n",
    )
    .unwrap();
    assert!(
        ops::append_session(&mut doc, &session("s-2", None)),
        "a mention in the description"
    );
    assert!(
        ops::append_session(&mut doc, &session("s-3", None)),
        "a branch name"
    );
    assert!(!ops::append_session(
        &mut doc,
        &session("s-1", Some("again"))
    ));
    assert!(
        !ops::append_session(&mut doc, &session("s-2", None)),
        "now tracked"
    );
    assert!(
        ops::append_worktree(&mut doc, &Worktree::new("/tmp/w")),
        "a description bullet"
    );
    assert!(!ops::append_worktree(
        &mut doc,
        &Worktree::on_branch("/a", "other")
    ));
    assert!(!ops::append_worktree(&mut doc, &Worktree::new("/tmp/w")));
    assert_eq!(
        format::render(&doc),
        "# [ ] T\n\n## Description\n\nabout `s-2`\n\n- /tmp/w is scratch\n\n\
         ## Worktrees\n\n- /a (`s-3`)\n- /tmp/w\n\n## Sessions\n\n- 2026-10-04 09:00: `s-1`\n\
         - 2026-10-04 10:15: `s-2`\n- 2026-10-04 10:15: `s-3`\n"
    );
    let task = format::parse(
        &format::render(&doc),
        tasq_core::model::TaskId::from(1),
        &Workflow::default(),
    )
    .unwrap()
    .task;
    let ids: Vec<&str> = task.sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["s-1", "s-2", "s-3"]);
    assert_eq!(task.worktrees.len(), 2);
}

/// The list-section operations on the current format (ADR 0026): the same
/// scenarios and checks as above, on documents with a frontmatter block.
/// Each pair under `fixtures/v2/ops/` is the pair of the same name under
/// `fixtures/ops/` with `## Tags`, `## Project` and `## Due` moved into the
/// block and the title without its marker; where that `## Tags` closed the
/// section under test, it is a `## Notes` section instead.
mod current_format {
    use super::*;

    use tasq_core::model::TaskId;

    const DIR: &str = "v2/ops";

    /// The block every hand-written document below starts with.
    const FRONT: &str = "---\nid: 1\n---\n";

    fn current(body: &str) -> Document {
        Document::parse(&file(body)).unwrap()
    }

    fn file(body: &str) -> String {
        format!("{FRONT}{body}")
    }

    fn reparse(doc: &Document) -> tasq_core::model::Task {
        format::parse_v2(&format::render(doc), None, &Workflow::default())
            .unwrap()
            .task
    }

    const SCENARIOS: &[(&str, Op, Option<bool>)] = &[
        (
            "progress_after_last",
            |d, _| {
                ops::append_progress(d, &note("second note"));
                None
            },
            None,
        ),
        (
            "progress_no_progress_section",
            |d, _| {
                ops::append_progress(d, &note("first note"));
                None
            },
            None,
        ),
        (
            "progress_empty_section",
            |d, _| {
                ops::append_progress(d, &note("first note"));
                None
            },
            None,
        ),
        (
            "progress_entries_then_text",
            |d, _| {
                ops::append_progress(d, &note("third"));
                None
            },
            None,
        ),
        (
            "progress_no_trailing_newline",
            |d, _| {
                ops::append_progress(d, &note("note"));
                None
            },
            None,
        ),
        (
            "worktree_creates_section",
            |d, _| {
                Some(ops::append_worktree(
                    d,
                    &Worktree::on_branch("/home/pau/wt/a", "main"),
                ))
            },
            Some(true),
        ),
        (
            "worktree_no_branch",
            |d, _| Some(ops::append_worktree(d, &Worktree::new("/home/pau/wt/a"))),
            Some(true),
        ),
        (
            "worktree_appends",
            |d, _| {
                Some(ops::append_worktree(
                    d,
                    &Worktree::on_branch("/home/pau/wt/b", "dev"),
                ))
            },
            Some(true),
        ),
        (
            "worktree_duplicate",
            |d, _| {
                Some(ops::append_worktree(
                    d,
                    &Worktree::on_branch("/home/pau/code/tasks-wt/detached", "other"),
                ))
            },
            Some(false),
        ),
        (
            "worktree_no_progress_section",
            |d, _| {
                Some(ops::append_worktree(
                    d,
                    &Worktree::on_branch("/home/pau/wt/a", "main"),
                ))
            },
            Some(true),
        ),
        (
            "session_creates_section",
            |d, _| Some(ops::append_session(d, &session("s-1", Some("first")))),
            Some(true),
        ),
        (
            "session_no_desc",
            |d, _| Some(ops::append_session(d, &session("s-1", None))),
            Some(true),
        ),
        (
            "session_appends",
            |d, _| Some(ops::append_session(d, &session("s-2", Some("second")))),
            Some(true),
        ),
        (
            "session_duplicate",
            |d, _| Some(ops::append_session(d, &session("abc-123", Some("again")))),
            Some(false),
        ),
        (
            "mr_creates_related_and_sub",
            |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
            Some(true),
        ),
        (
            "mr_no_progress_section",
            |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
            Some(true),
        ),
        (
            "mr_appends",
            |d, _| {
                Some(ops::append_merge_request(
                    d,
                    &mr("https://gl.invalid/g/p/-/merge_requests/9", "Ninth"),
                ))
            },
            Some(true),
        ),
        (
            "mr_duplicate",
            |d, _| {
                Some(ops::append_merge_request(
                    d,
                    &mr(
                        "https://gitlab.example.invalid/group/project/-/merge_requests/123",
                        "Again",
                    ),
                ))
            },
            Some(false),
        ),
        (
            "mr_related_exists_without_sub",
            |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
            Some(true),
        ),
        (
            "mr_related_is_last",
            |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
            Some(true),
        ),
        (
            "mr_sub_closed_by_other_sub",
            |d, _| {
                Some(ops::append_merge_request(
                    d,
                    &mr("https://gl.invalid/g/p/-/merge_requests/2", "Second"),
                ))
            },
            Some(true),
        ),
        (
            "mr_empty_sub",
            |d, _| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
            Some(true),
        ),
        (
            "description_replaces",
            |d, _| {
                ops::set_description(d, "One line now.");
                None
            },
            None,
        ),
        (
            "description_multiline",
            |d, _| {
                ops::set_description(d, "First.\n\nThird, after a blank line.");
                None
            },
            None,
        ),
        (
            "description_inserts_after_title",
            |d, _| {
                ops::set_description(d, "New text.");
                None
            },
            None,
        ),
        (
            "description_title_only_no_newline",
            |d, _| {
                ops::set_description(d, "New text.");
                None
            },
            None,
        ),
        (
            "description_replaces_last_section",
            |d, _| {
                ops::set_description(d, "New text.");
                None
            },
            None,
        ),
        (
            "description_replaces_duplicates",
            |d, _| {
                ops::set_description(d, "New text.");
                None
            },
            None,
        ),
        (
            "description_no_blank_after",
            |d, _| {
                ops::set_description(d, "New text.");
                None
            },
            None,
        ),
        (
            "clear_description_middle",
            |d, _| Some(ops::clear_description(d)),
            Some(true),
        ),
        (
            "clear_description_missing",
            |d, _| Some(ops::clear_description(d)),
            Some(false),
        ),
    ];

    #[test]
    fn every_scenario_matches_the_script_byte_for_byte() {
        check_scenarios(DIR, SCENARIOS);
    }

    #[test]
    fn every_pair_on_disk_has_a_scenario() {
        check_pairs_on_disk(DIR, SCENARIOS);
    }

    #[test]
    fn unchanged_scenarios_are_the_duplicates() {
        check_unchanged_are_the_duplicates(DIR, SCENARIOS);
    }

    #[test]
    fn operations_keep_the_documents_line_endings() {
        let crlf = read_in(DIR, "progress_after_last", "before").replace('\n', "\r\n");
        let mut doc = Document::parse(&crlf).unwrap();
        ops::append_progress(&mut doc, &note("second note"));
        let expected = read_in(DIR, "progress_after_last", "after").replace('\n', "\r\n");
        assert_eq!(format::render(&doc), expected);
        ops::append_worktree(&mut doc, &Worktree::new("/w"));
        ops::append_session(&mut doc, &session("s-1", None));
        ops::append_merge_request(&mut doc, &mr(MR1, "t"));
        ops::append_related(&mut doc, &Link::new("https://a.invalid/1"));
        ops::set_description(&mut doc, "One.\nTwo.");
        ops::append_to_section(&mut doc, "Checklist", "- [ ] one");
        let out = format::render(&doc);
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn append_to_section_is_public_for_other_list_sections() {
        let mut doc = current("# T\n\n## Progress\n\n- 2026-10-04 10:15: x\n");
        ops::append_to_section(&mut doc, "Checklist", "- [ ] one");
        ops::append_to_section(&mut doc, "Checklist", "- [ ] two");
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Checklist\n\n- [ ] one\n- [ ] two\n\n## Progress\n\n- 2026-10-04 10:15: x\n"
            )
        );
    }

    #[test]
    fn append_to_section_uses_the_last_duplicate_heading() {
        let mut doc = current(
            "# T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\n- 2026-10-04 10:15: b\n",
        );
        ops::append_progress(&mut doc, &note("c"));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\n- 2026-10-04 10:15: b\n- 2026-10-04 10:15: c\n"
            )
        );
    }

    #[test]
    fn append_related() {
        let mut doc = current("# T\n\n## Notes\n\nn\n\n## Progress\n\n- 2026-10-04 10:15: x\n");
        assert!(ops::append_related(
            &mut doc,
            &Link::new("https://a.invalid/1")
        ));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Notes\n\nn\n\n## Related\n\n- https://a.invalid/1\n\n## Progress\n\n- 2026-10-04 10:15: x\n"
            )
        );
        assert!(!ops::append_related(
            &mut doc,
            &Link::labelled("https://a.invalid/1", "dup")
        ));
        assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
        assert!(ops::append_related(
            &mut doc,
            &Link::labelled("https://a.invalid/2", "Two")
        ));
        assert_eq!(
            format::render(&doc),
            file(concat!(
                "# T\n\n## Notes\n\nn\n\n",
                "## Related\n\n- https://a.invalid/1\n- [Two](https://a.invalid/2)\n\n",
                "### Merge requests\n\n- [First](https://gl.invalid/g/p/-/merge_requests/1)\n\n",
                "## Progress\n\n- 2026-10-04 10:15: x\n"
            ))
        );
        // An MR url is not a related link, and vice versa.
        assert!(ops::append_related(&mut doc, &Link::new(MR1)));
        let task = reparse(&doc);
        assert_eq!(task.related.len(), 3);
        assert_eq!(task.merge_requests.len(), 1);

        // No Progress section: Related is appended at the end; without a `- `
        // line the entry follows a blank line after the heading.
        let mut doc = current("# T\n\n## Related\n\ntext\n");
        assert!(ops::append_related(
            &mut doc,
            &Link::new("https://a.invalid/1")
        ));
        assert_eq!(
            format::render(&doc),
            file("# T\n\n## Related\n\n- https://a.invalid/1\n\ntext\n")
        );
        let mut doc = current("# T\n");
        assert!(ops::append_related(
            &mut doc,
            &Link::new("https://a.invalid/1")
        ));
        assert_eq!(
            format::render(&doc),
            file("# T\n\n## Related\n\n- https://a.invalid/1\n")
        );
        // Related is the last section and ends with an MR subsection.
        let mut doc = current(
            "# T\n\n## Related\n\n- https://a.invalid/0\n\n### Merge requests\n\n- [m](u)\n",
        );
        assert!(ops::append_related(
            &mut doc,
            &Link::new("https://a.invalid/1")
        ));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Related\n\n- https://a.invalid/0\n- https://a.invalid/1\n\n### Merge requests\n\n- [m](u)\n"
            )
        );
    }

    #[test]
    fn append_to_section_takes_the_last_entry_of_any_duplicate_section() {
        // `last` is never reset: the first section's entry wins over the
        // second section's heading when the second has no `- ` line.
        let mut doc =
            current("# T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\ntext\n");
        ops::append_progress(&mut doc, &note("c"));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Progress\n\n- 2026-10-04 10:15: a\n- 2026-10-04 10:15: c\n\n## Progress\n\ntext\n"
            )
        );
        // A `- ` line outside the section does not count, a `###` line does not close it.
        let mut doc = current(
            "# T\n\n## Other\n\n- x\n\n## Progress\n\n### Sub\n\n- 2026-10-04 10:15: a\n\n## Notes\n\n- y\n",
        );
        ops::append_progress(&mut doc, &note("c"));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Other\n\n- x\n\n## Progress\n\n### Sub\n\n- 2026-10-04 10:15: a\n- 2026-10-04 10:15: c\n\n## Notes\n\n- y\n"
            )
        );
    }

    #[test]
    fn merge_request_tracking_scans_the_whole_file_like_the_awk() {
        // A `### Merge requests` under another section counts as tracked...
        let text = file(&format!(
            "# T\n\n## Other\n\n### Merge requests\n\n- [m]({MR1})\n"
        ));
        let mut doc = Document::parse(&text).unwrap();
        assert!(!ops::append_merge_request(&mut doc, &mr(MR1, "Again")));
        assert_eq!(format::render(&doc), text);
        // ...and is where a new entry goes, instead of creating `## Related`.
        let mut doc = current("# T\n\n## Other\n\n### Merge requests\n\n- [m](u)\n");
        assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
        assert_eq!(
            format::render(&doc),
            file(&format!(
                "# T\n\n## Other\n\n### Merge requests\n\n- [m](u)\n- [First]({MR1})\n"
            ))
        );
        // The url outside a subsection, or in a subsection closed by `##`, is not tracked.
        let mut doc = current(&format!(
            "# T\n\n## Related\n\n- {MR1}\n\n### Merge requests\n\n### Other\n\n- [m]({MR1})\n\n## Progress\n"
        ));
        assert!(ops::append_merge_request(&mut doc, &mr(MR1, "First")));
        assert_eq!(
            format::render(&doc),
            file(&format!(
                "# T\n\n## Related\n\n- {MR1}\n\n### Merge requests\n\n- [First]({MR1})\n\n### Other\n\n- [m]({MR1})\n\n## Progress\n"
            ))
        );
        // Duplicate subsections: the entry follows the last `- ` line of any of them.
        let mut doc = current(
            "# T\n\n## Related\n\n### Merge requests\n\n- [a](u1)\n\n## Other\n\n### Merge requests\n\ntext\n",
        );
        assert!(ops::append_merge_request(&mut doc, &mr("u2", "b")));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Related\n\n### Merge requests\n\n- [a](u1)\n- [b](u2)\n\n## Other\n\n### Merge requests\n\ntext\n"
            )
        );
        // Partial url matches do not count: the needle is `(url)` with parentheses.
        let mut doc = current("# T\n\n## Related\n\n### Merge requests\n\n- [a](u12)\n");
        assert!(ops::append_merge_request(&mut doc, &mr("u1", "b")));
        assert!(!ops::append_merge_request(&mut doc, &mr("u1", "c")));
        assert_eq!(
            format::render(&doc),
            file("# T\n\n## Related\n\n### Merge requests\n\n- [a](u12)\n- [b](u1)\n")
        );
    }

    #[test]
    fn merge_request_without_a_title_uses_the_url() {
        let mut doc = current("# T\n");
        assert!(ops::append_merge_request(&mut doc, &Link::new("u")));
        assert_eq!(
            format::render(&doc),
            file("# T\n\n## Related\n\n### Merge requests\n\n- [u](u)\n")
        );
        assert!(!ops::append_merge_request(&mut doc, &Link::new("u")));
    }

    #[test]
    fn worktree_tracking_matches_the_path_prefix_only() {
        let mut doc = current("# T\n\n## Worktrees\n\n- /a/b (`x`)\n- /c\n");
        assert!(!ops::append_worktree(&mut doc, &Worktree::new("/a/b")));
        assert!(!ops::append_worktree(
            &mut doc,
            &Worktree::on_branch("/c", "y")
        ));
        assert!(ops::append_worktree(&mut doc, &Worktree::new("/a")));
        assert!(ops::append_worktree(&mut doc, &Worktree::new("/a/b/c")));
        assert_eq!(
            format::render(&doc),
            file("# T\n\n## Worktrees\n\n- /a/b (`x`)\n- /c\n- /a\n- /a/b/c\n")
        );
    }

    #[test]
    fn duplicates_are_checked_in_their_own_section_only() {
        let mut doc = current(
            "# T\n\n## Description\n\nabout `s-2`\n\n- /tmp/w is scratch\n\n\
             ## Worktrees\n\n- /a (`s-3`)\n\n## Sessions\n\n- 2026-10-04 09:00: `s-1`\n",
        );
        assert!(
            ops::append_session(&mut doc, &session("s-2", None)),
            "a mention in the description"
        );
        assert!(
            ops::append_session(&mut doc, &session("s-3", None)),
            "a branch name"
        );
        assert!(!ops::append_session(
            &mut doc,
            &session("s-1", Some("again"))
        ));
        assert!(
            !ops::append_session(&mut doc, &session("s-2", None)),
            "now tracked"
        );
        assert!(
            ops::append_worktree(&mut doc, &Worktree::new("/tmp/w")),
            "a description bullet"
        );
        assert!(!ops::append_worktree(
            &mut doc,
            &Worktree::on_branch("/a", "other")
        ));
        assert!(!ops::append_worktree(&mut doc, &Worktree::new("/tmp/w")));
        assert_eq!(
            format::render(&doc),
            file(
                "# T\n\n## Description\n\nabout `s-2`\n\n- /tmp/w is scratch\n\n\
                 ## Worktrees\n\n- /a (`s-3`)\n- /tmp/w\n\n## Sessions\n\n- 2026-10-04 09:00: `s-1`\n\
                 - 2026-10-04 10:15: `s-2`\n- 2026-10-04 10:15: `s-3`\n"
            )
        );
        let task = reparse(&doc);
        let ids: Vec<&str> = task.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s-1", "s-2", "s-3"]);
        assert_eq!(task.worktrees.len(), 2);
    }

    #[test]
    fn operations_round_trip_through_the_task() {
        let wf = Workflow::default();
        let text = std::fs::read_to_string(format!(
            "{}/tests/fixtures/v2/full.md",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let mut parsed = format::parse_v2(&text, None, &wf).unwrap();
        ops::set_description(&mut parsed.document, "Rewritten.");
        ops::append_progress(&mut parsed.document, &note("more"));
        ops::append_worktree(&mut parsed.document, &Worktree::new("/w"));
        ops::append_session(&mut parsed.document, &session("zzz", None));
        ops::append_merge_request(&mut parsed.document, &mr(MR1, "First"));
        ops::append_related(&mut parsed.document, &Link::new("https://a.invalid/1"));
        let task = format::project(&parsed.document, TaskId::from(999), &wf);
        let mut expected = parsed.task.clone();
        expected.description = Some("Rewritten.".into());
        expected.progress.push(note("more"));
        expected.worktrees.push(Worktree::new("/w"));
        expected.sessions.push(session("zzz", None));
        expected.merge_requests.push(mr(MR1, "First"));
        expected.related.push(Link::new("https://a.invalid/1"));
        assert_eq!(task, expected);
        ops::set_done(&mut parsed.document, &wf);
        let task = format::project(&parsed.document, TaskId::from(999), &wf);
        expected.mark_done();
        assert_eq!(task, expected);
        ops::set_open(&mut parsed.document);
        let task = format::project(&parsed.document, TaskId::from(999), &wf);
        expected.done = false;
        assert_eq!(task, expected, "reopened without a status");
    }
}
