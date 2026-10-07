//! The task aggregate and its parts.

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use super::{Priority, Status, Tag, TaskId};
use crate::clock::{Clock, When, to_minute};

/// A URL with an optional human label (`- [label](url)` or `- url` in the file).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    /// The target.
    pub url: String,
    /// Link text, when the file had one.
    pub label: Option<String>,
}

impl Link {
    /// A bare link.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            label: None,
        }
    }

    /// A labelled link (`[label](url)`).
    pub fn labelled(url: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            label: Some(label.into()),
        }
    }
}

/// A git worktree where the task is being developed (`- /path (`branch`)`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Worktree {
    /// Absolute path of the checkout.
    pub path: PathBuf,
    /// Branch it was created on, when recorded.
    pub branch: Option<String>,
}

impl Worktree {
    /// A worktree with no recorded branch.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            branch: None,
        }
    }

    /// A worktree on `branch`.
    pub fn on_branch(path: impl Into<PathBuf>, branch: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            branch: Some(branch.into()),
        }
    }
}

/// A recorded agent session on the task (`- 2026-10-04 10:15: `id` — desc`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Session {
    /// When it was recorded (local time, see [`crate::clock`]). Serialises
    /// as `YYYY-MM-DD HH:MM`, like a progress entry's `at`.
    #[serde(with = "crate::clock::timestamp_serde")]
    pub at: NaiveDateTime,
    /// The launcher's session id (what `claude --resume` takes).
    pub id: String,
    /// Which launcher ran it (`claude`, `shell`, ...). Files written by the
    /// script carry none; `None` means "unknown, probably Claude".
    pub launcher: Option<String>,
    /// Free-text description.
    pub description: Option<String>,
}

impl Session {
    /// A session recorded at `at` (truncated to the minute, the precision
    /// the file keeps) with no launcher or description.
    pub fn new(at: NaiveDateTime, id: impl Into<String>) -> Self {
        Self {
            at: crate::clock::to_minute(at),
            id: id.into(),
            launcher: None,
            description: None,
        }
    }
}

/// One dated progress note (`- 2026-10-04 10:15: note`).
///
/// `at` is a [`When`] rather than a [`NaiveDateTime`] because entries written
/// before the script logged times have only a date, and the file must be
/// rewritten byte-for-byte. New entries made through [`Task::log`] always
/// carry a time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProgressEntry {
    /// When the note was logged (local time).
    pub at: When,
    /// The note.
    pub note: String,
}

impl ProgressEntry {
    /// An entry with a full timestamp, truncated to the minute (the
    /// precision the file keeps, so a logged entry round-trips unchanged).
    pub fn new(at: NaiveDateTime, note: impl Into<String>) -> Self {
        Self {
            at: When::from(at),
            note: note.into(),
        }
    }

    /// A legacy date-only entry.
    pub fn dated(at: NaiveDate, note: impl Into<String>) -> Self {
        Self {
            at: When::Date(at),
            note: note.into(),
        }
    }
}

/// Where a synced task came from, for reconciliation (`## Source` section).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Origin {
    /// Source name as configured (`gitlab-review-requests`, ...).
    pub source: String,
    /// Identifier within that source (MR iid, ticket id, ...).
    pub external_id: String,
    /// Canonical URL, when the source has one.
    pub url: Option<String>,
}

/// A task: one nb todo file.
///
/// Status and priority are fields, not tags; `tags` holds topic tags only.
/// Mutating methods keep the simple invariants (a done task has no status;
/// worktrees, sessions and merge requests are unique by path, id and url).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Store-local id.
    pub id: TaskId,
    /// Title line, without the `# [ ] ` prefix.
    pub title: String,
    /// `# [x]` in the file.
    pub done: bool,
    /// Workflow status; `None` for done tasks and for open tasks with no status tag.
    pub status: Option<Status>,
    /// Priority; `B` when the file has no priority tag.
    pub priority: Priority,
    /// `## Due`.
    pub due: Option<NaiveDate>,
    /// `## Description`, verbatim.
    pub description: Option<String>,
    /// `## Project`: directory sessions start in when no worktree exists.
    pub project: Option<PathBuf>,
    /// Topic tags (never status or priority).
    pub tags: Vec<Tag>,
    /// `## Related` links.
    pub related: Vec<Link>,
    /// `### Merge requests` under `## Related`.
    pub merge_requests: Vec<Link>,
    /// `## Worktrees`.
    pub worktrees: Vec<Worktree>,
    /// `## Sessions`.
    pub sessions: Vec<Session>,
    /// `## Progress`, oldest first.
    pub progress: Vec<ProgressEntry>,
    /// `## Source`, for synced tasks.
    pub origin: Option<Origin>,
    /// `## Closed`: when the task was closed, to the minute (local time).
    /// `None` on open tasks and on tasks closed without it (`nb todo do`,
    /// the original script, tasq before ADR 0020). Absent from older JSON.
    #[serde(default, with = "crate::clock::timestamp_serde::option")]
    pub closed_at: Option<NaiveDateTime>,
}

