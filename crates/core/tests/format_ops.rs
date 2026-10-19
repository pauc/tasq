//! The list-section operations, pinned byte for byte.
//!
//! Every `fixtures/ops/<name>.before.md` / `.after.md` pair is a
//! hand-maintained task file and the bytes it must have after one
//! operation, with the clock fixed at 2026-10-04 10:15. The pairs keep the
//! insertion rules the original script's awk had for the body (new
//! sections before `## Progress`, merge requests nested in `## Related`,
//! the last duplicate heading wins). The table in [`SCENARIOS`] applies the
//! operation and expects the exact bytes; the free-standing tests below
//! pin the rules on hand-written documents.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests read fixtures from disk; the no-I/O rule is for the library"
)]

use tasq_core::clock::FixedClock;
use tasq_core::format::{self, Document, ops};
use tasq_core::model::{Link, ProgressEntry, Session, Task, Workflow, Worktree};

type Op = fn(&mut Document) -> Option<bool>;

const DIR: &str = "ops";

/// The block every hand-written document below starts with.
const FRONT: &str = "---\nid: 1\n---\n";

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

fn current(body: &str) -> Document {
    Document::parse(&file(body)).unwrap()
}

fn file(body: &str) -> String {
    format!("{FRONT}{body}")
}

fn reparse(doc: &Document) -> Task {
    format::parse(&format::render(doc), None, &Workflow::default())
        .unwrap()
        .task
}

/// Scenario name, operation, and the change flag the operation must return
/// (`None` for operations without a return value).
const SCENARIOS: &[(&str, Op, Option<bool>)] = &[
    (
        "progress_after_last",
        |d| {
            ops::append_progress(d, &note("second note"));
            None
        },
        None,
    ),
    (
        "progress_no_progress_section",
        |d| {
            ops::append_progress(d, &note("first note"));
            None
        },
        None,
    ),
    (
        "progress_empty_section",
        |d| {
            ops::append_progress(d, &note("first note"));
            None
        },
        None,
    ),
    (
        "progress_entries_then_text",
        |d| {
            ops::append_progress(d, &note("third"));
            None
        },
        None,
    ),
    (
        "progress_no_trailing_newline",
        |d| {
            ops::append_progress(d, &note("note"));
            None
        },
        None,
    ),
    (
        "worktree_creates_section",
        |d| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/a", "main"),
            ))
        },
        Some(true),
    ),
    (
        "worktree_no_branch",
        |d| Some(ops::append_worktree(d, &Worktree::new("/home/pau/wt/a"))),
        Some(true),
    ),
    (
        "worktree_appends",
        |d| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/b", "dev"),
            ))
        },
        Some(true),
    ),
    (
        "worktree_duplicate",
        |d| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/code/tasks-wt/detached", "other"),
            ))
        },
        Some(false),
    ),
    (
        "worktree_no_progress_section",
        |d| {
            Some(ops::append_worktree(
                d,
                &Worktree::on_branch("/home/pau/wt/a", "main"),
            ))
        },
        Some(true),
    ),
    (
        "session_creates_section",
        |d| Some(ops::append_session(d, &session("s-1", Some("first")))),
        Some(true),
    ),
    (
        "session_no_desc",
        |d| Some(ops::append_session(d, &session("s-1", None))),
        Some(true),
    ),
    (
        "session_appends",
        |d| Some(ops::append_session(d, &session("s-2", Some("second")))),
        Some(true),
    ),
    (
        "session_duplicate",
        |d| Some(ops::append_session(d, &session("abc-123", Some("again")))),
        Some(false),
    ),
    (
        "mr_creates_related_and_sub",
        |d| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_no_progress_section",
        |d| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_appends",
        |d| {
            Some(ops::append_merge_request(
                d,
                &mr("https://gl.invalid/g/p/-/merge_requests/9", "Ninth"),
            ))
        },
        Some(true),
    ),
    (
        "mr_duplicate",
        |d| {
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
        |d| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_related_is_last",
        |d| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "mr_sub_closed_by_other_sub",
        |d| {
            Some(ops::append_merge_request(
                d,
                &mr("https://gl.invalid/g/p/-/merge_requests/2", "Second"),
            ))
        },
        Some(true),
    ),
    (
        "mr_empty_sub",
        |d| Some(ops::append_merge_request(d, &mr(MR1, "First"))),
        Some(true),
    ),
    (
        "description_replaces",
        |d| {
            ops::set_description(d, "One line now.");
            None
        },
        None,
    ),
    (
        "description_multiline",
        |d| {
            ops::set_description(d, "First.\n\nThird, after a blank line.");
            None
        },
        None,
    ),
    (
        "description_inserts_after_title",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_title_only_no_newline",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_inserts_before_adjacent_section",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_replaces_last_section",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_replaces_duplicates",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "description_no_blank_after",
        |d| {
            ops::set_description(d, "New text.");
            None
        },
        None,
    ),
    (
        "clear_description_middle",
        |d| Some(ops::clear_description(d)),
        Some(true),
    ),
    (
        "clear_description_last",
        |d| Some(ops::clear_description(d)),
        Some(true),
    ),
    (
        "clear_description_missing",
        |d| Some(ops::clear_description(d)),
        Some(false),
    ),
];

