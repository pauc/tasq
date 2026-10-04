//! Human dates: `today`, `yesterday`, weekday names, ISO dates and the
//! ranges reports take (`this week`, `last month`, `last 7 days`, two
//! dates).
//!
//! Everything here is pure: the caller passes "today" (from a
//! [`Clock`](crate::clock::Clock)), so tests pin the date. Two single-day
//! parsers exist because the same word means different days depending on
//! what it is for: a due date looks forward ([`parse_day`]), a report looks
//! back ([`parse_past_day`]).

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::clock::{TimeError, format_date, parse_date};

/// Parses a single day for a due date: `today`, `tomorrow`, `yesterday`
/// (any case) or `YYYY-MM-DD`. Surrounding whitespace is ignored.
pub fn parse_day(text: &str, today: NaiveDate) -> Result<NaiveDate, TimeError> {
    let word = text.trim();
    match word.to_ascii_lowercase().as_str() {
        "today" => Ok(today),
        "tomorrow" => Ok(today + Duration::days(1)),
        "yesterday" => Ok(today - Duration::days(1)),
        _ => parse_date(word),
    }
}

/// Parses a single day for a report, looking back: `today`, `yesterday`,
/// `YYYY-MM-DD`, a weekday name (`monday`, `mon`: the most recent one,
/// today included) or `last <weekday>` (the most recent one before today).
/// Case and surrounding whitespace do not matter.
pub fn parse_past_day(text: &str, today: NaiveDate) -> Result<NaiveDate, DateError> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let unknown = || DateError::UnknownDay(text.trim().to_owned());
    match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["today"] => Ok(today),
        ["yesterday"] => Ok(today - Duration::days(1)),
        [word] => match parse_weekday(word) {
            Some(weekday) => Ok(previous_or_today(today, weekday)),
            None => parse_date(word).map_err(|_| unknown()),
        },
        ["last", word] => parse_weekday(word)
            .map(|weekday| previous_or_today(today - Duration::days(1), weekday))
            .ok_or_else(unknown),
        _ => Err(unknown()),
    }
}

/// A weekday from its English name or three-letter abbreviation, any case.
pub fn parse_weekday(word: &str) -> Option<Weekday> {
    word.trim().parse().ok()
}

/// The most recent `weekday` on or before `day`.
pub fn previous_or_today(day: NaiveDate, weekday: Weekday) -> NaiveDate {
    let back = (day.weekday().num_days_from_monday() + 7 - weekday.num_days_from_monday()) % 7;
    day - Duration::days(i64::from(back))
}

/// Whether `day` is Monday to Friday.
pub fn is_working_day(day: NaiveDate) -> bool {
    !matches!(day.weekday(), Weekday::Sat | Weekday::Sun)
}

/// The last Monday-to-Friday day before `today`: Friday on a Monday (and on
/// a weekend), otherwise yesterday. Public holidays are not known.
pub fn last_working_day(today: NaiveDate) -> NaiveDate {
    // Three consecutive days always hold a working day.
    (1..=3)
        .map(|back| today - Duration::days(back))
        .find(|day| is_working_day(*day))
        .unwrap_or(today)
}

/// The Monday of `day`'s week.
pub fn monday_of(day: NaiveDate) -> NaiveDate {
    previous_or_today(day, Weekday::Mon)
}

/// The first day of `day`'s month.
pub fn first_of_month(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

/// An inclusive range of days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DateRange {
    /// First day.
    pub from: NaiveDate,
    /// Last day, never before `from`.
    pub to: NaiveDate,
}

impl DateRange {
    /// A range of one day.
    pub fn day(day: NaiveDate) -> Self {
        Self { from: day, to: day }
    }

    /// A range from two days in either order.
    pub fn between(a: NaiveDate, b: NaiveDate) -> Self {
        Self {
            from: a.min(b),
            to: a.max(b),
        }
    }

    /// Whether `day` is inside the range.
    pub fn contains(&self, day: NaiveDate) -> bool {
        self.from <= day && day <= self.to
    }

    /// Every day of the range, oldest first.
    pub fn days(&self) -> Vec<NaiveDate> {
        self.from
            .iter_days()
            .take_while(|day| *day <= self.to)
            .collect()
    }

    /// The Monday-to-Friday days of the range, oldest first.
    pub fn working_days(&self) -> Vec<NaiveDate> {
        self.days()
            .into_iter()
            .filter(|day| is_working_day(*day))
            .collect()
    }
}

impl std::fmt::Display for DateRange {
    /// `YYYY-MM-DD YYYY-MM-DD`, the shape the original script printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", format_date(self.from), format_date(self.to))
    }
}

