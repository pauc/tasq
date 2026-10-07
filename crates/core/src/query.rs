//! Queries, filtering and grouping over collections of tasks.
//!
//! Everything here is a pure function over borrowed [`Task`]s: no I/O, no
//! clock, no store. The functions mirror what the original script's `list`
//! and `next` commands did, generalised to a configurable [`Workflow`]:
//!
//! - [`Filter`] selects tasks by status, tag, priority, done flag and title
//!   text. Like the script, a default filter keeps **open** tasks only.
//! - [`sort`] and [`sort_key`] order tasks the way `sorted_group` did:
//!   priority `A` before `B` before `C`, then earliest due date first with
//!   undated tasks last, then by id. [`sort_done`] orders done tasks newest
//!   first by [`closed_key`].
//! - [`group_by_status`] buckets tasks per status in workflow order, with the
//!   tasks that have no status last; empty groups are dropped.
//! - [`next`] picks the task to work on: the first task of the first non-empty
//!   group among the leading workflow statuses (`in-progress`, then `ready`).
//!
//! The typical pipeline is `filter` → `group_by_status` (or [`list`], which
//! does both), or `filter` → `next`.

use std::cmp::Ordering;

use chrono::{NaiveDate, NaiveDateTime};

use crate::model::{ModelError, Priority, Status, Tag, Task, TaskId, Workflow};

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Which status values a [`Filter`] accepts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum StatusFilter {
    /// Any status, including none.
    #[default]
    Any,
    /// Exactly this status.
    Is(Status),
    /// Only tasks without a status.
    None,
}

/// Criteria for selecting tasks; every criterion that is set must match.
///
/// Build one with the chained setters and apply it with [`Filter::matches`]
/// or [`filter`]. The default filter matches every open task, which is what
/// the script's `list` showed: it never printed done todos.
///
/// ```
/// use tasq_core::model::{Status, Tag, TaskId, Task};
/// use tasq_core::query::{Filter, filter};
///
/// let mut a = Task::new(TaskId::from(1), "Fix the Build");
/// a.set_status(Status::READY);
/// let b = Task::new(TaskId::from(2), "Write docs");
/// let tasks = vec![a, b];
///
/// let ready = Filter::default().status(Status::READY);
/// assert_eq!(filter(&tasks, &ready).len(), 1);
///
/// let build = Filter::default().text("build");
/// assert_eq!(filter(&tasks, &build)[0].id, TaskId::from(1));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    status: StatusFilter,
    tags: Vec<Tag>,
    priority: Option<Priority>,
    /// `Some(flag)` keeps tasks whose `done` equals `flag`; `None` keeps both.
    done: Option<bool>,
    /// Lower-cased needle for the title search.
    text: Option<String>,
}

impl Default for Filter {
    /// Open tasks only, no other criteria.
    fn default() -> Self {
        Self {
            status: StatusFilter::Any,
            tags: Vec::new(),
            priority: None,
            done: Some(false),
            text: None,
        }
    }
}

impl Filter {
    /// Same as [`Filter::default`]: open tasks, no other criteria.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep only tasks in `status`. Replaces an earlier `status` or
    /// [`no_status`](Self::no_status) call.
    #[must_use]
    pub fn status(mut self, status: Status) -> Self {
        self.status = StatusFilter::Is(status);
        self
    }

    /// Keep only tasks that have no status (the script's `no-status` group).
    /// Replaces an earlier [`status`](Self::status) call.
    #[must_use]
    pub fn no_status(mut self) -> Self {
        self.status = StatusFilter::None;
        self
    }

    /// Keep only tasks carrying `tag`. Calling this several times requires
    /// all of the tags. Tags compare exactly (case-sensitive), as nb does.
    #[must_use]
    pub fn tag(mut self, tag: Tag) -> Self {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
        self
    }

    /// Keep only tasks with this priority.
    #[must_use]
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Keep only done tasks (`true`) or only open tasks (`false`, the default).
    #[must_use]
    pub fn done(mut self, done: bool) -> Self {
        self.done = Some(done);
        self
    }

    /// Keep both open and done tasks.
    #[must_use]
    pub fn any_done(mut self) -> Self {
        self.done = None;
        self
    }

    /// Keep only tasks whose title contains `needle`, ignoring case.
    /// An empty needle matches everything.
    #[must_use]
    pub fn text(mut self, needle: &str) -> Self {
        self.text = Some(needle.to_lowercase());
        self
    }

