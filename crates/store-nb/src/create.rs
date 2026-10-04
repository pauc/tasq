//! The pure parts of [`Store::create`](tasq_core::store::Store::create) for the nb store: nb's
//! filename rule and the checks on a draft (plan T-203).
//!
//! nb names a todo `YYYYMMDDHHMMSS.todo.md` after the local creation time
//! and, when that name is taken, moves to the next second until it finds a
//! free one. The store reproduces the rule so the files it creates look like
//! nb's own and sort by creation time.

use chrono::{Duration, NaiveDateTime};
use tasq_core::model::TaskDraft;
use tasq_core::store::StoreError;

use crate::index::TODO_SUFFIX;

/// How many consecutive seconds are tried before giving up on a free name.
pub const MAX_ATTEMPTS: u32 = 60;

/// The progress note a draft without one gets. The script wrote
/// `created via tasks create`; the binary is `tasq`.
pub const DEFAULT_NOTE: &str = "created via tasq create";

/// nb's filename for a todo created at `at`: `YYYYMMDDHHMMSS.todo.md`.
pub fn file_name_at(at: NaiveDateTime) -> String {
    format!("{}{TODO_SUFFIX}", at.format("%Y%m%d%H%M%S"))
}

/// The first of `file_name_at(now)`, `file_name_at(now + 1s)`, ... for
/// which `taken` is false, trying [`MAX_ATTEMPTS`] seconds. `None` when
/// every candidate is taken.
pub fn free_file_name(now: NaiveDateTime, taken: impl Fn(&str) -> bool) -> Option<String> {
    (0..MAX_ATTEMPTS)
        .map(|offset| file_name_at(now + Duration::seconds(i64::from(offset))))
        .find(|name| !taken(name))
}

/// Rejects a draft the store cannot write faithfully. Today: merge requests
/// without a label. The file format for a merge request is `- [title](url)`,
/// and the title is the CLI's to resolve (plan T-307); writing the url as
/// its own title would persist a placeholder nb users would have to fix.
pub fn check_draft(draft: &TaskDraft) -> Result<(), StoreError> {
    if let Some(mr) = draft
        .merge_requests
        .iter()
        .find(|mr| mr.label.as_deref().is_none_or(str::is_empty))
    {
        return Err(StoreError::Unsupported {
            operation: format!(
                "creating a task with the merge request {} without a title (resolve the title first)",
                mr.url
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;
    use tasq_core::model::Link;

    fn at(s: &str) -> NaiveDateTime {
        FixedClock::at(s).0
    }

    #[test]
    fn file_name_is_the_second_stamp() {
        assert_eq!(
            file_name_at(at("2026-10-04 12:34")),
            "20261004123400.todo.md"
        );
        let with_seconds = at("2026-01-02 03:04") + Duration::seconds(5);
        assert_eq!(file_name_at(with_seconds), "20260102030405.todo.md");
    }

    #[test]
    fn free_name_bumps_by_one_second_until_free() {
        let now = at("2026-10-04 12:34");
        assert_eq!(
            free_file_name(now, |_| false).unwrap(),
            "20261004123400.todo.md"
        );
        let taken = ["20261004123400.todo.md", "20261004123401.todo.md"];
        assert_eq!(
            free_file_name(now, |n| taken.contains(&n)).unwrap(),
            "20261004123402.todo.md"
        );
        // Crosses a minute boundary like a clock would.
        let late = at("2026-10-04 12:34") + Duration::seconds(59);
        assert_eq!(
            free_file_name(late, |n| n == "20261004123459.todo.md").unwrap(),
            "20261004123500.todo.md"
        );
    }

    #[test]
    fn free_name_gives_up_after_the_limit() {
        let now = at("2026-10-04 12:34");
        assert_eq!(free_file_name(now, |_| true), None);
        let last_free = free_file_name(now, |n| n != "20261004123459.todo.md").unwrap();
        assert_eq!(
            last_free, "20261004123459.todo.md",
            "attempt 60 is still tried"
        );
        let none = free_file_name(now, |n| n != "20261004123500.todo.md");
        assert_eq!(none, None, "attempt 61 is not");
    }

    #[test]
    fn drafts_need_labelled_merge_requests() {
        assert!(check_draft(&TaskDraft::new("t")).is_ok());
        let ok = TaskDraft::new("t").with_merge_request(Link::labelled("https://x/1", "One"));
        assert!(check_draft(&ok).is_ok());
        let bare = TaskDraft::new("t")
            .with_merge_request(Link::labelled("https://x/1", "One"))
            .with_merge_request(Link::new("https://x/2"));
        assert_eq!(
            check_draft(&bare).unwrap_err().to_string(),
            "unsupported: creating a task with the merge request https://x/2 without a title (resolve the title first)"
        );
        let empty = TaskDraft::new("t").with_merge_request(Link::labelled("https://x/3", ""));
        assert!(check_draft(&empty).is_err());
    }
}
