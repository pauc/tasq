//! The pure parts of [`Store::create`](tasq_core::store::Store::create) for the nb store: nb's
//! filename rule (plan T-203). The draft check and the default note are
//! [`tasq_store::draft`]'s, shared with the tasq store.
//!
//! nb names a todo `YYYYMMDDHHMMSS.todo.md` after the local creation time
//! and, when that name is taken, moves to the next second until it finds a
//! free one. The store reproduces the rule so the files it creates look like
//! nb's own and sort by creation time.

use chrono::{Duration, NaiveDateTime};

use crate::index::TODO_SUFFIX;

pub use tasq_store::draft::{DEFAULT_NOTE, check_draft};

/// How many consecutive seconds are tried before giving up on a free name.
pub const MAX_ATTEMPTS: u32 = 60;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;

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
}
