//! Human date words: `today`, `tomorrow`, `yesterday` and ISO dates.
//!
//! [`parse_day`] is what `--due` accepts. It is pure: the caller passes
//! "today" (from a [`Clock`](crate::clock::Clock)), so tests pin the date.
//! Ranges and weekday names arrive with the report commands (plan T-601
//! and T-602), in this module.

use chrono::{Duration, NaiveDate};

use crate::clock::{TimeError, parse_date};

/// Parses a single day: `today`, `tomorrow`, `yesterday` (any case) or
/// `YYYY-MM-DD`. Surrounding whitespace is ignored.
pub fn parse_day(text: &str, today: NaiveDate) -> Result<NaiveDate, TimeError> {
    let word = text.trim();
    match word.to_ascii_lowercase().as_str() {
        "today" => Ok(today),
        "tomorrow" => Ok(today + Duration::days(1)),
        "yesterday" => Ok(today - Duration::days(1)),
        _ => parse_date(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    #[test]
    fn words_are_relative_to_the_given_day() {
        let today = day("2026-10-04");
        assert_eq!(parse_day("today", today), Ok(today));
        assert_eq!(parse_day("Tomorrow", today), Ok(day("2026-10-05")));
        assert_eq!(parse_day(" YESTERDAY ", today), Ok(day("2026-10-03")));
        // Month boundaries.
        assert_eq!(
            parse_day("tomorrow", day("2026-10-31")),
            Ok(day("2026-11-01"))
        );
        assert_eq!(
            parse_day("yesterday", day("2026-03-01")),
            Ok(day("2026-02-28"))
        );
    }

    #[test]
    fn iso_dates_pass_through() {
        let today = day("2026-10-04");
        assert_eq!(parse_day("2026-12-24", today), Ok(day("2026-12-24")));
        assert_eq!(parse_day(" 2026-12-24 ", today), Ok(day("2026-12-24")));
    }

    #[test]
    fn anything_else_is_an_error_naming_the_text() {
        let today = day("2026-10-04");
        assert_eq!(
            parse_day("next week", today),
            Err(TimeError::InvalidDate("next week".to_owned()))
        );
        assert_eq!(
            parse_day("2026-13-01", today),
            Err(TimeError::InvalidDate("2026-13-01".to_owned()))
        );
        assert!(parse_day("", today).is_err());
    }
}