/// Resolves a range spec against `today`. Case and extra whitespace do not
/// matter. Accepted:
///
/// | Spec | Range |
/// |------|-------|
/// | nothing, `today` | today |
/// | `week`, `this week`, `current week` | Monday of this week to Friday, or today if earlier |
/// | `last week`, `past week`, `previous week` | Monday to Friday of the previous week |
/// | `month`, `this month`, `current month` | the 1st to today |
/// | `last month`, `past month`, `previous month` | the whole previous month |
/// | `last N days`, `past N days` (N > 0) | the N days ending today |
/// | one day (see [`parse_past_day`]) | that day |
/// | two days | both days and everything between, in either order |
///
/// A range never extends past today: an end after today is moved back to
/// today, a start after today is an error.
pub fn resolve_range(spec: &str, today: NaiveDate) -> Result<DateRange, DateError> {
    let words: Vec<String> = spec
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let unknown = || DateError::UnknownRange(spec.trim().to_owned());
    let range = match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => DateRange::day(today),
        ["week"] | ["this" | "current", "week"] => {
            let monday = monday_of(today);
            DateRange {
                from: monday,
                to: monday + Duration::days(4),
            }
        }
        ["last" | "past" | "previous", "week"] => {
            let monday = monday_of(today) - Duration::days(7);
            DateRange {
                from: monday,
                to: monday + Duration::days(4),
            }
        }
        ["month"] | ["this" | "current", "month"] => DateRange {
            from: first_of_month(today),
            to: today,
        },
        ["last" | "past" | "previous", "month"] => {
            let to = first_of_month(today) - Duration::days(1);
            DateRange {
                from: first_of_month(to),
                to,
            }
        }
        ["last" | "past", n, "day" | "days"] => {
            let n: i64 = n.parse().ok().filter(|n| *n > 0).ok_or_else(unknown)?;
            DateRange {
                from: today - Duration::days(n - 1),
                to: today,
            }
        }
        [a, b] if parse_past_day(spec, today).is_err() => DateRange::between(
            parse_past_day(a, today).map_err(|_| unknown())?,
            parse_past_day(b, today).map_err(|_| unknown())?,
        ),
        _ => DateRange::day(parse_past_day(spec, today).map_err(|_| unknown())?),
    };
    if range.from > today {
        return Err(DateError::InFuture {
            day: format_date(range.from),
            today: format_date(today),
        });
    }
    Ok(DateRange {
        from: range.from,
        to: range.to.min(today),
    })
}

