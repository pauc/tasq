//! Time source abstraction so progress logging and due dates are testable.
//!
//! Everything in `tasq` that stamps a time goes through [`Clock`], so tests
//! inject a [`FixedClock`] and production code uses [`SystemClock`].
//!
//! # Local time, no zone
//!
//! The files `tasq` writes are the files the original `tasks` script wrote,
//! and that script stamped entries with `date +'%F %H:%M'`: the wall-clock
//! time of the machine, with no zone information. We keep that contract.
//! [`Clock::now`] returns a [`NaiveDateTime`] in **local time**, and the
//! helpers here format and parse that same `YYYY-MM-DD HH:MM` shape. Nothing
//! in the domain converts between zones.
//!
//! # Where the timestamp helpers live
//!
//! The format layer needs to turn timestamps into text and back exactly the
//! way the script did. Those helpers ([`format_timestamp`], [`parse_timestamp`],
//! [`format_date`], [`parse_date`]) and the lossless [`When`] type live in this
//! module rather than in `model`, because they are about *time* rather than
//! about *tasks*; `model` re-exports [`When`] for convenience.

use std::fmt;
use std::str::FromStr;

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The `YYYY-MM-DD HH:MM` shape the original script wrote with `date +'%F %H:%M'`.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

/// The `YYYY-MM-DD` shape used for due dates and legacy progress entries.
pub const DATE_FORMAT: &str = "%Y-%m-%d";

/// A source of "now", in local wall-clock time (see the module docs).
pub trait Clock {
    /// The current local date and time, to the second or better.
    fn now(&self) -> NaiveDateTime;

    /// The current local date; a convenience over [`Clock::now`].
    fn today(&self) -> NaiveDate {
        self.now().date()
    }
}

/// The real clock: `chrono::Local::now()`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl Clock for SystemClock {
    /// Reason: reads the wall clock; nothing deterministic to assert.
    #[mutants::skip]
    fn now(&self) -> NaiveDateTime {
        Local::now().naive_local()
    }
}

/// A clock frozen at one instant, for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub NaiveDateTime);

impl FixedClock {
    /// A fixed clock at `date` and `time`, as `YYYY-MM-DD HH:MM` strings.
    ///
    /// # Panics
    ///
    /// Panics when the strings do not parse; it is a test helper.
    pub fn at(timestamp: &str) -> Self {
        let at = NaiveDateTime::parse_from_str(timestamp, TIMESTAMP_FORMAT)
            .unwrap_or_else(|e| panic!("FixedClock::at({timestamp:?}): {e}"));
        Self(at)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> NaiveDateTime {
        self.0
    }
}

/// Formats a timestamp as `YYYY-MM-DD HH:MM`, the shape the script wrote.
///
/// Seconds are dropped, not rounded, matching `date +'%F %H:%M'`.
pub fn format_timestamp(at: NaiveDateTime) -> String {
    at.format(TIMESTAMP_FORMAT).to_string()
}

/// Formats a date as `YYYY-MM-DD`.
pub fn format_date(date: NaiveDate) -> String {
    date.format(DATE_FORMAT).to_string()
}

/// Parses `YYYY-MM-DD HH:MM` or a bare `YYYY-MM-DD` (legacy progress entries).
///
/// Leading and trailing whitespace is ignored. The result remembers which
/// shape it saw, so the format layer can write it back unchanged.
pub fn parse_timestamp(text: &str) -> Result<When, TimeError> {
    let text = text.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(text, TIMESTAMP_FORMAT) {
        return Ok(When::DateTime(dt));
    }
    parse_date(text).map(When::Date)
}

/// Parses a `YYYY-MM-DD` date. Leading and trailing whitespace is ignored.
pub fn parse_date(text: &str) -> Result<NaiveDate, TimeError> {
    let text = text.trim();
    NaiveDate::parse_from_str(text, DATE_FORMAT)
        .map_err(|_| TimeError::InvalidDate(text.to_owned()))
}

/// Errors from the timestamp helpers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TimeError {
    /// The text is neither `YYYY-MM-DD HH:MM` nor `YYYY-MM-DD`.
    #[error("invalid date or timestamp {0:?} (expected YYYY-MM-DD or YYYY-MM-DD HH:MM)")]
    InvalidDate(String),
}

/// A point in time as the task file records it: a full timestamp, or just a
/// date for entries the script wrote before it logged times.
///
/// Modelled as an enum rather than widening everything to a
/// [`NaiveDateTime`] so that a file with legacy `- 2025-03-01: note` entries is
/// rewritten byte-for-byte: the format layer prints whichever shape it read.
/// Code that only wants to order or compare entries calls [`When::date_time`],
/// which puts a date-only value at midnight.
///
/// Serialises as the same string the file uses (`YYYY-MM-DD` or
/// `YYYY-MM-DD HH:MM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum When {
    /// Only the day is known (legacy entries).
    Date(NaiveDate),
    /// Day and time, to the minute.
    DateTime(NaiveDateTime),
}

impl When {
    /// The calendar day.
    pub fn date(self) -> NaiveDate {
        match self {
            Self::Date(d) => d,
            Self::DateTime(dt) => dt.date(),
        }
    }

    /// The instant, with a date-only value placed at `00:00`.
    pub fn date_time(self) -> NaiveDateTime {
        match self {
            Self::Date(d) => d.and_time(NaiveTime::MIN),
            Self::DateTime(dt) => dt,
        }
    }

    /// Whether the file recorded a time, not just a day.
    pub fn has_time(self) -> bool {
        matches!(self, Self::DateTime(_))
    }
}

impl From<NaiveDateTime> for When {
    fn from(dt: NaiveDateTime) -> Self {
        Self::DateTime(dt)
    }
}

