//! Property tests: parsing never panics, every parseable text renders back
//! unchanged, operations never panic and keep every other line, and tasks
//! written with `Document::from_task_v2` read back equal.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use proptest::prelude::*;
use tasq_core::clock::When;
use tasq_core::format::{self, Document, FormatError, key, ops};
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Session, Status, Tag, Task, TaskId, Workflow, Worktree,
};

fn workflow() -> Workflow {
    Workflow::default()
}

fn date() -> impl Strategy<Value = NaiveDate> {
    (2000i32..2100, 1u32..=12, 1u32..=28)
        .prop_map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).unwrap())
}

fn date_time() -> impl Strategy<Value = NaiveDateTime> {
    (date(), 0u32..24, 0u32..60)
        .prop_map(|(d, h, m)| d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap()))
}

fn when() -> impl Strategy<Value = When> {
    prop_oneof![
        date().prop_map(When::Date),
        date_time().prop_map(When::DateTime)
    ]
}

fn url() -> impl Strategy<Value = String> {
    "https://[a-z]{1,5}\\.invalid/[a-z0-9/_-]{1,12}"
}

fn label() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 .,:!'-]{1,16}".prop_filter("no ]( sequence", |l| {
        !l.contains("](") && !l.ends_with(' ') && !l.starts_with(' ')
    })
}

fn link() -> impl Strategy<Value = Link> {
    (url(), proptest::option::of(label())).prop_map(|(url, label)| Link { url, label })
}

fn status() -> impl Strategy<Value = Status> {
    proptest::sample::select(Status::DEFAULTS.to_vec())
}

fn topic() -> impl Strategy<Value = Tag> {
    "[a-z][a-z0-9_-]{0,10}"
        .prop_filter("not a status", |s| {
            Workflow::default().parse_status(s).is_none()
        })
        .prop_map(|s| Tag::new(s).unwrap())
}

fn unique_by<T: Clone, K: Eq + std::hash::Hash>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|i| seen.insert(key(i))).collect()
}

fn note() -> impl Strategy<Value = String> {
    "([A-Za-z0-9é#:][A-Za-z0-9 é#:().,-]{0,30})?"
}

fn origin() -> impl Strategy<Value = Origin> {
    (
        "[a-z]{1,8}",
        prop_oneof![
            url().prop_map(|u| (u.clone(), Some(u))),
            ("[A-Za-z0-9!#/_-]{1,10}", proptest::option::of(url())),
        ],
    )
        .prop_map(|(source, (external_id, url))| Origin {
            source,
            external_id,
            url,
        })
}

/// The key of a `key: value` line, by the block's grammar: the text before
/// the first colon, trimmed, when it is one word.
fn key_of(line: &str) -> Option<&str> {
    let (key, _) = line.split_once(':')?;
    let key = key.trim();
    (!key.is_empty() && !key.contains(char::is_whitespace)).then_some(key)
}

/// A line of a frontmatter block that is not the `id` line: a known or
/// unknown key with some value, a blank line, or a line of another shape.
fn front_line() -> impl Strategy<Value = String> {
    prop_oneof![
        (
            "(status|priority|rank|due|closed|project|source|source_id|source_url|tags|owner|x)",
            "(done|ready|A|7|2026-10-10|2026-10-10 10:00|/p|gitlab|\\[a, b\\]|)"
        )
            .prop_map(|(k, v)| if v.is_empty() {
                format!("{k}:")
            } else {
                format!("{k}: {v}")
            }),
        " {0,2}".prop_map(String::from),
        "[a-z ]{1,8}".prop_map(String::from),
        "# [a-z]{1,8}".prop_map(String::from),
    ]
}

/// A body: the title line and up to a dozen lines of headings, list items
/// and text, with any line endings.
fn body() -> impl Strategy<Value = String> {
    "(\\r?\\n(## [A-Za-z ]{0,10}|### Merge requests|- .*|.*)){0,12}(\\r?\\n)?"
}

