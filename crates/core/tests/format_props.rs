//! Property tests: parsing never panics, every parseable text renders back
//! unchanged, operations never panic, and tasks written with
//! `Document::from_task` read back equal.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use proptest::prelude::*;
use tasq_core::clock::When;
use tasq_core::format::{self, Document, ops};
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
        t
    }
}

proptest! {
    #[test]
    fn parsing_any_string_never_panics(text in "\\PC*") {
        let _ = format::parse(&text, TaskId::from(1), &workflow());
    }

    #[test]
    fn parsing_task_shaped_text_never_panics(body in ".*") {
        let text = format!("# [ ] T\n{body}");
        let parsed = format::parse(&text, TaskId::from(1), &workflow()).unwrap();
        prop_assert_eq!(format::render(&parsed.document), text);
    }

    #[test]
    fn every_parseable_text_renders_back_unchanged(
        marker in prop_oneof![Just("# [ ] "), Just("# [x] ")],
        title in "[^\\r\\n]{1,20}",
        body in "(\\r?\\n(## [A-Za-z ]{0,10}|### Merge requests|- .*|.*)){0,12}(\\r?\\n)?",
    ) {
        let text = format!("{marker}{title}{body}");
        let parsed = format::parse(&text, TaskId::from(1), &workflow()).unwrap();
        prop_assert_eq!(format::render(&parsed.document), text.clone());
        prop_assert_eq!(parsed.document.is_done(), marker.contains('x'));
        let again = format::parse(&format::render(&parsed.document), TaskId::from(1), &workflow()).unwrap();
        prop_assert_eq!(again, parsed);
    }

    #[test]
    fn operations_never_panic_and_stay_parseable(
        body in "(\\r?\\n(## (Tags|Progress|Related|Project|Description|Worktrees|Sessions|[A-Za-z]{1,6})|### Merge requests|- .*|#[a-z]{1,8}( #[A-Z])?|.*)){0,12}(\\r?\\n)?",
        n in 0usize..8,
    ) {
        let wf = workflow();
        let text = format!("# [ ] T{body}");
        let mut doc = Document::parse(&text).unwrap();
        let entry = ProgressEntry::new(NaiveDate::from_ymd_opt(2026, 10, 4).unwrap().and_time(NaiveTime::MIN), "n");
        match n {
            0 => ops::set_status(&mut doc, &Status::READY, &wf),
            1 => ops::set_priority(&mut doc, Priority::A, &wf),
            2 => ops::append_progress(&mut doc, &entry),
            3 => { ops::append_worktree(&mut doc, &Worktree::new("/w")); }
            4 => { ops::append_session(&mut doc, &Session { at: entry.at.date_time(), id: "s".into(), launcher: None, description: None }); }
            5 => { ops::append_merge_request(&mut doc, &Link::labelled("u", "t")); }
            6 => ops::set_project(&mut doc, Path::new("/p")),
            _ => ops::set_done(&mut doc, &wf),
        }
        let rendered = format::render(&doc);
        let again = Document::parse(&rendered).unwrap();
        prop_assert_eq!(again.render(), rendered);
        prop_assert!(doc.ends_with_newline());
        let task = format::project(&doc, TaskId::from(1), &wf);
        match n {
            0 => prop_assert_eq!(task.status, Some(Status::READY)),
            1 => prop_assert_eq!(task.priority, Priority::A),
            2 => prop_assert_eq!(task.progress.last(), Some(&entry)),
            3 => prop_assert!(task.worktrees.iter().any(|w| w.path == Path::new("/w"))),
            4 => prop_assert!(task.sessions.iter().any(|s| s.id == "s") || text.contains("`s`")),
            5 => prop_assert!(task.merge_requests.iter().any(|l| l.url == "u") || !text.contains("## Related") || text.contains("### Merge requests")),
            6 => prop_assert_eq!(task.project, Some(PathBuf::from("/p"))),
            _ => { prop_assert!(task.done); prop_assert_eq!(task.status, None); }
        }
    }

    #[test]
    fn tasks_written_from_scratch_read_back_equal(task in task()) {
        let wf = workflow();
        let doc = Document::from_task(&task, &wf);
        let text = format::render(&doc);
        let parsed = format::parse(&text, TaskId::from(1), &wf).unwrap();
        prop_assert_eq!(&parsed.task, &task, "{}", text);
        prop_assert_eq!(format::render(&parsed.document), text);
    }
}