    /// The filter the script built from `tasks list <word>`.
    ///
    /// - A word that names a status of `workflow` (with or without `#`)
    ///   gives a status filter.
    /// - `A`, `B`, `C` (or `#A`...) give a priority filter. The script matched
    ///   the word against the file's tags, where priority was a tag, so
    ///   `tasks list A` listed the priority-A todos; priority is a field
    ///   here, so it needs its own case to keep that behaviour.
    /// - Anything else is a topic tag, with one optional leading `#`.
    ///
    /// Fails with [`ModelError::InvalidTag`] when the word is not a valid tag
    /// either (empty, `##x`, contains whitespace). Open tasks only, like the
    /// script.
    pub fn from_word(word: &str, workflow: &Workflow) -> Result<Self, ModelError> {
        if let Some(status) = workflow.parse_status(word) {
            return Ok(Self::default().status(status));
        }
        if let Ok(priority) = word.parse::<Priority>() {
            return Ok(Self::default().priority(priority));
        }
        let tag: Tag = word.parse()?;
        Ok(Self::default().tag(tag))
    }

    /// Whether `task` satisfies every criterion.
    pub fn matches(&self, task: &Task) -> bool {
        let status_ok = match &self.status {
            StatusFilter::Any => true,
            StatusFilter::Is(s) => task.status.as_ref() == Some(s),
            StatusFilter::None => task.status.is_none(),
        };
        status_ok
            && self.tags.iter().all(|t| task.has_tag(t))
            && self.priority.is_none_or(|p| task.priority == p)
            && self.done.is_none_or(|d| task.done == d)
            && self
                .text
                .as_deref()
                .is_none_or(|needle| task.title.to_lowercase().contains(needle))
    }
}

/// The tasks matching `filter`, in their original order.
pub fn filter<'a, I>(tasks: I, filter: &Filter) -> Vec<&'a Task>
where
    I: IntoIterator<Item = &'a Task>,
{
    tasks.into_iter().filter(|t| filter.matches(t)).collect()
}

// ---------------------------------------------------------------------------
// Ordering
// ---------------------------------------------------------------------------

/// A due date that orders undated tasks after every dated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DueKey {
    On(NaiveDate),
    Never,
}

/// An id that orders numeric ids numerically.
///
/// All numeric ids come before all non-numeric ones; non-numeric ids order
/// as text. Equal numbers (`7` and `007`) fall back to text so the order is
/// total and deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IdKey<'a> {
    Number(u64, &'a str),
    Text(&'a str),
}

impl<'a> IdKey<'a> {
    fn of(id: &'a TaskId) -> Self {
        let s = id.as_str();
        let numeric = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        match numeric.then(|| s.parse::<u64>().ok()).flatten() {
            Some(n) => Self::Number(n, s),
            None => Self::Text(s),
        }
    }
}

/// The key tasks are sorted by; compare two of them to order two tasks.
///
/// Orders by priority (`A` < `B` < `C`), then due date ascending with undated
/// tasks last, then id. Ids are compared numerically when they are made of
/// digits (`9` before `10`, as the script's `sort -n` did); any other id sorts
/// after all numeric ids, as text. Two tasks compare equal only when all
/// three parts are equal, which in practice means the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SortKey<'a> {
    priority: Priority,
    due: DueKey,
    id: IdKey<'a>,
}

/// The [`SortKey`] of `task`.
pub fn sort_key(task: &Task) -> SortKey<'_> {
    SortKey {
        priority: task.priority,
        due: task.due.map_or(DueKey::Never, DueKey::On),
        id: IdKey::of(&task.id),
    }
}

/// Orders two ids the way [`sort_key`] does: numerically when both are
/// digit strings, numeric before non-numeric, otherwise as text.
pub fn compare_ids(a: &TaskId, b: &TaskId) -> Ordering {
    IdKey::of(a).cmp(&IdKey::of(b))
}

/// Orders two tasks by their [`sort_key`].
pub fn compare(a: &Task, b: &Task) -> Ordering {
    sort_key(a).cmp(&sort_key(b))
}

/// The tasks in [`sort_key`] order.
pub fn sort<'a, I>(tasks: I) -> Vec<&'a Task>
where
    I: IntoIterator<Item = &'a Task>,
{
    let mut out: Vec<&Task> = tasks.into_iter().collect();
    out.sort_by(|a, b| compare(a, b));
    out
}

/// When `task` was closed, as far as the file tells: its `closed_at`, else
/// the time of its last progress entry (the note of `tasks done`, for tasks
/// closed before `## Closed` existed), else nothing.
pub fn closed_key(task: &Task) -> Option<NaiveDateTime> {
    task.closed_at
        .or_else(|| task.latest_progress().map(|entry| entry.at.date_time()))
}

/// Done tasks newest first: by [`closed_key`] descending, tasks with
/// neither time last, then by id descending (a later id was created later).
pub fn sort_done<'a, I>(tasks: I) -> Vec<&'a Task>
where
    I: IntoIterator<Item = &'a Task>,
{
    let mut out: Vec<&Task> = tasks.into_iter().collect();
    out.sort_by(|a, b| {
        closed_key(b)
            .cmp(&closed_key(a))
            .then_with(|| compare_ids(&b.id, &a.id))
    });
    out
}

