//! Grammar edge cases of the typed projection, on hand-written documents.

use std::path::PathBuf;

use chrono::NaiveDate;
use tasq_core::clock::FixedClock;
use tasq_core::format::{self, SESSION_SEPARATOR, section};
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Status, Tag, Task, TaskId, Workflow, Worktree,
};

fn task(text: &str) -> Task {
    format::parse(text, TaskId::from(1), &Workflow::default())
        .expect("parses")
        .task
}

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

fn tag(s: &str) -> Tag {
    Tag::new(s).unwrap()
}

#[test]
fn section_name_constants_match_the_headings() {
    assert_eq!(section::DESCRIPTION, "Description");
    assert_eq!(section::PROJECT, "Project");
    assert_eq!(section::DUE, "Due");
    assert_eq!(section::RELATED, "Related");
    assert_eq!(section::MERGE_REQUESTS, "Merge requests");
    assert_eq!(section::TAGS, "Tags");
    assert_eq!(section::PROGRESS, "Progress");
    assert_eq!(section::WORKTREES, "Worktrees");
    assert_eq!(section::SESSIONS, "Sessions");
    assert_eq!(section::SOURCE, "Source");
    assert_eq!(SESSION_SEPARATOR, " \u{2014} ");
}

#[test]
fn tags_last_status_and_priority_win_and_topics_keep_order() {
    let t = task("# [ ] T\n\n## Tags\n\n#zeta #ready #A #alpha\n#C #blocked #zeta\n");
    assert_eq!(t.status, Some(Status::BLOCKED));
    assert_eq!(t.priority, Priority::C);
    assert_eq!(t.tags, vec![tag("zeta"), tag("alpha")]);
}

#[test]
fn tags_ignore_tokens_that_are_not_tags() {
    let t = task("# [ ] T\n\n## Tags\n\nplain ##double # #ready\n");
    assert_eq!(t.status, Some(Status::READY));
    assert_eq!(t.tags, Vec::new());
}

#[test]
fn tags_use_the_given_workflow() {
    let wf = Workflow::new(vec![
        Status::new("todo").unwrap(),
        Status::new("doing").unwrap(),
    ]);
    let text = "# [ ] T\n\n## Tags\n\n#ready #doing\n";
    let t = format::parse(text, TaskId::from(1), &wf).unwrap().task;
    assert_eq!(t.status, Some(Status::new("doing").unwrap()));
    assert_eq!(
        t.tags,
        vec![tag("ready")],
        "ready is a topic tag in this workflow"
    );
}

#[test]
fn done_task_has_no_status_even_with_a_status_tag() {
    let t = task("# [x] T\n\n## Tags\n\n#ready #A\n");
    assert!(t.done);
    assert_eq!(t.status, None);
    assert_eq!(t.priority, Priority::A);
}

#[test]
fn duplicate_sections_are_concatenated_like_the_awk() {
    let t = task(
        "# [ ] T\n\n## Tags\n\n#a\n\n## Progress\n\n- 2026-10-04 10:15: one\n\n## Tags\n\n#b #A\n\n## Progress\n\n- 2026-10-04 10:16: two\n\n## Due\n\n\n\n## Due\n\n2026-01-02\n",
    );
    assert_eq!(t.tags, vec![tag("a"), tag("b")]);
    assert_eq!(t.priority, Priority::A);
    assert_eq!(t.progress.len(), 2);
    assert_eq!(t.due, NaiveDate::from_ymd_opt(2026, 1, 2));
}

#[test]
fn due_takes_the_first_non_empty_line() {
    assert_eq!(
        task("# [ ] T\n\n## Due\n\n\n  2026-10-10  \nignored\n").due,
        NaiveDate::from_ymd_opt(2026, 10, 10)
    );
    assert_eq!(
        task("# [ ] T\n\n## Due\n\ntomorrow\n2026-10-10\n").due,
        None
    );
    assert_eq!(task("# [ ] T\n\n## Due\n\n## Tags\n\n#B\n").due, None);
    assert_eq!(task("# [ ] T\n\n## Due\n\n2026-10-10 10:00\n").due, None);
}

