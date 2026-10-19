//! Grammar edge cases of the body's list sections, on hand-written
//! documents: how progress notes, worktrees, sessions, links and
//! descriptions read, and how duplicate and unknown sections behave. The
//! frontmatter grammar is `format_v2.rs`.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use chrono::NaiveDate;
use tasq_core::clock::FixedClock;
use tasq_core::format::{self, SESSION_SEPARATOR, section};
use tasq_core::model::{Link, ProgressEntry, Task, Workflow, Worktree};

/// The task of a document whose body is `body`, behind a minimal block.
fn task(body: &str) -> Task {
    format::parse_v2(
        &format!("---\nid: 1\n---\n{body}"),
        None,
        &Workflow::default(),
    )
    .expect("parses")
    .task
}

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

#[test]
fn section_name_constants_match_the_headings() {
    assert_eq!(section::DESCRIPTION, "Description");
    assert_eq!(section::RELATED, "Related");
    assert_eq!(section::MERGE_REQUESTS, "Merge requests");
    assert_eq!(section::PROGRESS, "Progress");
    assert_eq!(section::WORKTREES, "Worktrees");
    assert_eq!(section::SESSIONS, "Sessions");
    assert_eq!(SESSION_SEPARATOR, " \u{2014} ");
}

#[test]
fn duplicate_sections_are_concatenated_like_the_awk() {
    let t = task(
        "# T\n\n## Progress\n\n- 2026-10-04 10:15: one\n\n## Notes\n\nn\n\n## Progress\n\n- 2026-10-04 10:16: two\n",
    );
    assert_eq!(t.progress.len(), 2);
    assert_eq!(t.progress[1].note, "two");
}

#[test]
fn scalar_sections_in_the_body_are_unknown_sections() {
    // The script's one-value sections read as nothing: the keys are the
    // only source of the scalar fields.
    let t = task(
        "# T\n\n## Due\n\n2026-10-10\n\n## Project\n\n/p\n\n## Tags\n\n#A #ready #x\n\n## Rank\n\n3\n\n## Closed\n\n2026-10-07 14:32\n\n## Source\n\ngitlab: !1\n",
    );
    assert_eq!(t.due, None);
    assert_eq!(t.project, None);
    assert_eq!(t.tags, Vec::new());
    assert_eq!(t.status, None);
    assert_eq!(t.rank, None);
    assert_eq!(t.closed_at, None);
    assert_eq!(t.origin, None);
}

#[test]
fn description_is_trimmed_of_blank_lines_and_joined() {
    assert_eq!(
        task("# T\n\n## Description\n\n\nline one\n\nline two\n\n\n## Notes\n")
            .description
            .as_deref(),
        Some("line one\n\nline two")
    );
    assert_eq!(
        task("# T\n\n## Description\n\n\n\n## Notes\n").description,
        None
    );
    assert_eq!(task("# T\n\n## Description\n").description, None);
    assert_eq!(
        task("# T\n\n## Description\n\none\n\n## Description\n\ntwo\n")
            .description
            .as_deref(),
        Some("one\ntwo")
    );
}

#[test]
fn progress_grammar() {
    let t = task(concat!(
        "# T\n\n## Progress\n\n",
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
        task("# T\n\n## Progress\n\n- 2026-10-04 10:1é: x\n- é\n- 2026-10-04\u{e9}\n").progress,
        Vec::new()
    );
}

#[test]
fn worktree_grammar() {
    let t = task(concat!(
        "# T\n\n## Worktrees\n\n",
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
        "# T\n\n## Sessions\n\n",
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
        "# T\n\n## Related\n\n",
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
        "## Notes\n\nn\n",
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
    let t = task("# T\n\n## Other\n\n### Merge requests\n\n- [MR](https://a.invalid/1)\n");
    assert_eq!(t.merge_requests, Vec::new());
    assert_eq!(t.related, Vec::new());
}