fn read(name: &str, side: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/{DIR}/{name}.{side}.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Every scenario, applied to `<name>.before.md`, returns its change flag
/// and leaves the bytes of `<name>.after.md`.
#[test]
fn every_scenario_matches_its_pair_byte_for_byte() {
    for (name, op, changed) in SCENARIOS {
        let before = read(name, "before");
        let after = read(name, "after");
        let mut doc = Document::parse(&before).unwrap();
        assert_eq!(op(&mut doc), *changed, "scenario {name} return value");
        assert_eq!(format::render(&doc), after, "scenario {name}");
        assert!(
            doc.ends_with_newline() || before == after,
            "{name}: a write terminates every line"
        );
    }
}

/// Every `<name>.before.md` on disk is a scenario, and every scenario has
/// one.
#[test]
fn every_pair_on_disk_has_a_scenario() {
    let path = format!("{}/tests/fixtures/{DIR}", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".before.md").map(str::to_owned))
        .collect();
    names.sort();
    let mut listed: Vec<String> = SCENARIOS.iter().map(|(n, _, _)| (*n).to_owned()).collect();
    listed.sort();
    assert_eq!(names, listed);
}

/// A scenario returning `false` leaves the file as it was; one returning
/// `true` changes it.
#[test]
fn unchanged_scenarios_are_the_duplicates() {
    for (name, _, changed) in SCENARIOS {
        let same = read(name, "before") == read(name, "after");
        match changed {
            Some(false) => assert!(same, "{name} must be a no-op"),
            Some(true) => assert!(!same, "{name} must change the file"),
            None => {}
        }
    }
}

#[test]
fn operations_keep_the_documents_line_endings() {
    let crlf = read("progress_after_last", "before").replace('\n', "\r\n");
    let mut doc = Document::parse(&crlf).unwrap();
    ops::append_progress(&mut doc, &note("second note"));
    let expected = read("progress_after_last", "after").replace('\n', "\r\n");
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
    let mut doc =
        current("# T\n\n## Related\n\n- https://a.invalid/0\n\n### Merge requests\n\n- [m](u)\n");
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
    let mut doc = current("# T\n\n## Progress\n\n- 2026-10-04 10:15: a\n\n## Progress\n\ntext\n");
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

/// Deliberate deviation from the script, which looked for `` `id` `` and
/// `- <path>` anywhere in the file: the duplicate checks only look where the
/// reader takes sessions and worktrees from, so nothing written is lost on
/// the way back (found by `format_props`: a worktree on branch `s0` hid
/// session `s0`).
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
        "{}/tests/fixtures/full.md",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut parsed = format::parse(&text, None, &wf).unwrap();
    ops::set_description(&mut parsed.document, "Rewritten.");
    ops::append_progress(&mut parsed.document, &note("more"));
    ops::append_worktree(&mut parsed.document, &Worktree::new("/w"));
    ops::append_session(&mut parsed.document, &session("zzz", None));
    ops::append_merge_request(&mut parsed.document, &mr(MR1, "First"));
    ops::append_related(&mut parsed.document, &Link::new("https://a.invalid/1"));
    let task = format::project(&parsed.document, &wf);
    let mut expected = parsed.task.clone();
    expected.description = Some("Rewritten.".into());
    expected.progress.push(note("more"));
    expected.worktrees.push(Worktree::new("/w"));
    expected.sessions.push(session("zzz", None));
    expected.merge_requests.push(mr(MR1, "First"));
    expected.related.push(Link::new("https://a.invalid/1"));
    assert_eq!(task, expected);
    ops::set_done(&mut parsed.document);
    let task = format::project(&parsed.document, &wf);
    expected.mark_done();
    assert_eq!(task, expected);
    ops::set_open(&mut parsed.document);
    let task = format::project(&parsed.document, &wf);
    expected.done = false;
    assert_eq!(task, expected, "reopened without a status");
}