impl From<NaiveDate> for When {
    fn from(d: NaiveDate) -> Self {
        Self::Date(d)
    }
}

impl fmt::Display for When {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(d) => f.write_str(&format_date(*d)),
            Self::DateTime(dt) => f.write_str(&format_timestamp(*dt)),
        }
    }
}

impl FromStr for When {
    type Err = TimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_timestamp(s)
    }
}

impl TryFrom<String> for When {
    type Error = TimeError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<When> for String {
    fn from(w: When) -> Self {
        w.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn fixed_clock_returns_its_instant_and_day() {
        let clock = FixedClock::at("2026-10-04 10:15");
        assert_eq!(clock.now(), dt("2026-10-04 10:15:00"));
        assert_eq!(clock.today(), d("2026-10-04"));
    }

    #[test]
    #[should_panic(expected = "FixedClock::at(\"nope\")")]
    fn fixed_clock_at_panics_on_garbage() {
        let _ = FixedClock::at("nope");
    }

    #[test]
    fn system_clock_is_close_to_chrono_local() {
        let before = Local::now().naive_local();
        let now = SystemClock.now();
        let after = Local::now().naive_local();
        assert!(before <= now && now <= after);
    }

    #[test]
    fn format_timestamp_drops_seconds_without_rounding() {
        assert_eq!(
            format_timestamp(dt("2026-10-04 10:15:59")),
            "2026-10-04 10:15"
        );
        assert_eq!(
            format_timestamp(dt("2026-01-01 00:00:00")),
            "2026-01-01 00:00"
        );
    }

    #[test]
    fn format_date_is_iso() {
        assert_eq!(format_date(d("2026-10-04")), "2026-10-04");
    }

    #[test]
    fn parse_timestamp_accepts_both_shapes_and_trims() {
        assert_eq!(
            parse_timestamp("2026-10-04 10:15"),
            Ok(When::DateTime(dt("2026-10-04 10:15:00")))
        );
        assert_eq!(
            parse_timestamp("2026-10-04"),
            Ok(When::Date(d("2026-10-04")))
        );
        assert_eq!(
            parse_timestamp("  2026-10-04 10:15 "),
            Ok(When::DateTime(dt("2026-10-04 10:15:00")))
        );
        assert_eq!(
            parse_timestamp(" 2026-10-04 "),
            Ok(When::Date(d("2026-10-04")))
        );
    }

    #[test]
    fn parse_timestamp_rejects_garbage_with_trimmed_text() {
        assert_eq!(
            parse_timestamp(" 2026-10-04 10:15:00 "),
            Err(TimeError::InvalidDate("2026-10-04 10:15:00".into()))
        );
        assert_eq!(
            parse_timestamp(""),
            Err(TimeError::InvalidDate(String::new()))
        );
        assert_eq!(
            parse_timestamp("04/10/2026"),
            Err(TimeError::InvalidDate("04/10/2026".into()))
        );
        assert_eq!(
            parse_timestamp("2026-13-01"),
            Err(TimeError::InvalidDate("2026-13-01".into()))
        );
    }

    #[test]
    fn parse_date_rejects_timestamps() {
        assert_eq!(parse_date("2026-10-04"), Ok(d("2026-10-04")));
        assert_eq!(parse_date(" 2026-10-04\n"), Ok(d("2026-10-04")));
        assert_eq!(
            parse_date("2026-10-04 10:15"),
            Err(TimeError::InvalidDate("2026-10-04 10:15".into()))
        );
    }

    #[test]
    fn time_error_message_names_the_input() {
        let msg = TimeError::InvalidDate("x".into()).to_string();
        assert!(msg.contains("\"x\""), "{msg}");
        assert!(msg.contains("YYYY-MM-DD HH:MM"), "{msg}");
    }

    #[test]
    fn when_round_trips_through_text_losslessly() {
        for text in ["2026-10-04", "2026-10-04 10:15"] {
            let w: When = text.parse().unwrap();
            assert_eq!(w.to_string(), text);
            assert_eq!(String::from(w), text);
            assert_eq!(When::try_from(text.to_owned()), Ok(w));
        }
        assert!("nope".parse::<When>().is_err());
        assert!(When::try_from("nope".to_owned()).is_err());
    }

    #[test]
    fn when_accessors() {
        let date = When::Date(d("2026-10-04"));
        let time = When::DateTime(dt("2026-10-04 10:15:00"));
        assert_eq!(date.date(), d("2026-10-04"));
        assert_eq!(time.date(), d("2026-10-04"));
        assert_eq!(date.date_time(), dt("2026-10-04 00:00:00"));
        assert_eq!(time.date_time(), dt("2026-10-04 10:15:00"));
        assert!(!date.has_time());
        assert!(time.has_time());
    }

    #[test]
    fn when_from_chrono_types() {
        assert_eq!(When::from(d("2026-10-04")), When::Date(d("2026-10-04")));
        assert_eq!(
            When::from(dt("2026-10-04 10:15:00")),
            When::DateTime(dt("2026-10-04 10:15:00"))
        );
    }

    #[test]
    fn when_serde_uses_the_file_text() {
        let time = When::DateTime(dt("2026-10-04 10:15:00"));
        let date = When::Date(d("2026-10-04"));
        assert_eq!(
            serde_json::to_string(&time).unwrap(),
            "\"2026-10-04 10:15\""
        );
        assert_eq!(serde_json::to_string(&date).unwrap(), "\"2026-10-04\"");
        assert_eq!(
            serde_json::from_str::<When>("\"2026-10-04 10:15\"").unwrap(),
            time
        );
        assert_eq!(
            serde_json::from_str::<When>("\"2026-10-04\"").unwrap(),
            date
        );
        assert!(serde_json::from_str::<When>("\"later\"").is_err());
    }
}