/// Errors from the report date parsers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DateError {
    /// Not a day [`parse_past_day`] understands.
    #[error(
        "unrecognized date {0:?} (expected YYYY-MM-DD, today, yesterday, a weekday name or last <weekday>)"
    )]
    UnknownDay(String),
    /// Not a range [`resolve_range`] understands.
    #[error(
        "unrecognized date range {0:?} (expected a day, two days, this|last week, this|last month or last N days)"
    )]
    UnknownRange(String),
    /// The range starts after today.
    #[error("{day} is after today ({today}); reports never look ahead")]
    InFuture {
        /// The offending start, `YYYY-MM-DD`.
        day: String,
        /// Today, `YYYY-MM-DD`.
        today: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    // 2026-10-04 is a Sunday; 2026-10-07 a Wednesday.
    const SUNDAY: &str = "2026-10-04";
    const WEDNESDAY: &str = "2026-10-07";

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
        assert_eq!(
            parse_day("", today),
            Err(TimeError::InvalidDate(String::new()))
        );
    }

    #[test]
    fn weekday_names_and_abbreviations() {
        assert_eq!(parse_weekday("Monday"), Some(Weekday::Mon));
        assert_eq!(parse_weekday("fri"), Some(Weekday::Fri));
        assert_eq!(parse_weekday(" SUN "), Some(Weekday::Sun));
        assert_eq!(parse_weekday("someday"), None);
        assert_eq!(parse_weekday(""), None);
    }

    #[test]
    fn previous_or_today_counts_back_within_the_week() {
        let wednesday = day(WEDNESDAY);
        assert_eq!(previous_or_today(wednesday, Weekday::Wed), wednesday);
        assert_eq!(
            previous_or_today(wednesday, Weekday::Mon),
            day("2026-10-05")
        );
        assert_eq!(
            previous_or_today(wednesday, Weekday::Thu),
            day("2026-10-01")
        );
        assert_eq!(
            previous_or_today(day(SUNDAY), Weekday::Mon),
            day("2026-09-28")
        );
        assert_eq!(previous_or_today(day(SUNDAY), Weekday::Sun), day(SUNDAY));
    }

    #[test]
    fn past_days_table() {
        let today = day(WEDNESDAY);
        let cases: &[(&str, &str)] = &[
            ("today", WEDNESDAY),
            ("Yesterday", "2026-10-06"),
            ("2026-09-30", "2026-09-30"),
            (" 2026-10-07 ", WEDNESDAY),
            ("wednesday", WEDNESDAY),
            ("Wed", WEDNESDAY),
            ("monday", "2026-10-05"),
            ("friday", "2026-10-02"),
            ("thursday", "2026-10-01"),
            ("last wednesday", "2026-09-30"),
            ("LAST  Monday", "2026-10-05"),
            ("last tue", "2026-10-06"),
        ];
        for (text, expected) in cases {
            assert_eq!(
                parse_past_day(text, today),
                Ok(day(expected)),
                "parse_past_day({text:?})"
            );
        }
    }

    #[test]
    fn past_days_reject_forward_words_and_garbage() {
        let today = day(WEDNESDAY);
        for text in [
            "tomorrow",
            "next monday",
            "last",
            "last someday",
            "2026-13-01",
            "",
            "2026-10-07 10:15",
            "a b c",
        ] {
            assert_eq!(
                parse_past_day(text, today),
                Err(DateError::UnknownDay(text.trim().to_owned())),
                "parse_past_day({text:?})"
            );
        }
        let msg = parse_past_day("x", today).unwrap_err().to_string();
        assert_eq!(
            msg,
            "unrecognized date \"x\" (expected YYYY-MM-DD, today, yesterday, a weekday name or last <weekday>)"
        );
    }

    #[test]
    fn working_days() {
        assert!(is_working_day(day("2026-10-05")));
        assert!(is_working_day(day("2026-10-09")));
        assert!(!is_working_day(day("2026-10-10")));
        assert!(!is_working_day(day("2026-10-11")));
    }

    #[test]
    fn last_working_day_skips_weekends() {
        let cases: &[(&str, &str)] = &[
            ("2026-10-05", "2026-10-02"), // Monday -> Friday
            ("2026-10-06", "2026-10-05"), // Tuesday -> Monday
            ("2026-10-09", "2026-10-08"), // Friday -> Thursday
            ("2026-10-10", "2026-10-09"), // Saturday -> Friday
            ("2026-10-11", "2026-10-09"), // Sunday -> Friday
            ("2026-11-02", "2026-10-30"), // Monday across a month boundary
        ];
        for (today, expected) in cases {
            assert_eq!(
                last_working_day(day(today)),
                day(expected),
                "last_working_day({today})"
            );
        }
    }

    #[test]
    fn week_and_month_starts() {
        assert_eq!(monday_of(day(WEDNESDAY)), day("2026-10-05"));
        assert_eq!(monday_of(day("2026-10-05")), day("2026-10-05"));
        assert_eq!(monday_of(day(SUNDAY)), day("2026-09-28"));
        assert_eq!(first_of_month(day(WEDNESDAY)), day("2026-10-01"));
        assert_eq!(first_of_month(day("2026-10-01")), day("2026-10-01"));
    }

    #[test]
    fn range_helpers() {
        let range = DateRange::between(day("2026-10-02"), day("2026-09-30"));
        assert_eq!(
            range,
            DateRange {
                from: day("2026-09-30"),
                to: day("2026-10-02")
            }
        );
        assert_eq!(range.to_string(), "2026-09-30 2026-10-02");
        assert!(range.contains(day("2026-09-30")));
        assert!(range.contains(day("2026-10-01")));
        assert!(range.contains(day("2026-10-02")));
        assert!(!range.contains(day("2026-09-29")));
        assert!(!range.contains(day("2026-10-03")));
        assert_eq!(
            range.days(),
            vec![day("2026-09-30"), day("2026-10-01"), day("2026-10-02")]
        );
        assert_eq!(DateRange::day(day(SUNDAY)).days(), vec![day(SUNDAY)]);
        // 2026-10-02 is a Friday, 03/04 the weekend.
        let weekend = DateRange::between(day("2026-10-02"), day("2026-10-05"));
        assert_eq!(
            weekend.working_days(),
            vec![day("2026-10-02"), day("2026-10-05")]
        );
        assert_eq!(
            serde_json::to_string(&range).unwrap(),
            "{\"from\":\"2026-09-30\",\"to\":\"2026-10-02\"}"
        );
        assert_eq!(
            serde_json::from_str::<DateRange>("{\"from\":\"2026-09-30\",\"to\":\"2026-10-02\"}")
                .unwrap(),
            range
        );
    }

    #[test]
    fn ranges_table_on_a_wednesday() {
        let today = day(WEDNESDAY);
        let cases: &[(&str, &str, &str)] = &[
            ("", WEDNESDAY, WEDNESDAY),
            ("   ", WEDNESDAY, WEDNESDAY),
            ("today", WEDNESDAY, WEDNESDAY),
            ("yesterday", "2026-10-06", "2026-10-06"),
            ("week", "2026-10-05", WEDNESDAY),
            ("This Week", "2026-10-05", WEDNESDAY),
            ("current week", "2026-10-05", WEDNESDAY),
            ("last week", "2026-09-28", "2026-10-02"),
            ("past week", "2026-09-28", "2026-10-02"),
            ("previous  week", "2026-09-28", "2026-10-02"),
            ("month", "2026-10-01", WEDNESDAY),
            ("this month", "2026-10-01", WEDNESDAY),
            ("current month", "2026-10-01", WEDNESDAY),
            ("last month", "2026-09-01", "2026-09-30"),
            ("past month", "2026-09-01", "2026-09-30"),
            ("previous month", "2026-09-01", "2026-09-30"),
            ("last 1 day", WEDNESDAY, WEDNESDAY),
            ("last 7 days", "2026-10-01", WEDNESDAY),
            ("past 3 days", "2026-10-05", WEDNESDAY),
            ("2026-09-30", "2026-09-30", "2026-09-30"),
            ("monday", "2026-10-05", "2026-10-05"),
            ("last monday", "2026-10-05", "2026-10-05"),
            ("last wednesday", "2026-09-30", "2026-09-30"),
            ("2026-09-28 2026-10-02", "2026-09-28", "2026-10-02"),
            ("2026-10-02 2026-09-28", "2026-09-28", "2026-10-02"),
            ("monday today", "2026-10-05", WEDNESDAY),
            ("2026-09-28 2026-12-31", "2026-09-28", WEDNESDAY),
            ("last 30 days", "2026-09-08", WEDNESDAY),
        ];
        for (spec, from, to) in cases {
            assert_eq!(
                resolve_range(spec, today),
                Ok(DateRange {
                    from: day(from),
                    to: day(to)
                }),
                "resolve_range({spec:?})"
            );
        }
    }

    #[test]
    fn this_week_never_runs_past_today_or_friday() {
        // Monday: just today.
        assert_eq!(
            resolve_range("this week", day("2026-10-05")),
            Ok(DateRange::day(day("2026-10-05")))
        );
        // Sunday: Monday to Friday of the week that just ended.
        assert_eq!(
            resolve_range("this week", day(SUNDAY)),
            Ok(DateRange {
                from: day("2026-09-28"),
                to: day("2026-10-02")
            })
        );
        // Saturday too.
        assert_eq!(
            resolve_range("week", day("2026-10-10")),
            Ok(DateRange {
                from: day("2026-10-05"),
                to: day("2026-10-09")
            })
        );
    }

    #[test]
    fn month_edges() {
        assert_eq!(
            resolve_range("last month", day("2026-03-01")),
            Ok(DateRange {
                from: day("2026-02-01"),
                to: day("2026-02-28")
            })
        );
        assert_eq!(
            resolve_range("last month", day("2026-01-15")),
            Ok(DateRange {
                from: day("2025-12-01"),
                to: day("2025-12-31")
            })
        );
        assert_eq!(
            resolve_range("this month", day("2026-01-01")),
            Ok(DateRange::day(day("2026-01-01")))
        );
    }

    #[test]
    fn unknown_ranges_name_the_spec() {
        let today = day(WEDNESDAY);
        for spec in [
            "last 0 days",
            "last -1 days",
            "last x days",
            "last 2 weeks",
            "next week",
            "someday",
            "2026-09-28 someday",
            "someday 2026-09-28",
            "a b c",
            "last",
        ] {
            assert_eq!(
                resolve_range(spec, today),
                Err(DateError::UnknownRange(spec.to_owned())),
                "resolve_range({spec:?})"
            );
        }
        assert_eq!(
            resolve_range(" nope ", today).unwrap_err().to_string(),
            "unrecognized date range \"nope\" (expected a day, two days, this|last week, this|last month or last N days)"
        );
    }

    #[test]
    fn ranges_never_start_after_today() {
        let today = day(WEDNESDAY);
        let err = resolve_range("2026-10-08", today).unwrap_err();
        assert_eq!(
            err,
            DateError::InFuture {
                day: "2026-10-08".to_owned(),
                today: WEDNESDAY.to_owned()
            }
        );
        assert_eq!(
            err.to_string(),
            "2026-10-08 is after today (2026-10-07); reports never look ahead"
        );
        assert!(matches!(
            resolve_range("2026-10-08 2026-10-09", today),
            Err(DateError::InFuture { .. })
        ));
        // Today itself is fine.
        assert_eq!(resolve_range(WEDNESDAY, today), Ok(DateRange::day(today)));
    }
}