prop_compose! {
    fn task()(
        title in "[^\\r\\n]{1,40}".prop_filter("title keeps its shape", |t| !t.starts_with(' ') && !t.ends_with(' ') && !t.contains('\t')),
        done in any::<bool>(),
        status in proptest::option::of(status()),
        priority in proptest::sample::select(Priority::ALL.to_vec()),
        due in proptest::option::of(date()),
        description in proptest::option::of("[A-Za-z0-9 ,.é!-]{1,30}(\\n[A-Za-z0-9 ,.é!-]{1,30}){0,2}".prop_filter("lines are not blank", |d| d.lines().all(|l| !l.trim().is_empty() && !l.starts_with(' ') && !l.ends_with(' ')))),
        project in proptest::option::of("(/[a-z0-9._-]{1,8}){1,4}"),
        tags in proptest::collection::vec(topic(), 0..4),
        related in proptest::collection::vec(link(), 0..3),
        merge_requests in proptest::collection::vec((url(), label()), 0..3),
        worktrees in proptest::collection::vec(("(/[a-z0-9._-]{1,8}){1,4}", proptest::option::of("[a-z][a-z0-9/_-]{0,10}")), 0..3),
        sessions in proptest::collection::vec((date_time(), "s[0-9]{1,6}", proptest::option::of("[A-Za-z0-9 é!,.-]{1,20}")), 0..3),
        progress in proptest::collection::vec((when(), note()), 0..4),
        origin in proptest::option::of(origin()),
        closed_at in proptest::option::of(date_time()),
        rank in proptest::option::of(-4096i64..4096),
    ) -> Task {
        let mut t = Task::new(TaskId::from(1), title);
        t.done = done;
        t.status = if done { None } else { status };
        t.priority = priority;
        t.due = due;
        t.description = description;
        t.project = project.map(PathBuf::from);
        t.tags = unique_by(tags, Clone::clone);
        t.related = unique_by(related, |l| l.url.clone());
        t.merge_requests = unique_by(merge_requests.into_iter().map(|(u, l)| Link::labelled(u, l)).collect(), |l| l.url.clone());
        t.worktrees = unique_by(worktrees.into_iter().map(|(p, b)| Worktree { path: p.into(), branch: b }).collect(), |w| w.path.clone());
        t.sessions = unique_by(sessions.into_iter().map(|(at, id, description)| Session { at, id, launcher: None, description }).collect(), |s| s.id.clone());
        t.progress = progress.into_iter().map(|(at, note)| ProgressEntry { at, note }).collect();
        t.origin = origin;
        t.closed_at = closed_at.filter(|_| done);
        t.rank = rank;
        t
    }
}