#[test]
fn project_takes_the_first_non_empty_line_and_the_second_section_is_ignored() {
    let t = task("# [ ] T\n\n## Project\n\n\n/a/b\n/c\n\n## Project\n\n/d\n");
    assert_eq!(t.project, Some(PathBuf::from("/a/b")));
    assert_eq!(task("# [ ] T\n\n## Project\n\n").project, None);
}

#[test]
fn description_is_trimmed_of_blank_lines_and_joined() {
    assert_eq!(
        task("# [ ] T\n\n## Description\n\n\nline one\n\nline two\n\n\n## Tags\n")
            .description
            .as_deref(),
        Some("line one\n\nline two")
    );
    assert_eq!(
        task("# [ ] T\n\n## Description\n\n\n\n## Tags\n").description,
        None
    );
    assert_eq!(task("# [ ] T\n\n## Description\n").description, None);
    assert_eq!(
        task("# [ ] T\n\n## Description\n\none\n\n## Description\n\ntwo\n")
            .description
            .as_deref(),
        Some("one\ntwo")
    );
}

#[test]
fn progress_grammar() {
    let t = task(concat!(
        "# [ ] T\n\n## Progress\n\n",
        "- 2026-10-04 10:15: with time\n",
        "- 2026-03-01: date only\n",
        "- 2026-10-04 10:15:no space\n",
        "- 2026-10-04 10:15:   extra spaces\n",
        "- 2026-10-04 10:15:\n",
        "- 2026-10-04 10:15 missing colon\n",
        "- 2026-13-01: bad month\n",
        "- 2026-10-04T10:15: iso t\n",
        "- 2026-10-04 10:15: colon: inside\n",
        "not an entry\n",
        "-2026-10-04 10:15: no space after dash\n",
        "- 2026-10-04 10:1\n",
        "- 2026-10-0\n",
    ));
    let when = at("2026-10-04 10:15");
    assert_eq!(
        t.progress,
        vec![
            ProgressEntry::new(when, "with time"),
            ProgressEntry::dated(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(), "date only"),
            ProgressEntry::new(when, "no space"),
            ProgressEntry::new(when, "extra spaces"),
            ProgressEntry::new(when, ""),
            ProgressEntry::new(when, "colon: inside"),
        ]
    );
}

#[test]
fn progress_with_multibyte_text_near_the_stamp_does_not_panic() {
    assert_eq!(
        task("# [ ] T\n\n## Progress\n\n- 2026-10-04 10:1é: x\n- é\n- 2026-10-04\u{e9}\n").progress,
        Vec::new()
    );
}

#[test]
fn worktree_grammar() {
    let t = task(concat!(
        "# [ ] T\n\n## Worktrees\n\n",
        "- /a (`main`)\n",
        "-  \u{e9}/b\n",
        "- /c   (`feat/x`)\n",
        "- /d (main)\n",
        "- /e (``)\n",
        "- /f (`main`) trailing\n",
        "-  \n",
        "- \n",
        "text\n",
    ));
    assert_eq!(
        t.worktrees,
        vec![
            Worktree::on_branch("/a", "main"),
            Worktree::new("\u{e9}/b"),
            Worktree::on_branch("/c", "feat/x"),
            Worktree::new("/d"),
            Worktree::new("/e"),
            Worktree::new("/f"),
        ]
    );
}

#[test]
fn session_grammar() {
    let t = task(concat!(
        "# [ ] T\n\n## Sessions\n\n",
        "- 2026-10-04 10:15: `abc` \u{2014} desc with \u{2014} dash\n",
        "- 2026-10-04 10:16: `def`\n",
        "- 2026-10-04 10:17: `ghi` - hyphen desc\n",
        "- 2026-10-04 10:18: `` \u{2014} empty id\n",
        "- 2026-10-04: `date-only`\n",
        "- 2026-10-04 10:19: no-backticks\n",
        "- 2026-10-04 10:20: `unterminated\n",
        "- 2026-10-04 10:21:`no-space`\n",
        "- 2026-10-04 10:22: `jkl` \u{2014} \n",
    ));
    let ids: Vec<(&str, Option<&str>)> = t
        .sessions
        .iter()
        .map(|s| (s.id.as_str(), s.description.as_deref()))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("abc", Some("desc with \u{2014} dash")),
            ("def", None),
            ("jkl", Some(""))
        ]
    );
    assert_eq!(t.sessions[0].at, at("2026-10-04 10:15"));
    assert_eq!(t.sessions[1].at, at("2026-10-04 10:16"));
    assert!(t.sessions.iter().all(|s| s.launcher.is_none()));
}