impl Task {
    /// A blank open task with the given id and title: priority `B`, no
    /// status, nothing else set. Mostly for tests and stores; the CLI
    /// creates tasks through [`TaskDraft`].
    pub fn new(id: TaskId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            done: false,
            status: None,
            priority: Priority::default(),
            due: None,
            description: None,
            project: None,
            tags: Vec::new(),
            related: Vec::new(),
            merge_requests: Vec::new(),
            worktrees: Vec::new(),
            sessions: Vec::new(),
            progress: Vec::new(),
            origin: None,
            closed_at: None,
        }
    }

    /// Appends a progress note stamped with `clock.now()` (minute precision
    /// is applied when written, not here).
    pub fn log(&mut self, note: impl Into<String>, clock: &dyn Clock) {
        self.progress.push(ProgressEntry::new(clock.now(), note));
    }

    /// The most recent progress note, if any.
    pub fn latest_progress(&self) -> Option<&ProgressEntry> {
        self.progress.last()
    }

    /// Sets the status. Does not reopen a done task; callers that want that
    /// set `done = false` explicitly.
    pub fn set_status(&mut self, status: Status) {
        self.status = Some(status);
    }

    /// Removes the status (the task becomes "no-status").
    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// Sets the priority.
    pub fn set_priority(&mut self, priority: Priority) {
        self.priority = priority;
    }

    /// Marks the task done and drops its status, as `tasks done` did: a
    /// closed todo that kept `#ready` would misdescribe itself in every view.
    pub fn mark_done(&mut self) {
        self.done = true;
        self.status = None;
    }

    /// [`mark_done`](Self::mark_done) and stamps `closed_at` with
    /// `clock.now()` to the minute. A task already done keeps its
    /// `closed_at`: closing it again does not move it in the DONE list.
    pub fn close(&mut self, clock: &dyn Clock) {
        if !self.done {
            self.closed_at = Some(to_minute(clock.now()));
        }
        self.mark_done();
    }

    /// Whether the task carries topic tag `tag`.
    pub fn has_tag(&self, tag: &Tag) -> bool {
        self.tags.contains(tag)
    }

    /// Adds a topic tag unless already present. Returns whether it was added.
    pub fn add_tag(&mut self, tag: Tag) -> bool {
        push_unique(&mut self.tags, tag, |a, b| a == b)
    }

    /// Removes a topic tag. Returns whether it was present.
    pub fn remove_tag(&mut self, tag: &Tag) -> bool {
        let before = self.tags.len();
        self.tags.retain(|t| t != tag);
        self.tags.len() != before
    }

    /// Adds a related link unless one with the same url exists.
    pub fn add_related(&mut self, link: Link) -> bool {
        push_unique(&mut self.related, link, |a, b| a.url == b.url)
    }

    /// Adds a merge request unless one with the same url exists.
    pub fn add_merge_request(&mut self, link: Link) -> bool {
        push_unique(&mut self.merge_requests, link, |a, b| a.url == b.url)
    }

    /// Adds a worktree unless one with the same path is tracked.
    pub fn add_worktree(&mut self, worktree: Worktree) -> bool {
        push_unique(&mut self.worktrees, worktree, |a, b| a.path == b.path)
    }

    /// Adds a session unless one with the same id is tracked.
    pub fn add_session(&mut self, session: Session) -> bool {
        push_unique(&mut self.sessions, session, |a, b| a.id == b.id)
    }
}

fn push_unique<T>(items: &mut Vec<T>, item: T, same: impl Fn(&T, &T) -> bool) -> bool {
    if items.iter().any(|existing| same(existing, &item)) {
        return false;
    }
    items.push(item);
    true
}