#[test]
fn from_task_writes_the_create_shape() {
    let wf = workflow();
    let mut task = Task::new(TaskId::from(1), "Full task");
    task.status = Some(Status::IN_PROGRESS);
    task.priority = Priority::A;
    task.description =
        Some("Rewrite the tasks script in Rust.\nSecond paragraph of the description.".into());
    task.project = Some("/home/pau/code/tasks".into());
    task.due = NaiveDate::from_ymd_opt(2026, 10, 10);
    task.related = vec![
        Link::new("https://gitlab.example.invalid/group/project/-/issues/42"),
        Link::new("https://example.invalid/docs/spec"),
    ];
    task.tags = vec![
        Tag::new("gitlab").unwrap(),
        Tag::new("review-request").unwrap(),
    ];
    task.progress = vec![ProgressEntry::new(
        NaiveDate::from_ymd_opt(2026, 10, 4)
            .unwrap()
            .and_hms_opt(10, 15, 0)
            .unwrap(),
        "created from the issue",
    )];
    task.merge_requests = vec![Link::labelled(
        "https://gitlab.example.invalid/group/project/-/merge_requests/123",
        "Add parser",
    )];
    task.worktrees = vec![Worktree::on_branch(
        "/home/pau/code/tasks-wt/feature-a",
        "feature-a",
    )];
    task.sessions = vec![Session {
        at: NaiveDate::from_ymd_opt(2026, 10, 4)
            .unwrap()
            .and_hms_opt(10, 20, 0)
            .unwrap(),
        id: "abc-123".into(),
        launcher: None,
        description: Some("first session".into()),
    }];
    let text = format::render(&Document::from_task(&task, &wf));
    let full = std::fs::read_to_string(format!(
        "{}/tests/fixtures/full.md",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    // Same as the script's file up to the entries added afterwards.
    let prefix_end = full.find("- [Add writer]").unwrap();
    assert_eq!(&text[..prefix_end], &full[..prefix_end]);
    assert_eq!(
        &text[prefix_end..],
        concat!(
            "\n## Tags\n\n#gitlab #review-request #A #in-progress\n\n",
            "## Worktrees\n\n- /home/pau/code/tasks-wt/feature-a (`feature-a`)\n\n",
            "## Sessions\n\n- 2026-10-04 10:20: `abc-123` \u{2014} first session\n\n",
            "## Progress\n\n- 2026-10-04 10:15: created from the issue\n"
        )
    );
}

#[test]
fn from_task_minimal_done_and_source() {
    let wf = workflow();
    let mut task = Task::new(TaskId::from(1), "Minimal");
    assert_eq!(
        format::render(&Document::from_task(&task, &wf)),
        "# [ ] Minimal\n\n## Tags\n\n#B\n\n## Progress\n"
    );
    task.done = true;
    task.status = Some(Status::READY);
    task.description = Some(String::new());
    task.origin = Some(Origin {
        source: "gitlab".into(),
        external_id: "!1".into(),
        url: Some("https://gl.invalid/1".into()),
    });
    assert_eq!(
        format::render(&Document::from_task(&task, &wf)),
        "# [x] Minimal\n\n## Source\n\ngitlab: !1 https://gl.invalid/1\n\n## Tags\n\n#B\n\n## Progress\n"
    );
    task.done = false;
    assert!(format::render(&Document::from_task(&task, &wf)).contains("#B #ready\n"));
    let parsed = format::parse(
        &format::render(&Document::from_task(&task, &wf)),
        TaskId::from(1),
        &wf,
    )
    .unwrap();
    assert_eq!(parsed.task.origin, task.origin);
    assert_eq!(parsed.task.description, None);
}

#[test]
fn source_line_omits_a_url_equal_to_the_id() {
    // `gitlab: <url>` parses as id = url = <url>; writing the url twice would
    // still read back equal, so the rendered line is checked exactly.
    let wf = workflow();
    let mut task = Task::new(TaskId::from(1), "From a url");
    task.origin = Some(Origin {
        source: "gitlab".into(),
        external_id: "https://gl.invalid/g/p/-/issues/7".into(),
        url: Some("https://gl.invalid/g/p/-/issues/7".into()),
    });
    assert_eq!(
        format::render(&Document::from_task(&task, &wf)),
        "# [ ] From a url\n\n## Source\n\ngitlab: https://gl.invalid/g/p/-/issues/7\n\n## Tags\n\n#B\n\n## Progress\n"
    );
    task.origin.as_mut().unwrap().url = None;
    assert_eq!(
        format::render(&Document::from_task(&task, &wf)),
        "# [ ] From a url\n\n## Source\n\ngitlab: https://gl.invalid/g/p/-/issues/7\n\n## Tags\n\n#B\n\n## Progress\n"
    );
}

fn is_title_line(text: &str) -> bool {
    [format::OPEN_PREFIX, format::DONE_PREFIX]
        .iter()
        .any(|p| text.len() > p.len() && text.starts_with(p))
}

proptest! {
    #[test]
    fn text_without_a_title_line_is_not_a_task(
        first in prop_oneof![
            "\\PC*",
            "# \\[[ xX]?\\]\\PC*",
            " ?#? ?\\[ \\] \\PC*",
        ].prop_filter("would be a title line", |t| !is_title_line(t)),
        rest in "(\\r?\\n\\PC*){0,3}",
    ) {
        let text = format!("{first}{rest}");
        let err = format::parse(&text, TaskId::from(1), &workflow()).unwrap_err();
        prop_assert_eq!(&err, &format::FormatError::NotATask { first_line: first.clone() });
        prop_assert_eq!(Document::parse(&text), Err(err));
    }
}