#[test]
fn related_grammar() {
    let t = task(concat!(
        "# [ ] T\n\n## Related\n\n",
        "- https://a.invalid/1\n",
        "- [Label](https://a.invalid/2)\n",
        "- [a](b)](https://a.invalid/3)\n",
        "- [no close](https://a.invalid/4\n",
        "- [nothing]\n",
        "- \n",
        "-\n",
        "  - indented\n",
        "### Merge requests\n\n",
        "- [MR](https://a.invalid/-/merge_requests/5)\n",
        "- https://a.invalid/-/merge_requests/6\n",
        "### Other\n\n",
        "- https://a.invalid/ignored\n",
        "## Tags\n\n#B\n",
    ));
    assert_eq!(
        t.related,
        vec![
            Link::new("https://a.invalid/1"),
            Link::labelled("https://a.invalid/2", "Label"),
            Link::labelled("https://a.invalid/3", "a](b)"),
            Link::new("[no close](https://a.invalid/4"),
            Link::new("[nothing]"),
        ]
    );
    assert_eq!(
        t.merge_requests,
        vec![
            Link::labelled("https://a.invalid/-/merge_requests/5", "MR"),
            Link::new("https://a.invalid/-/merge_requests/6"),
        ]
    );
}

#[test]
fn merge_requests_outside_related_are_ignored() {
    let t = task("# [ ] T\n\n## Other\n\n### Merge requests\n\n- [MR](https://a.invalid/1)\n");
    assert_eq!(t.merge_requests, Vec::new());
    assert_eq!(t.related, Vec::new());
}

#[test]
fn source_grammar() {
    let origin = |line: &str| task(&format!("# [ ] T\n\n## Source\n\n{line}\n")).origin;
    assert_eq!(
        origin("gitlab: https://gl.invalid/g/p/-/merge_requests/1"),
        Some(Origin {
            source: "gitlab".into(),
            external_id: "https://gl.invalid/g/p/-/merge_requests/1".into(),
            url: Some("https://gl.invalid/g/p/-/merge_requests/1".into())
        })
    );
    assert_eq!(
        origin("gitlab: !123 https://gl.invalid/g/p/-/merge_requests/123"),
        Some(Origin {
            source: "gitlab".into(),
            external_id: "!123".into(),
            url: Some("https://gl.invalid/g/p/-/merge_requests/123".into())
        })
    );
    assert_eq!(
        origin("slack: C123/p456"),
        Some(Origin {
            source: "slack".into(),
            external_id: "C123/p456".into(),
            url: None
        })
    );
    assert_eq!(
        origin("  jira:   ABC-1  "),
        Some(Origin {
            source: "jira".into(),
            external_id: "ABC-1".into(),
            url: None
        })
    );
    assert_eq!(
        origin("http: http://x.invalid/1"),
        Some(Origin {
            source: "http".into(),
            external_id: "http://x.invalid/1".into(),
            url: Some("http://x.invalid/1".into())
        })
    );
    assert_eq!(origin("gitlab:"), None);
    assert_eq!(origin("no colon here"), None);
    assert_eq!(origin(": id"), None);
    assert_eq!(origin("two words: id"), None);
    assert_eq!(origin(""), None);
    // First parseable line wins; a later section does not override.
    assert_eq!(
        task("# [ ] T\n\n## Source\n\nbad\n\ngitlab: 1\n\n## Source\n\ngithub: 2\n")
            .origin
            .unwrap()
            .source,
        "gitlab"
    );
}

#[test]
fn double_hash_tokens_are_neither_statuses_nor_priorities() {
    // The script matched `^#(A|B|C)$` and `^#(status)$`; `##A` is neither.
    let t = task("# [ ] T\n\n## Tags\n\n##A ##ready #C #waiting\n");
    assert_eq!(t.status, Some(Status::WAITING));
    assert_eq!(t.priority, Priority::C);
    assert_eq!(t.tags, Vec::new());
    let t = task("# [ ] T\n\n## Tags\n\n##A ##ready\n");
    assert_eq!(t.status, None);
    assert_eq!(t.priority, Priority::B);
}