/// What is needed to create a task; everything but the title has a default.
///
/// Defaults mirror `tasks create`: status `ready`, priority `B`, open.
/// Build it with the `with_*` methods, then hand it to a store, which
/// assigns the id and calls [`TaskDraft::into_task`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDraft {
    /// Title line.
    pub title: String,
    /// Initial status; `Some(ready)` by default. Ignored when `done`.
    pub status: Option<Status>,
    /// Initial priority; `B` by default.
    pub priority: Priority,
    /// Create the task already closed (`tasks create --status done`).
    pub done: bool,
    /// `## Due`.
    pub due: Option<NaiveDate>,
    /// `## Description`.
    pub description: Option<String>,
    /// `## Project`.
    pub project: Option<PathBuf>,
    /// Topic tags.
    pub tags: Vec<Tag>,
    /// `## Related` links.
    pub related: Vec<Link>,
    /// Merge requests.
    pub merge_requests: Vec<Link>,
    /// First progress note. `None` leaves `progress` empty; the CLI supplies
    /// its own default text (`created via tasks create`).
    pub note: Option<String>,
    /// Origin for synced tasks.
    pub origin: Option<Origin>,
}

impl TaskDraft {
    /// A draft with the script's defaults: `ready`, `B`, open.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status: Some(Status::READY),
            priority: Priority::default(),
            done: false,
            due: None,
            description: None,
            project: None,
            tags: Vec::new(),
            related: Vec::new(),
            merge_requests: Vec::new(),
            note: None,
            origin: None,
        }
    }

    /// Sets the initial status (`None` for no status tag).
    #[must_use]
    pub fn with_status(mut self, status: Option<Status>) -> Self {
        self.status = status;
        self
    }

    /// Sets the priority.
    #[must_use]
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Creates the task already done (no status).
    #[must_use]
    pub fn with_done(mut self, done: bool) -> Self {
        self.done = done;
        self
    }

    /// Sets the due date.
    #[must_use]
    pub fn with_due(mut self, due: NaiveDate) -> Self {
        self.due = Some(due);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the project directory.
    #[must_use]
    pub fn with_project(mut self, project: impl Into<PathBuf>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Adds a topic tag (duplicates are dropped).
    #[must_use]
    pub fn with_tag(mut self, tag: Tag) -> Self {
        push_unique(&mut self.tags, tag, |a, b| a == b);
        self
    }

    /// Adds a related link (duplicate urls are dropped).
    #[must_use]
    pub fn with_related(mut self, link: Link) -> Self {
        push_unique(&mut self.related, link, |a, b| a.url == b.url);
        self
    }

    /// Adds a merge request (duplicate urls are dropped).
    #[must_use]
    pub fn with_merge_request(mut self, link: Link) -> Self {
        push_unique(&mut self.merge_requests, link, |a, b| a.url == b.url);
        self
    }

    /// Sets the first progress note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Sets the origin.
    #[must_use]
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Materialises the task once the store has assigned `id`.
    ///
    /// A `done` draft gets no status (same shape `tasks done` leaves) and
    /// is closed at `clock.now()`. The note, when present, becomes the
    /// first progress entry stamped at `clock.now()`.
    pub fn into_task(self, id: TaskId, clock: &dyn Clock) -> Task {
        let mut task = Task {
            id,
            title: self.title,
            done: self.done,
            status: if self.done { None } else { self.status },
            priority: self.priority,
            due: self.due,
            description: self.description,
            project: self.project,
            tags: self.tags,
            related: self.related,
            merge_requests: self.merge_requests,
            worktrees: Vec::new(),
            sessions: Vec::new(),
            progress: Vec::new(),
            origin: self.origin,
            closed_at: self.done.then(|| to_minute(clock.now())),
        };
        if let Some(note) = self.note {
            task.log(note, clock);
        }
        task
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::at("2026-10-04 10:15")
    }

    fn tag(s: &str) -> Tag {
        Tag::new(s).unwrap()
    }

    fn task() -> Task {
        Task::new(TaskId::from(1), "Title")
    }

    fn session(id: &str) -> Session {
        Session {
            at: clock().now(),
            id: id.into(),
            launcher: None,
            description: None,
        }
    }

    #[test]
    fn new_task_is_open_unprioritised_and_empty() {
        let t = task();
        assert_eq!(t.id, TaskId::from(1));
        assert_eq!(t.title, "Title");
        assert!(!t.done);
        assert_eq!(t.status, None);
        assert_eq!(t.priority, Priority::B);
        assert_eq!(t.latest_progress(), None);
        assert_eq!(
            (t.due, t.description, t.project, t.origin),
            (None, None, None, None)
        );
        assert!(t.tags.is_empty() && t.related.is_empty() && t.merge_requests.is_empty());
        assert!(t.worktrees.is_empty() && t.sessions.is_empty() && t.progress.is_empty());
    }

    #[test]
    fn log_appends_with_the_clock_time() {
        let mut t = task();
        t.log("first", &clock());
        t.log("second", &FixedClock::at("2026-10-05 09:00"));
        assert_eq!(
            t.progress,
            vec![
                ProgressEntry::new(clock().now(), "first"),
                ProgressEntry::new(FixedClock::at("2026-10-05 09:00").now(), "second"),
            ]
        );
        assert_eq!(t.latest_progress().unwrap().note, "second");
        assert_eq!(t.progress[0].at, When::DateTime(clock().now()));
    }

    #[test]
    fn progress_entry_constructors() {
        let day = clock().today();
        assert_eq!(
            ProgressEntry::dated(day, "old"),
            ProgressEntry {
                at: When::Date(day),
                note: "old".into()
            }
        );
        assert_eq!(
            ProgressEntry::new(clock().now(), "new"),
            ProgressEntry {
                at: When::DateTime(clock().now()),
                note: "new".into()
            }
        );
    }

    #[test]
    fn link_and_worktree_constructors() {
        assert_eq!(
            Link::new("u"),
            Link {
                url: "u".into(),
                label: None
            }
        );
        assert_eq!(
            Link::labelled("u", "l"),
            Link {
                url: "u".into(),
                label: Some("l".into())
            }
        );
        assert_eq!(
            Worktree::new("/p"),
            Worktree {
                path: "/p".into(),
                branch: None
            }
        );
        assert_eq!(
            Worktree::on_branch("/p", "b"),
            Worktree {
                path: "/p".into(),
                branch: Some("b".into())
            }
        );
    }

    #[test]
    fn set_and_clear_status() {
        let mut t = task();
        t.set_status(Status::READY);
        assert_eq!(t.status, Some(Status::READY));
        t.set_status(Status::IN_PROGRESS);
        assert_eq!(t.status, Some(Status::IN_PROGRESS));
        assert!(!t.done, "set_status must not touch done");
        t.clear_status();
        assert_eq!(t.status, None);
    }

    #[test]
    fn set_priority() {
        let mut t = task();
        t.set_priority(Priority::A);
        assert_eq!(t.priority, Priority::A);
        t.set_priority(Priority::C);
        assert_eq!(t.priority, Priority::C);
    }

    #[test]
    fn mark_done_sets_done_and_clears_status_but_keeps_priority() {
        let mut t = task();
        t.set_status(Status::READY);
        t.set_priority(Priority::A);
        t.mark_done();
        assert!(t.done);
        assert_eq!(t.status, None);
        assert_eq!(t.priority, Priority::A);
        assert_eq!(t.closed_at, None, "mark_done has no clock");
    }

    #[test]
    fn close_stamps_closed_at_once() {
        let mut t = task();
        t.set_status(Status::READY);
        let at = FixedClock::at("2026-10-07 14:32").0;
        t.close(&FixedClock(at + chrono::Duration::seconds(42)));
        assert!(t.done);
        assert_eq!(t.status, None);
        assert_eq!(t.closed_at, Some(at));
        // Closing a done task again keeps the first time.
        t.close(&FixedClock::at("2026-10-08 09:00"));
        assert_eq!(t.closed_at, Some(at));
    }

    #[test]
    fn tags_are_a_set() {
        let mut t = task();
        assert!(!t.has_tag(&tag("gitlab")));
        assert!(t.add_tag(tag("gitlab")));
        assert!(!t.add_tag(tag("gitlab")));
        assert!(t.add_tag(tag("support")));
        assert!(t.has_tag(&tag("gitlab")));
        assert!(t.has_tag(&tag("support")));
        assert!(!t.has_tag(&tag("Gitlab")));
        assert_eq!(t.tags, vec![tag("gitlab"), tag("support")]);
        assert!(t.remove_tag(&tag("gitlab")));
        assert!(!t.remove_tag(&tag("gitlab")));
        assert_eq!(t.tags, vec![tag("support")]);
    }

    #[test]
    fn related_and_merge_requests_dedupe_by_url_only() {
        let mut t = task();
        assert!(t.add_related(Link::new("https://a")));
        assert!(
            !t.add_related(Link::labelled("https://a", "A")),
            "same url, different label"
        );
        assert!(t.add_related(Link::new("https://b")));
        assert_eq!(
            t.related,
            vec![Link::new("https://a"), Link::new("https://b")]
        );

        assert!(t.add_merge_request(Link::labelled("https://mr/1", "MR 1")));
        assert!(!t.add_merge_request(Link::new("https://mr/1")));
        assert!(t.add_merge_request(Link::labelled("https://mr/2", "MR 2")));
        assert_eq!(t.merge_requests.len(), 2);
        assert_eq!(t.merge_requests[0].label.as_deref(), Some("MR 1"));
        assert_eq!(
            t.related.len(),
            2,
            "merge requests do not leak into related"
        );
    }

    #[test]
    fn worktrees_dedupe_by_path_only() {
        let mut t = task();
        assert!(t.add_worktree(Worktree::new("/w/a")));
        assert!(
            !t.add_worktree(Worktree::on_branch("/w/a", "feat")),
            "same path, different branch"
        );
        assert!(t.add_worktree(Worktree::on_branch("/w/b", "feat")));
        assert_eq!(
            t.worktrees,
            vec![Worktree::new("/w/a"), Worktree::on_branch("/w/b", "feat")]
        );
    }

    #[test]
    fn sessions_dedupe_by_id_only() {
        let mut t = task();
        assert!(t.add_session(session("s1")));
        let mut again = session("s1");
        again.description = Some("different".into());
        assert!(!t.add_session(again));
        assert!(t.add_session(session("s2")));
        assert_eq!(t.sessions.len(), 2);
        assert_eq!(t.sessions[0].description, None);
        assert_eq!(t.sessions[1].id, "s2");
    }

    #[test]
    fn draft_defaults_match_the_script() {
        let d = TaskDraft::new("T");
        assert_eq!(d.title, "T");
        assert_eq!(d.status, Some(Status::READY));
        assert_eq!(d.priority, Priority::B);
        assert!(!d.done);
        assert_eq!(
            (d.due, d.description, d.project, d.note, d.origin),
            (None, None, None, None, None)
        );
        assert!(d.tags.is_empty() && d.related.is_empty() && d.merge_requests.is_empty());
    }

    #[test]
    fn draft_builders_set_every_field() {
        let origin = Origin {
            source: "gitlab".into(),
            external_id: "12".into(),
            url: Some("https://g/12".into()),
        };
        let d = TaskDraft::new("T")
            .with_status(Some(Status::WAITING))
            .with_priority(Priority::A)
            .with_done(false)
            .with_due(clock().today())
            .with_description("desc")
            .with_project("/proj")
            .with_tag(tag("gitlab"))
            .with_tag(tag("gitlab"))
            .with_tag(tag("support"))
            .with_related(Link::new("https://a"))
            .with_related(Link::labelled("https://a", "dup"))
            .with_merge_request(Link::labelled("https://mr/1", "MR"))
            .with_merge_request(Link::new("https://mr/1"))
            .with_note("created")
            .with_origin(origin.clone());
        assert_eq!(d.status, Some(Status::WAITING));
        assert_eq!(d.priority, Priority::A);
        assert!(!d.done);
        assert_eq!(d.due, Some(clock().today()));
        assert_eq!(d.description.as_deref(), Some("desc"));
        assert_eq!(d.project, Some(PathBuf::from("/proj")));
        assert_eq!(d.tags, vec![tag("gitlab"), tag("support")]);
        assert_eq!(d.related, vec![Link::new("https://a")]);
        assert_eq!(d.merge_requests, vec![Link::labelled("https://mr/1", "MR")]);
        assert_eq!(d.note.as_deref(), Some("created"));
        assert_eq!(d.origin, Some(origin));
        assert_eq!(TaskDraft::new("T").with_status(None).status, None);
        assert!(TaskDraft::new("T").with_done(true).done);
    }

    #[test]
    fn into_task_copies_fields_and_logs_the_note() {
        let origin = Origin {
            source: "gitlab".into(),
            external_id: "12".into(),
            url: None,
        };
        let d = TaskDraft::new("T")
            .with_status(Some(Status::IN_PROGRESS))
            .with_priority(Priority::C)
            .with_due(clock().today())
            .with_description("desc")
            .with_project("/proj")
            .with_tag(tag("x"))
            .with_related(Link::new("https://a"))
            .with_merge_request(Link::new("https://mr/1"))
            .with_note("created")
            .with_origin(origin.clone());
        let t = d.into_task(TaskId::from(5), &clock());
        assert_eq!(t.id, TaskId::from(5));
        assert_eq!(t.title, "T");
        assert!(!t.done);
        assert_eq!(t.status, Some(Status::IN_PROGRESS));
        assert_eq!(t.priority, Priority::C);
        assert_eq!(t.due, Some(clock().today()));
        assert_eq!(t.description.as_deref(), Some("desc"));
        assert_eq!(t.project, Some(PathBuf::from("/proj")));
        assert_eq!(t.tags, vec![tag("x")]);
        assert_eq!(t.related, vec![Link::new("https://a")]);
        assert_eq!(t.merge_requests, vec![Link::new("https://mr/1")]);
        assert!(t.worktrees.is_empty() && t.sessions.is_empty());
        assert_eq!(
            t.progress,
            vec![ProgressEntry::new(clock().now(), "created")]
        );
        assert_eq!(t.origin, Some(origin));
    }

    #[test]
    fn into_task_without_note_has_no_progress() {
        let t = TaskDraft::new("T").into_task(TaskId::from(1), &clock());
        assert_eq!(t.progress, Vec::new());
        assert_eq!(t.status, Some(Status::READY));
    }

    #[test]
    fn into_task_done_draft_drops_status() {
        let t = TaskDraft::new("T")
            .with_done(true)
            .into_task(TaskId::from(1), &clock());
        assert!(t.done);
        assert_eq!(t.status, None);
        assert_eq!(t.closed_at, Some(clock().now()));
        let open = TaskDraft::new("T")
            .with_done(false)
            .into_task(TaskId::from(1), &clock());
        assert!(!open.done);
        assert_eq!(open.status, Some(Status::READY));
        assert_eq!(open.closed_at, None);
    }

    #[test]
    fn task_serde_round_trip() {
        let mut t = task();
        t.set_status(Status::READY);
        t.add_tag(tag("gitlab"));
        t.log("note", &clock());
        t.progress
            .push(ProgressEntry::dated(clock().today(), "legacy"));
        t.add_session(session("s1"));
        t.add_worktree(Worktree::on_branch("/w", "b"));
        t.add_merge_request(Link::labelled("https://mr/1", "MR"));
        t.origin = Some(Origin {
            source: "gitlab".into(),
            external_id: "1".into(),
            url: None,
        });
        t.closed_at = Some(clock().now());
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"status\":\"ready\""), "{json}");
        assert!(
            json.ends_with(",\"closed_at\":\"2026-10-04 10:15\"}"),
            "{json}"
        );
        assert!(json.contains("\"tags\":[\"gitlab\"]"), "{json}");
        assert!(json.contains("\"at\":\"2026-10-04 10:15\""), "{json}");
        assert!(json.contains("\"at\":\"2026-10-04\""), "{json}");
        assert!(
            !json.contains("T10:"),
            "session timestamps use the file shape: {json}"
        );
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // JSON written before `closed_at` existed still reads.
        let older = json.replace(",\"closed_at\":\"2026-10-04 10:15\"", "");
        let back: Task = serde_json::from_str(&older).unwrap();
        assert_eq!(back.closed_at, None);
        let draft = TaskDraft::new("T").with_tag(tag("x"));
        let back: TaskDraft =
            serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
        assert_eq!(back, draft);
    }

    #[test]
    fn clock_stamps_are_truncated_to_the_minute() {
        let base = clock().now();
        let seconds = FixedClock(base + chrono::Duration::seconds(42));
        let mut t = Task::new(TaskId::from(1), "T");
        t.log("note", &seconds);
        assert_eq!(t.progress[0].at, When::DateTime(base));
        assert_eq!(
            ProgressEntry::new(seconds.now(), "n").at,
            When::DateTime(base)
        );
        let s = Session::new(seconds.now(), "sid");
        assert_eq!(s.at, base);
        assert_eq!(s.id, "sid");
        assert_eq!((s.launcher, s.description), (None, None));
        let done = TaskDraft::new("T")
            .with_done(true)
            .into_task(TaskId::from(1), &seconds);
        assert_eq!(done.closed_at, Some(base));
    }
}