/// Whether `task` belongs in the Today view (TUI `T`) on `today`: open,
/// and either in the workflow's first status (`in-progress` by default, the
/// one [`next`] looks at first) or due on or before `today`.
pub fn is_today(task: &Task, today: NaiveDate, workflow: &Workflow) -> bool {
    let doing = task.status.is_some() && task.status.as_ref() == workflow.statuses.first();
    !task.done && (doing || task.due.is_some_and(|due| due <= today))
}

/// Whether `task` was closed on `today`, by its [`closed_key`].
pub fn closed_on(task: &Task, today: NaiveDate) -> bool {
    task.done && closed_key(task).is_some_and(|at| at.date() == today)
}

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// The tasks in one status, sorted. `status` is `None` for the `no-status`
/// group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group<'a> {
    /// The shared status, or `None` for tasks without one.
    pub status: Option<Status>,
    /// The tasks, in [`sort_key`] order; never empty.
    pub tasks: Vec<&'a Task>,
}

/// Buckets `tasks` by status in `workflow` order, each bucket sorted.
///
/// Groups come out in workflow order; empty groups are omitted. Tasks whose
/// status is not in the workflow (a status removed from the configuration,
/// say) are not lost: they get their own groups after the workflow ones,
/// ordered by status name. Tasks with no status form the last group.
///
/// Done tasks are grouped like any other; apply a [`Filter`] first (or use
/// [`list`]) to leave them out.
pub fn group_by_status<'a, I>(tasks: I, workflow: &Workflow) -> Vec<Group<'a>>
where
    I: IntoIterator<Item = &'a Task>,
{
    let tasks: Vec<&Task> = tasks.into_iter().collect();

    let mut statuses: Vec<Option<Status>> = workflow.statuses.iter().cloned().map(Some).collect();
    let mut unknown: Vec<Status> = tasks
        .iter()
        .filter_map(|t| t.status.clone())
        .filter(|s| !workflow.contains(s))
        .collect();
    unknown.sort();
    unknown.dedup();
    statuses.extend(unknown.into_iter().map(Some));
    statuses.push(None);

    statuses
        .into_iter()
        .filter_map(|status| {
            let members = sort(tasks.iter().copied().filter(|t| t.status == status));
            (!members.is_empty()).then_some(Group {
                status,
                tasks: members,
            })
        })
        .collect()
}

/// `filter` then [`group_by_status`]: what `tasks list` printed.
pub fn list<'a, I>(tasks: I, filter_by: &Filter, workflow: &Workflow) -> Vec<Group<'a>>
where
    I: IntoIterator<Item = &'a Task>,
{
    group_by_status(filter(tasks, filter_by), workflow)
}

// ---------------------------------------------------------------------------
// Next
// ---------------------------------------------------------------------------

/// How many leading workflow statuses [`next`] considers.
///
/// The script looked at `in-progress`, then `ready`: the first two of its
/// five statuses. Statuses further down (`waiting`, `blocked`, `later`) are
/// by definition not actionable right now, so a custom workflow is expected
/// to keep the same shape: actionable statuses first.
pub const NEXT_STATUSES: usize = 2;

/// The task to pick up next: the first task, in [`sort_key`] order, of the
/// first status in `statuses` that has any tasks.
///
/// Returns `None` when no task is in any of the given statuses. Tasks are
/// not filtered by `done`; apply a [`Filter`] first if the input may contain
/// done tasks (a done task has no status, so this only matters for data that
/// breaks that invariant).
pub fn next_from<'a, I>(tasks: I, statuses: &[Status]) -> Option<&'a Task>
where
    I: IntoIterator<Item = &'a Task>,
{
    let tasks: Vec<&Task> = tasks.into_iter().collect();
    statuses.iter().find_map(|status| {
        tasks
            .iter()
            .copied()
            .filter(|t| t.status.as_ref() == Some(status))
            .min_by(|a, b| compare(a, b))
    })
}