proptest! {
    #[test]
    fn parsing_any_string_never_panics(text in "\\PC*") {
        let _ = format::parse_v2(&text, None, &workflow());
    }

    #[test]
    fn parsing_task_shaped_text_never_panics(body in ".*") {
        let text = format!("---\nid: 1\n---\n# T\n{body}");
        let parsed = format::parse_v2(&text, None, &workflow()).unwrap();
        prop_assert_eq!(format::render(&parsed.document), text);
    }

    /// Any block of `key: value`, blank and stray lines around an `id`
    /// line, any blank lines before the title, any body: the id read is the
    /// one written, `status: done` is the done marker, and the bytes come
    /// back unchanged.
    #[test]
    fn every_parseable_text_renders_back_unchanged(
        id in 1u64..100_000,
        before in proptest::collection::vec(front_line(), 0..4),
        after in proptest::collection::vec(front_line(), 0..4),
        gap in "(\\r?\\n *){0,2}",
        title in "[^\\r\\n]{1,20}",
        body in body(),
        crlf in any::<bool>(),
        created in proptest::option::of(date_time()),
    ) {
        let nl = if crlf { "\r\n" } else { "\n" };
        let block: Vec<String> = before.iter().cloned().chain([format!("id: {id}")]).chain(after.iter().cloned()).collect();
        let text = format!("---{nl}{}{nl}---{gap}{nl}# {title}{body}", block.join(nl));
        let parsed = format::parse_v2(&text, created, &workflow()).unwrap();
        prop_assert_eq!(format::render(&parsed.document), text.clone());
        prop_assert_eq!(&parsed.task.id, &TaskId::from(id));
        prop_assert_eq!(parsed.task.created_at, created);
        let front = parsed.document.frontmatter();
        let expected: Vec<&str> = block.iter().map(String::as_str).collect();
        prop_assert_eq!(front.lines(), expected.as_slice());
        let first_status = block.iter().find_map(|l| l.strip_prefix("status:")).map(str::trim);
        prop_assert_eq!(parsed.document.is_done(), first_status == Some("done"));
        prop_assert_eq!(parsed.task.done, first_status == Some("done"));
        prop_assert_eq!(parsed.task.title.as_str(), title.as_str());
        let again = format::parse_v2(&format::render(&parsed.document), created, &workflow()).unwrap();
        prop_assert_eq!(again, parsed);
    }

    #[test]
    fn list_operations_never_panic_and_stay_parseable(
        body in "(\\r?\\n(## (Progress|Related|Description|Worktrees|Sessions|[A-Za-z]{1,6})|### Merge requests|- .*|.*)){0,12}(\\r?\\n)?",
        n in 0usize..6,
    ) {
        let wf = workflow();
        let text = format!("---\nid: 1\n---\n# T{body}");
        let mut doc = Document::parse(&text).unwrap();
        let entry = ProgressEntry::new(NaiveDate::from_ymd_opt(2026, 10, 4).unwrap().and_time(NaiveTime::MIN), "n");
        // The append ops report `false` when the entry is already tracked
        // and leave the file untouched, missing final newline included.
        let changed = match n {
            0 => { ops::append_progress(&mut doc, &entry); true }
            1 => ops::append_worktree(&mut doc, &Worktree::new("/w")),
            2 => ops::append_session(&mut doc, &Session { at: entry.at.date_time(), id: "s".into(), launcher: None, description: None }),
            3 => ops::append_merge_request(&mut doc, &Link::labelled("u", "t")),
            4 => ops::append_related(&mut doc, &Link::new("r")),
            _ => { ops::set_description(&mut doc, "d"); true }
        };
        let rendered = format::render(&doc);
        let again = Document::parse(&rendered).unwrap();
        prop_assert_eq!(again.render(), rendered.clone());
        if changed {
            prop_assert!(doc.ends_with_newline());
        } else {
            prop_assert_eq!(rendered, text.clone());
        }
        let task = format::project(&doc, &wf);
        match n {
            0 => prop_assert_eq!(task.progress.last(), Some(&entry)),
            1 => prop_assert!(task.worktrees.iter().any(|w| w.path == Path::new("/w"))),
            2 => prop_assert!(task.sessions.iter().any(|s| s.id == "s") || text.contains("`s`")),
            3 => prop_assert!(task.merge_requests.iter().any(|l| l.url == "u") || !text.contains("## Related") || text.contains("### Merge requests")),
            4 => prop_assert!(task.related.iter().any(|l| l.url == "r")),
            // A `###` line after the title joins the new section (it ends at `## `).
            _ => prop_assert!(task.description.as_deref().is_some_and(|d| d.starts_with('d'))),
        }
    }

    /// A fresh document reads back the same task, and its block has every
    /// key once in canonical order.
    #[test]
    fn tasks_written_from_scratch_read_back_equal(task in task(), created in proptest::option::of(date_time())) {
        let wf = workflow();
        let mut task = task;
        task.created_at = created;
        let doc = Document::from_task_v2(&task);
        let text = format::render(&doc);
        let parsed = format::parse_v2(&text, created, &wf).unwrap();
        prop_assert_eq!(&parsed.task, &task, "{}", text);
        prop_assert_eq!(format::render(&parsed.document), text.clone());
        let ranks: Vec<usize> = parsed.document.frontmatter().entries()
            .map(|(k, _)| key::ORDER.iter().position(|o| *o == k).unwrap())
            .collect();
        prop_assert!(ranks.windows(2).all(|w| w[0] < w[1]), "{:?} in {}", ranks, text);
        prop_assert_eq!(ranks.first(), Some(&0), "id first");
        prop_assert!(!text.contains("## Tags") && !text.contains("## Source") && !text.contains("## Closed"));
    }

    /// Any scalar op on any block: the result re-parses, the body and the
    /// lines of the other keys are untouched, and the key reads back.
    #[test]
    fn scalar_ops_on_any_block_keep_every_other_line(
        id in 1u64..100_000,
        before in proptest::collection::vec(front_line(), 0..4),
        after in proptest::collection::vec(front_line(), 0..4),
        gap in "(\\r?\\n *){0,2}",
        title in "[^\\r\\n]{1,20}",
        body in body(),
        crlf in any::<bool>(),
        n in 0usize..14,
    ) {
        let nl = if crlf { "\r\n" } else { "\n" };
        let block: Vec<String> = before.iter().cloned().chain([format!("id: {id}")]).chain(after.iter().cloned()).collect();
        let text = format!("---{nl}{}{nl}---{gap}{nl}# {title}{body}", block.join(nl));
        let mut doc = Document::parse(&text).unwrap();
        let body_before: Vec<String> = doc.lines().map(str::to_owned).collect();
        let origin = Origin { source: "gl".into(), external_id: "!1".into(), url: Some("https://gl.invalid/1".into()) };
        let date = NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
        // The keys the op owns and the value the first of them must read.
        let (keys, value): (Vec<&str>, Option<&str>) = match n {
            0 => { ops::set_status(&mut doc, &Status::READY); (vec![key::STATUS], Some("ready")) }
            1 => { ops::set_priority(&mut doc, Priority::A); (vec![key::PRIORITY], Some("A")) }
            2 => { ops::set_rank(&mut doc, -3); (vec![key::RANK], Some("-3")) }
            3 => { ops::set_due(&mut doc, date); (vec![key::DUE], Some("2026-10-04")) }
            4 => { ops::set_closed(&mut doc, date.and_time(NaiveTime::MIN)); (vec![key::CLOSED], Some("2026-10-04 00:00")) }
            5 => { ops::set_project(&mut doc, Path::new("/p")); (vec![key::PROJECT], Some("/p")) }
            6 => { ops::set_tags(&mut doc, &[Tag::new("a").unwrap()]); (vec![key::TAGS], Some("[a]")) }
            7 => { ops::set_done(&mut doc); (vec![key::STATUS], Some("done")) }
            8 => { ops::set_origin(&mut doc, &origin); (vec![key::SOURCE, key::SOURCE_ID, key::SOURCE_URL], Some("gl")) }
            9 => { ops::clear_status(&mut doc); (vec![key::STATUS], None) }
            10 => { ops::clear_rank(&mut doc); (vec![key::RANK], None) }
            11 => { ops::set_tags(&mut doc, &[]); (vec![key::TAGS], None) }
            12 => { ops::clear_origin(&mut doc); (vec![key::SOURCE, key::SOURCE_ID, key::SOURCE_URL], None) }
            _ => {
                // `set_open` touches the block only when it said `done`.
                let was_done = doc.is_done();
                ops::set_open(&mut doc);
                (if was_done { vec![key::STATUS] } else { Vec::new() }, None)
            }
        };
        let rendered = format::render(&doc);
        let again = Document::parse(&rendered).unwrap();
        prop_assert_eq!(&again, &doc);
        prop_assert_eq!(again.lines().collect::<Vec<_>>(), body_before.iter().map(String::as_str).collect::<Vec<_>>());
        prop_assert_eq!(doc.ends_with_newline(), text.ends_with('\n'));
        let front = doc.frontmatter();
        let owned = |line: &&str| key_of(line).is_some_and(|k| keys.contains(&k));
        let others: Vec<&str> = front.lines().iter().filter(|l| !owned(l)).copied().collect();
        let others_before: Vec<&str> = block.iter().map(String::as_str).filter(|l| !owned(l)).collect();
        prop_assert_eq!(others, others_before);
        if let Some(&first_key) = keys.first() {
            prop_assert_eq!(front.get(first_key), value);
            let count = front.entries().filter(|(k, _)| *k == first_key).count();
            prop_assert_eq!(count, usize::from(value.is_some()));
        }
        if n == 13 {
            prop_assert!(!doc.is_done());
        }
    }

    /// Whatever follows, a text whose first line is not the fence is not a
    /// task file, and the error names that line.
    #[test]
    fn text_without_a_fence_first_is_not_a_task_file(
        first in "\\PC*".prop_filter("would be the fence", |t| t != "---"),
        rest in "(\\r?\\n\\PC*){0,3}",
    ) {
        let text = format!("{first}{rest}");
        let err = format::parse_v2(&text, None, &workflow()).unwrap_err();
        prop_assert_eq!(&err, &FormatError::NotFrontmatter { first_line: first.clone() });
        prop_assert_eq!(Document::parse(&text), Err(err));
    }
}

/// Found by `list_operations_never_panic_and_stay_parseable` in CI: a
/// session that is already tracked in a file without a final newline is a
/// no-op, and the file keeps its bytes.
#[test]
fn duplicate_session_without_final_newline_is_untouched() {
    let text = "---\nid: 1\n---\n# T\n\n## Sessions\n\n- 2026-10-04 00:00: `s`";
    let mut doc = Document::parse(text).unwrap();
    let at = NaiveDate::from_ymd_opt(2026, 10, 4)
        .unwrap()
        .and_time(NaiveTime::MIN);
    let session = Session {
        at,
        id: "s".into(),
        launcher: None,
        description: None,
    };
    assert!(!ops::append_session(&mut doc, &session));
    assert_eq!(format::render(&doc), text);
}