/// The task to pick up next, as the script's `tasks next` chose it: the top
/// task of the first non-empty group among the first [`NEXT_STATUSES`]
/// statuses of `workflow` (`in-progress`, then `ready` by default).
///
/// Returns `None` when nothing is in those statuses, where the script died
/// with "no in-progress or ready todos". Use [`next_from`] to choose the
/// statuses explicitly.
pub fn next<'a, I>(tasks: I, workflow: &Workflow) -> Option<&'a Task>
where
    I: IntoIterator<Item = &'a Task>,
{
    let end = workflow.statuses.len().min(NEXT_STATUSES);
    next_from(tasks, &workflow.statuses[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn status(s: &str) -> Status {
        Status::new(s).unwrap()
    }

    fn tag(s: &str) -> Tag {
        Tag::new(s).unwrap()
    }

    /// A task built from a compact description.
    fn task(
        id: &str,
        title: &str,
        status: Option<&str>,
        prio: Priority,
        due: Option<&str>,
        tags: &[&str],
    ) -> Task {
        let mut t = Task::new(TaskId::new(id).unwrap(), title);
        t.status = status.map(|s| Status::new(s).unwrap());
        t.priority = prio;
        t.due = due.map(date);
        t.tags = tags.iter().map(|s| tag(s)).collect();
        t
    }

    fn ids<'a>(tasks: &[&'a Task]) -> Vec<&'a str> {
        tasks.iter().map(|t| t.id.as_str()).collect()
    }

    fn fixture() -> Vec<Task> {
        use Priority::{A, B, C};
        let mut done = task("6", "Shipped thing", None, A, None, &["gitlab"]);
        done.done = true;
        vec![
            task(
                "1",
                "Review MR",
                Some("ready"),
                B,
                Some("2026-10-10"),
                &["gitlab"],
            ),
            task("2", "Fix build", Some("in-progress"), B, None, &["ci"]),
            task("3", "Plan Q4", Some("later"), A, None, &[]),
            task(
                "4",
                "Write docs",
                None,
                C,
                Some("2026-10-01"),
                &["docs", "gitlab"],
            ),
            task(
                "5",
                "Ping vendor",
                Some("waiting"),
                A,
                Some("2026-10-05"),
                &[],
            ),
            done,
            task("10", "Hotfix", Some("in-progress"), A, None, &["ci"]),
        ]
    }

    // -- Done ordering -------------------------------------------------------

    #[test]
    fn done_tasks_sort_newest_first_by_closing_time_then_last_note() {
        use crate::clock::FixedClock;
        use crate::model::ProgressEntry;
        let at = |s: &str| FixedClock::at(s).0;
        let mut stamped = task("3", "Stamped", None, Priority::B, None, &[]);
        stamped.closed_at = Some(at("2026-10-07 14:32"));
        // A later note does not move a task that has a closing time.
        stamped
            .progress
            .push(ProgressEntry::new(at("2026-10-09 08:00"), "late note"));
        let mut noted = task("8", "Noted", None, Priority::B, None, &[]);
        noted
            .progress
            .push(ProgressEntry::new(at("2026-10-01 09:00"), "first"));
        noted
            .progress
            .push(ProgressEntry::new(at("2026-10-08 09:00"), "shipped"));
        let mut legacy = task("9", "Legacy", None, Priority::B, None, &[]);
        legacy
            .progress
            .push(ProgressEntry::dated(date("2026-10-07"), "dated only"));
        let bare_low = task("2", "Bare", None, Priority::B, None, &[]);
        let bare_high = task("10", "Bare too", None, Priority::B, None, &[]);

        assert_eq!(closed_key(&stamped), Some(at("2026-10-07 14:32")));
        assert_eq!(closed_key(&noted), Some(at("2026-10-08 09:00")));
        assert_eq!(closed_key(&legacy), Some(at("2026-10-07 00:00")));
        assert_eq!(closed_key(&bare_low), None);

        let tasks = [&bare_low, &legacy, &stamped, &bare_high, &noted];
        assert_eq!(
            ids(&sort_done(tasks.iter().copied())),
            ["8", "3", "9", "10", "2"]
        );
    }

    #[test]
    fn today_is_doing_or_due_by_today_and_closed_on_is_the_day() {
        use crate::clock::FixedClock;
        let today = date("2026-10-07");
        let wf = Workflow::default();
        let check = |status: Option<&str>, due: Option<&str>| {
            is_today(&task("1", "T", status, Priority::B, due, &[]), today, &wf)
        };
        assert!(check(Some("in-progress"), None));
        assert!(check(Some("ready"), Some("2026-10-07")), "due today");
        assert!(check(Some("waiting"), Some("2026-10-01")), "overdue");
        assert!(check(None, Some("2026-10-06")), "any status, none included");
        assert!(!check(Some("ready"), Some("2026-10-08")), "due tomorrow");
        assert!(!check(Some("ready"), None));
        assert!(!check(None, None), "no status is not the first status");
        let mut done = task("2", "T", None, Priority::B, Some("2026-10-01"), &[]);
        done.done = true;
        assert!(
            !is_today(&done, today, &wf),
            "done tasks are not today's work"
        );
        // The first status of a custom workflow, not the literal in-progress.
        let custom = Workflow::new(vec![Status::new("doing").unwrap(), Status::READY]);
        assert!(is_today(
            &task("3", "T", Some("doing"), Priority::B, None, &[]),
            today,
            &custom
        ));
        assert!(!is_today(
            &task("4", "T", Some("in-progress"), Priority::B, None, &[]),
            today,
            &custom
        ));
        assert!(!is_today(
            &task("5", "T", None, Priority::B, None, &[]),
            today,
            &Workflow::new(Vec::new())
        ));

        let mut closed = task("6", "T", None, Priority::B, None, &[]);
        closed.close(&FixedClock::at("2026-10-07 18:00"));
        assert!(closed_on(&closed, today));
        assert!(!closed_on(&closed, date("2026-10-08")));
        closed.done = false;
        assert!(!closed_on(&closed, today), "an open task was not closed");
        assert!(!closed_on(&done, today), "no closing time, no note");
    }

    // -- Filter --------------------------------------------------------------

    #[test]
    fn default_filter_keeps_open_tasks_only() {
        let tasks = fixture();
        let open = filter(&tasks, &Filter::default());
        assert_eq!(ids(&open), ["1", "2", "3", "4", "5", "10"]);
        assert_eq!(Filter::new(), Filter::default());
        assert!(!Filter::default().matches(&tasks[5]));
    }

    #[test]
    fn done_filter_selects_either_side_or_both() {
        let tasks = fixture();
        assert_eq!(ids(&filter(&tasks, &Filter::new().done(true))), ["6"]);
        assert_eq!(
            ids(&filter(&tasks, &Filter::new().done(false))),
            ["1", "2", "3", "4", "5", "10"]
        );
        assert_eq!(filter(&tasks, &Filter::new().any_done()).len(), 7);
    }

    #[test]
    fn status_filter_matches_exactly_and_no_status_matches_none() {
        let tasks = fixture();
        let cases: &[(Filter, &[&str])] = &[
            (Filter::new().status(Status::IN_PROGRESS), &["2", "10"]),
            (Filter::new().status(Status::READY), &["1"]),
            (Filter::new().status(Status::BLOCKED), &[]),
            (Filter::new().no_status(), &["4"]),
            // no_status on done tasks too: the done task has no status.
            (Filter::new().no_status().any_done(), &["4", "6"]),
            // the last status call wins
            (Filter::new().no_status().status(Status::LATER), &["3"]),
            (Filter::new().status(Status::LATER).no_status(), &["4"]),
        ];
        for (f, expected) in cases {
            assert_eq!(ids(&filter(&tasks, f)), *expected, "{f:?}");
        }
    }

    #[test]
    fn tag_filter_is_exact_and_conjunctive() {
        let tasks = fixture();
        let cases: &[(Filter, &[&str])] = &[
            (Filter::new().tag(tag("gitlab")), &["1", "4"]),
            (
                Filter::new().tag(tag("gitlab")).any_done(),
                &["1", "4", "6"],
            ),
            (Filter::new().tag(tag("Gitlab")), &[]),
            (Filter::new().tag(tag("gitlab")).tag(tag("docs")), &["4"]),
            (
                Filter::new().tag(tag("gitlab")).tag(tag("gitlab")),
                &["1", "4"],
            ),
            (Filter::new().tag(tag("nope")), &[]),
        ];
        for (f, expected) in cases {
            assert_eq!(ids(&filter(&tasks, f)), *expected, "{f:?}");
        }
        assert_eq!(
            Filter::new().tag(tag("a")).tag(tag("a")),
            Filter::new().tag(tag("a"))
        );
    }

    #[test]
    fn priority_filter() {
        let tasks = fixture();
        assert_eq!(
            ids(&filter(&tasks, &Filter::new().priority(Priority::A))),
            ["3", "5", "10"]
        );
        assert_eq!(
            ids(&filter(&tasks, &Filter::new().priority(Priority::B))),
            ["1", "2"]
        );
        assert_eq!(
            ids(&filter(&tasks, &Filter::new().priority(Priority::C))),
            ["4"]
        );
    }

    #[test]
    fn text_filter_is_case_insensitive_substring_on_title() {
        let tasks = fixture();
        let cases: &[(&str, &[&str])] = &[
            ("fix", &["2", "10"]),
            ("FIX", &["2", "10"]),
            ("Hotfix", &["10"]),
            ("mr", &["1"]),
            ("", &["1", "2", "3", "4", "5", "10"]),
            ("gitlab", &[]), // tags are not searched
            ("zzz", &[]),
        ];
        for (needle, expected) in cases {
            assert_eq!(
                ids(&filter(&tasks, &Filter::new().text(needle))),
                *expected,
                "{needle:?}"
            );
        }
        // Unicode case folding, not just ASCII.
        let t = task("1", "Ärger", None, Priority::B, None, &[]);
        assert!(Filter::new().text("ärger").matches(&t));
        assert!(Filter::new().text("ÄRGER").matches(&t));
    }

    #[test]
    fn criteria_combine_with_and() {
        let tasks = fixture();
        let f = Filter::new()
            .status(Status::IN_PROGRESS)
            .priority(Priority::A)
            .tag(tag("ci"))
            .text("hot");
        assert_eq!(ids(&filter(&tasks, &f)), ["10"]);
        let f = f.priority(Priority::B);
        assert_eq!(ids(&filter(&tasks, &f)), Vec::<&str>::new());
    }

    #[test]
    fn filter_preserves_input_order() {
        let tasks = fixture();
        let reversed: Vec<&Task> = tasks.iter().rev().collect();
        let open = filter(reversed, &Filter::default());
        assert_eq!(ids(&open), ["10", "5", "4", "3", "2", "1"]);
    }

    #[test]
    fn from_word_picks_status_then_priority_then_tag() {
        let wf = Workflow::default();
        let cases: &[(&str, Filter)] = &[
            ("ready", Filter::new().status(Status::READY)),
            ("#ready", Filter::new().status(Status::READY)),
            ("in-progress", Filter::new().status(Status::IN_PROGRESS)),
            ("A", Filter::new().priority(Priority::A)),
            ("#C", Filter::new().priority(Priority::C)),
            ("gitlab", Filter::new().tag(tag("gitlab"))),
            ("#gitlab", Filter::new().tag(tag("gitlab"))),
            ("Ready", Filter::new().tag(tag("Ready"))), // not a status: case matters
            ("a", Filter::new().tag(tag("a"))),         // not a priority
            ("done", Filter::new().tag(tag("done"))),
        ];
        for (word, expected) in cases {
            assert_eq!(
                Filter::from_word(word, &wf).as_ref(),
                Ok(expected),
                "{word:?}"
            );
        }
    }

    #[test]
    fn from_word_rejects_invalid_tags() {
        let wf = Workflow::default();
        assert_eq!(
            Filter::from_word("", &wf),
            Err(ModelError::InvalidTag {
                tag: String::new(),
                reason: "must not be empty",
            })
        );
        assert_eq!(
            Filter::from_word("##x", &wf),
            Err(ModelError::InvalidTag {
                tag: "#x".into(),
                reason: "must not start with '#'",
            })
        );
        assert_eq!(
            Filter::from_word("git lab", &wf),
            Err(ModelError::InvalidTag {
                tag: "git lab".into(),
                reason: "must not contain whitespace",
            })
        );
    }

    #[test]
    fn from_word_uses_the_given_workflow() {
        let wf = Workflow::new(vec![status("todo"), status("doing")]);
        assert_eq!(
            Filter::from_word("doing", &wf),
            Ok(Filter::new().status(status("doing")))
        );
        // `ready` is just a tag in this workflow.
        assert_eq!(
            Filter::from_word("ready", &wf),
            Ok(Filter::new().tag(tag("ready")))
        );
    }

    // -- Ordering -------------------------------------------------------------

    #[test]
    fn priority_orders_first() {
        let tasks = vec![
            task("1", "c", None, Priority::C, Some("2026-01-01"), &[]),
            task("2", "b", None, Priority::B, Some("2026-01-02"), &[]),
            task("3", "a", None, Priority::A, None, &[]),
        ];
        assert_eq!(ids(&sort(&tasks)), ["3", "2", "1"]);
    }

    #[test]
    fn earlier_due_first_and_missing_due_last() {
        let tasks = vec![
            task("1", "none", None, Priority::B, None, &[]),
            task("2", "late", None, Priority::B, Some("2026-12-31"), &[]),
            task("3", "early", None, Priority::B, Some("2026-01-01"), &[]),
            task("4", "none2", None, Priority::B, None, &[]),
        ];
        assert_eq!(ids(&sort(&tasks)), ["3", "2", "1", "4"]);
    }

    #[test]
    fn id_breaks_ties_numerically_then_text() {
        let tasks = vec![
            task("10", "", None, Priority::B, None, &[]),
            task("b", "", None, Priority::B, None, &[]),
            task("9", "", None, Priority::B, None, &[]),
            task("a", "", None, Priority::B, None, &[]),
            task("007", "", None, Priority::B, None, &[]),
            task("7", "", None, Priority::B, None, &[]),
            task("1a", "", None, Priority::B, None, &[]),
            task("+8", "", None, Priority::B, None, &[]),
        ];
        assert_eq!(
            ids(&sort(&tasks)),
            ["007", "7", "9", "10", "+8", "1a", "a", "b"]
        );
    }

    #[test]
    fn compare_ids_table() {
        use Ordering::{Equal, Greater, Less};
        let cases = [
            ("9", "10", Less),
            ("10", "9", Greater),
            ("7", "7", Equal),
            ("007", "7", Less),
            ("7", "007", Greater),
            ("10", "a", Less), // numeric before text
            ("a", "10", Greater),
            ("a", "b", Less),
            ("1a", "10", Greater),                  // text, after all numbers
            ("18446744073709551616", "1", Greater), // overflows u64: text
            ("0", "18446744073709551615", Less),
        ];
        for (a, b, expected) in cases {
            let a = TaskId::new(a).unwrap();
            let b = TaskId::new(b).unwrap();
            assert_eq!(compare_ids(&a, &b), expected, "{a} vs {b}");
            assert_eq!(compare_ids(&b, &a), expected.reverse(), "{b} vs {a}");
        }
    }

    #[test]
    fn id_key_classifies_digit_strings_only() {
        let id = TaskId::new("23").unwrap();
        assert_eq!(IdKey::of(&id), IdKey::Number(23, "23"));
        let id = TaskId::new("+23").unwrap();
        assert_eq!(IdKey::of(&id), IdKey::Text("+23"));
        let id = TaskId::new("99999999999999999999").unwrap();
        assert_eq!(IdKey::of(&id), IdKey::Text("99999999999999999999"));
    }

    #[test]
    fn sort_key_and_compare_agree_with_sort() {
        let tasks = fixture();
        let sorted = sort(&tasks);
        assert_eq!(ids(&sorted), ["5", "3", "6", "10", "1", "2", "4"]);
        for pair in sorted.windows(2) {
            assert!(sort_key(pair[0]) < sort_key(pair[1]));
            assert_eq!(compare(pair[0], pair[1]), Ordering::Less);
            assert_eq!(compare(pair[1], pair[0]), Ordering::Greater);
        }
        assert_eq!(compare(&tasks[0], &tasks[0]), Ordering::Equal);
        assert_eq!(sort_key(&tasks[0]), sort_key(&tasks[0]));
    }

    #[test]
    fn sort_key_fields() {
        let t = task("5", "", None, Priority::A, Some("2026-10-05"), &[]);
        assert_eq!(
            sort_key(&t),
            SortKey {
                priority: Priority::A,
                due: DueKey::On(date("2026-10-05")),
                id: IdKey::Number(5, "5"),
            }
        );
        let t = task("x", "", None, Priority::C, None, &[]);
        assert_eq!(
            sort_key(&t),
            SortKey {
                priority: Priority::C,
                due: DueKey::Never,
                id: IdKey::Text("x"),
            }
        );
        assert!(DueKey::On(date("9999-12-31")) < DueKey::Never);
    }

    #[test]
    fn sort_of_nothing_is_nothing() {
        let tasks: Vec<Task> = Vec::new();
        assert_eq!(sort(&tasks), Vec::<&Task>::new());
    }

    // -- Grouping -------------------------------------------------------------

    #[test]
    fn groups_follow_workflow_order_omit_empty_and_put_no_status_last() {
        let tasks = fixture();
        let open = filter(&tasks, &Filter::default());
        let groups = group_by_status(open, &Workflow::default());
        let shape: Vec<(Option<&str>, Vec<&str>)> = groups
            .iter()
            .map(|g| (g.status.as_ref().map(Status::as_str), ids(&g.tasks)))
            .collect();
        assert_eq!(
            shape,
            [
                (Some("in-progress"), vec!["10", "2"]),
                (Some("ready"), vec!["1"]),
                (Some("waiting"), vec!["5"]),
                // blocked omitted: empty
                (Some("later"), vec!["3"]),
                (None, vec!["4"]),
            ]
        );
    }

    #[test]
    fn each_group_is_sorted() {
        let tasks = vec![
            task("3", "", Some("ready"), Priority::B, None, &[]),
            task("1", "", Some("ready"), Priority::C, None, &[]),
            task("2", "", Some("ready"), Priority::B, Some("2026-01-01"), &[]),
            task("10", "", Some("ready"), Priority::B, None, &[]),
        ];
        let groups = group_by_status(&tasks, &Workflow::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, Some(Status::READY));
        assert_eq!(ids(&groups[0].tasks), ["2", "3", "10", "1"]);
    }

    #[test]
    fn grouping_does_not_filter_done_tasks_but_list_does() {
        let tasks = fixture();
        let wf = Workflow::default();
        let groups = group_by_status(&tasks, &wf);
        let last = groups.last().unwrap();
        assert_eq!(last.status, None);
        assert_eq!(ids(&last.tasks), ["6", "4"]);

        let groups = list(&tasks, &Filter::default(), &wf);
        assert_eq!(ids(&groups.last().unwrap().tasks), ["4"]);

        let groups = list(&tasks, &Filter::new().tag(tag("ci")), &wf);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, Some(Status::IN_PROGRESS));
        assert_eq!(ids(&groups[0].tasks), ["10", "2"]);
    }

    #[test]
    fn grouping_empty_input_gives_no_groups() {
        let tasks: Vec<Task> = Vec::new();
        assert_eq!(group_by_status(&tasks, &Workflow::default()), Vec::new());
    }

    #[test]
    fn statuses_outside_the_workflow_get_trailing_groups_by_name() {
        let wf = Workflow::new(vec![status("todo"), status("doing")]);
        let tasks = vec![
            task("1", "", Some("zeta"), Priority::B, None, &[]),
            task("2", "", None, Priority::B, None, &[]),
            task("3", "", Some("doing"), Priority::B, None, &[]),
            task("4", "", Some("alpha"), Priority::B, None, &[]),
            task("5", "", Some("zeta"), Priority::A, None, &[]),
            task("6", "", Some("todo"), Priority::B, None, &[]),
        ];
        let groups = group_by_status(&tasks, &wf);
        let shape: Vec<(Option<&str>, Vec<&str>)> = groups
            .iter()
            .map(|g| (g.status.as_ref().map(Status::as_str), ids(&g.tasks)))
            .collect();
        assert_eq!(
            shape,
            [
                (Some("todo"), vec!["6"]),
                (Some("doing"), vec!["3"]),
                (Some("alpha"), vec!["4"]),
                (Some("zeta"), vec!["5", "1"]),
                (None, vec!["2"]),
            ]
        );
    }

    // -- Next -----------------------------------------------------------------

    #[test]
    fn next_prefers_in_progress_then_ready() {
        let wf = Workflow::default();
        let tasks = fixture();
        // Two in-progress tasks: priority A wins.
        assert_eq!(next(&tasks, &wf).unwrap().id.as_str(), "10");

        // No in-progress: fall through to ready.
        let tasks: Vec<Task> = tasks
            .into_iter()
            .filter(|t| t.status != Some(Status::IN_PROGRESS))
            .collect();
        assert_eq!(next(&tasks, &wf).unwrap().id.as_str(), "1");

        // Waiting/later/no-status do not count, however urgent.
        let tasks: Vec<Task> = tasks
            .into_iter()
            .filter(|t| t.status != Some(Status::READY))
            .collect();
        assert_eq!(next(&tasks, &wf), None);
    }

    #[test]
    fn next_returns_none_for_no_tasks() {
        let tasks: Vec<Task> = Vec::new();
        assert_eq!(next(&tasks, &Workflow::default()), None);
        assert_eq!(next_from(&tasks, &[Status::READY]), None);
    }

    #[test]
    fn next_picks_the_sorted_head_of_the_group() {
        let wf = Workflow::default();
        let tasks = vec![
            task("3", "", Some("ready"), Priority::B, None, &[]),
            task(
                "10",
                "",
                Some("ready"),
                Priority::B,
                Some("2026-02-01"),
                &[],
            ),
            task("9", "", Some("ready"), Priority::B, Some("2026-02-01"), &[]),
            task("1", "", Some("ready"), Priority::C, Some("2026-01-01"), &[]),
        ];
        assert_eq!(next(&tasks, &wf).unwrap().id.as_str(), "9");
    }

    #[test]
    fn next_from_takes_statuses_in_the_given_order() {
        let tasks = vec![
            task("1", "", Some("later"), Priority::A, None, &[]),
            task("2", "", Some("waiting"), Priority::C, None, &[]),
            task("3", "", Some("ready"), Priority::B, None, &[]),
        ];
        let pick = |statuses: &[Status]| next_from(&tasks, statuses).map(|t| t.id.as_str());
        assert_eq!(pick(&[Status::WAITING, Status::LATER]), Some("2"));
        assert_eq!(pick(&[Status::LATER, Status::WAITING]), Some("1"));
        assert_eq!(pick(&[Status::BLOCKED, Status::READY]), Some("3"));
        assert_eq!(pick(&[Status::BLOCKED, Status::IN_PROGRESS]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn next_uses_the_first_two_statuses_of_any_workflow() {
        assert_eq!(NEXT_STATUSES, 2);
        let wf = Workflow::new(vec![status("doing"), status("todo"), status("someday")]);
        let tasks = vec![
            task("1", "", Some("someday"), Priority::A, None, &[]),
            task("2", "", Some("todo"), Priority::C, None, &[]),
        ];
        assert_eq!(next(&tasks, &wf).unwrap().id.as_str(), "2");

        // A workflow shorter than NEXT_STATUSES must not panic.
        let wf = Workflow::new(vec![status("todo")]);
        assert_eq!(next(&tasks, &wf).unwrap().id.as_str(), "2");
        let wf = Workflow::new(Vec::new());
        assert_eq!(next(&tasks, &wf), None);
    }
}
