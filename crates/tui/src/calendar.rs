//! The calendar picker of the edit view's Due box (ADR-0017): a month
//! shown over the view, a day moved with the keys, picked with `Enter`.
//!
//! Pure data, like the rest of the model: [`Calendar`] is the day under
//! the cursor and the date arithmetic the keys drive; [`month_grid`] lays
//! the month out as rows of seven cells starting on the configured
//! weekday, and [`crate::view()`] draws them. The layout follows ratatui's
//! `Monthly` widget (MIT), which was not used because its weeks always
//! start on Sunday.

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};
use tasq_core::dates::parse_day;

/// The picker's state: the day under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Calendar {
    /// The day the cursor is on; `Enter` picks it.
    pub day: NaiveDate,
}

impl Calendar {
    /// The picker opened on what the Due box holds: that day when it
    /// parses (ISO or `today`/`tomorrow`/`yesterday`, against `today`),
    /// else `today`.
    pub fn open(due: &str, today: NaiveDate) -> Self {
        let day = parse_day(due.trim(), today).unwrap_or(today);
        Self { day }
    }

    /// Moves the cursor `days` forward (back for a negative count);
    /// stays put at the ends of chrono's range.
    pub fn shift_days(&mut self, days: i64) {
        let count = Days::new(days.unsigned_abs());
        let moved = if days.is_negative() {
            self.day.checked_sub_days(count)
        } else {
            self.day.checked_add_days(count)
        };
        self.day = moved.unwrap_or(self.day);
    }

    /// Moves the cursor `months` forward (back for a negative count), the
    /// day of the month clamped to the target month's length (31 January
    /// goes to 28 or 29 February); stays put at the ends of chrono's
    /// range.
    pub fn shift_months(&mut self, months: i32) {
        let count = Months::new(months.unsigned_abs());
        let moved = if months.is_negative() {
            self.day.checked_sub_months(count)
        } else {
            self.day.checked_add_months(count)
        };
        self.day = moved.unwrap_or(self.day);
    }

    /// The cursor to `today` (`t`).
    pub fn today(&mut self, today: NaiveDate) {
        self.day = today;
    }

    /// The day as the Due box takes it: ISO `YYYY-MM-DD`.
    pub fn picked(&self) -> String {
        self.day.format("%Y-%m-%d").to_string()
    }
}

/// Days per row of the grid.
pub const DAYS_PER_WEEK: usize = 7;

/// A month laid out for the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthGrid {
    /// The month and year, `October 2026`.
    pub title: String,
    /// The two-letter weekday names in column order, each after a space:
    /// ` Mo Tu We Th Fr Sa Su` for a Monday start.
    pub header: String,
    /// The weeks, top to bottom: seven cells each, a day of the month or
    /// `None` before the 1st and after the last day.
    pub weeks: Vec<[Option<NaiveDate>; DAYS_PER_WEEK]>,
}

/// The month of `day`, its weeks starting on `week_start`.
pub fn month_grid(day: NaiveDate, week_start: Weekday) -> MonthGrid {
    let first = day.with_day(1).unwrap_or(day);
    let title = first.format("%B %Y").to_string();
    let mut header = String::new();
    let mut weekday = week_start;
    for _ in 0..DAYS_PER_WEEK {
        header.push(' ');
        header.push_str(&weekday.to_string()[..2]);
        weekday = weekday.succ();
    }
    let mut weeks = Vec::new();
    let mut week = [None; DAYS_PER_WEEK];
    let mut column = column_of(first.weekday(), week_start);
    let mut date = Some(first);
    while let Some(day) = date.filter(|d| d.month() == first.month()) {
        week[column] = Some(day);
        column += 1;
        if column == DAYS_PER_WEEK {
            weeks.push(week);
            week = [None; DAYS_PER_WEEK];
            column = 0;
        }
        date = day.succ_opt();
    }
    if column > 0 {
        weeks.push(week);
    }
    MonthGrid {
        title,
        header,
        weeks,
    }
}

/// The column of `weekday` in a week starting on `week_start`.
fn column_of(weekday: Weekday, week_start: Weekday) -> usize {
    let days = (weekday.num_days_from_monday() + 7 - week_start.num_days_from_monday()) % 7;
    usize::try_from(days).unwrap_or(0)
}

/// Whether `day` is a Saturday or a Sunday (drawn dim in the picker).
pub fn is_weekend(day: NaiveDate) -> bool {
    matches!(day.weekday(), Weekday::Sat | Weekday::Sun)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn opens_on_the_typed_day_or_today() {
        let today = date("2026-10-05");
        assert_eq!(Calendar::open("2026-12-24", today).day, date("2026-12-24"));
        assert_eq!(Calendar::open(" tomorrow ", today).day, date("2026-10-06"));
        assert_eq!(Calendar::open("", today).day, today);
        assert_eq!(Calendar::open("soon", today).day, today);
    }

    #[test]
    fn moves_by_day_and_week() {
        let mut c = Calendar::open("2026-10-31", date("2026-10-05"));
        c.shift_days(1);
        assert_eq!(c.day, date("2026-11-01"), "into the next month");
        c.shift_days(-7);
        assert_eq!(c.day, date("2026-10-25"));
        c.shift_days(7);
        assert_eq!(c.day, date("2026-11-01"));
        c.shift_days(-1);
        assert_eq!(c.day, date("2026-10-31"));
        c.shift_days(0);
        assert_eq!(c.day, date("2026-10-31"));
        let mut edge = Calendar {
            day: NaiveDate::MAX,
        };
        edge.shift_days(1);
        assert_eq!(edge.day, NaiveDate::MAX, "stays at the end of the range");
        edge.shift_days(-1);
        assert_eq!(edge.day, NaiveDate::MAX.pred_opt().unwrap());
        let mut edge = Calendar {
            day: NaiveDate::MIN,
        };
        edge.shift_days(-1);
        assert_eq!(edge.day, NaiveDate::MIN, "stays at the start of the range");
    }

    #[test]
    fn moves_by_month_with_the_day_clamped() {
        let mut c = Calendar::open("2026-01-31", date("2026-10-05"));
        c.shift_months(1);
        assert_eq!(
            c.day,
            date("2026-02-28"),
            "31 January to the end of February"
        );
        c.shift_months(1);
        assert_eq!(c.day, date("2026-03-28"), "the clamped day stays");
        c.shift_months(-3);
        assert_eq!(c.day, date("2025-12-28"), "back across the year");
        c.shift_months(0);
        assert_eq!(c.day, date("2025-12-28"));
        let mut edge = Calendar {
            day: NaiveDate::MAX,
        };
        edge.shift_months(1);
        assert_eq!(edge.day, NaiveDate::MAX, "stays at the end of the range");
        let mut edge = Calendar {
            day: NaiveDate::MIN,
        };
        edge.shift_months(-1);
        assert_eq!(edge.day, NaiveDate::MIN, "stays at the start of the range");
    }

    #[test]
    fn today_and_the_picked_text() {
        let today = date("2026-10-05");
        let mut c = Calendar::open("2030-01-01", today);
        c.today(today);
        assert_eq!(c.day, today);
        assert_eq!(c.picked(), "2026-10-05");
        assert_eq!(
            Calendar {
                day: date("0987-03-04")
            }
            .picked(),
            "0987-03-04"
        );
    }

    #[test]
    fn weekends() {
        assert!(!is_weekend(date("2026-10-05")), "Monday");
        assert!(!is_weekend(date("2026-10-09")), "Friday");
        assert!(is_weekend(date("2026-10-10")), "Saturday");
        assert!(is_weekend(date("2026-10-11")), "Sunday");
    }

    /// The grid as text: `..` for a blank cell, the day of the month
    /// otherwise.
    fn rows(grid: &MonthGrid) -> Vec<String> {
        grid.weeks
            .iter()
            .map(|week| {
                week.iter()
                    .map(|cell| cell.map_or("..".to_owned(), |d| format!("{:2}", d.day())))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn a_month_from_monday() {
        // October 2026 starts on a Thursday and ends on a Saturday.
        let grid = month_grid(date("2026-10-20"), Weekday::Mon);
        assert_eq!(grid.title, "October 2026");
        assert_eq!(grid.header, " Mo Tu We Th Fr Sa Su");
        assert_eq!(
            rows(&grid),
            [
                ".. .. ..  1  2  3  4",
                " 5  6  7  8  9 10 11",
                "12 13 14 15 16 17 18",
                "19 20 21 22 23 24 25",
                "26 27 28 29 30 31 .."
            ]
        );
        assert_eq!(grid.weeks[0][3], Some(date("2026-10-01")));
        assert_eq!(grid.weeks[4][5], Some(date("2026-10-31")));
    }

    #[test]
    fn a_month_from_sunday_and_from_saturday() {
        let grid = month_grid(date("2026-10-01"), Weekday::Sun);
        assert_eq!(grid.header, " Su Mo Tu We Th Fr Sa");
        assert_eq!(
            rows(&grid),
            [
                ".. .. .. ..  1  2  3",
                " 4  5  6  7  8  9 10",
                "11 12 13 14 15 16 17",
                "18 19 20 21 22 23 24",
                "25 26 27 28 29 30 31"
            ]
        );
        // From Saturday the same month needs six rows: the 31st is one.
        let grid = month_grid(date("2026-10-31"), Weekday::Sat);
        assert_eq!(grid.header, " Sa Su Mo Tu We Th Fr");
        assert_eq!(rows(&grid)[0], ".. .. .. .. ..  1  2");
        assert_eq!(rows(&grid)[5], "31 .. .. .. .. .. ..");
        assert_eq!(grid.weeks.len(), 6);
    }

    #[test]
    fn months_that_fill_four_five_or_six_rows() {
        // February 2027 starts on a Monday: exactly four rows.
        let grid = month_grid(date("2027-02-14"), Weekday::Mon);
        assert_eq!(grid.title, "February 2027");
        assert_eq!(grid.weeks.len(), 4);
        assert_eq!(rows(&grid)[0], " 1  2  3  4  5  6  7");
        assert_eq!(rows(&grid)[3], "22 23 24 25 26 27 28");
        // May 2027 starts on a Saturday and has 31 days: six rows.
        let grid = month_grid(date("2027-05-01"), Weekday::Mon);
        assert_eq!(grid.weeks.len(), 6);
        assert_eq!(rows(&grid)[0], ".. .. .. .. ..  1  2");
        assert_eq!(rows(&grid)[5], "31 .. .. .. .. .. ..");
        // A leap February, from Sunday.
        let grid = month_grid(date("2024-02-29"), Weekday::Sun);
        assert_eq!(rows(&grid)[4], "25 26 27 28 29 .. ..");
        // The ends of chrono's range still lay out.
        let grid = month_grid(NaiveDate::MAX, Weekday::Mon);
        assert_eq!(
            grid.weeks.last().unwrap().iter().flatten().last(),
            Some(&NaiveDate::MAX)
        );
        let grid = month_grid(NaiveDate::MIN, Weekday::Mon);
        assert_eq!(grid.weeks[0].iter().flatten().next(), Some(&NaiveDate::MIN));
    }

    #[test]
    fn columns() {
        assert_eq!(column_of(Weekday::Mon, Weekday::Mon), 0);
        assert_eq!(column_of(Weekday::Sun, Weekday::Mon), 6);
        assert_eq!(column_of(Weekday::Sun, Weekday::Sun), 0);
        assert_eq!(column_of(Weekday::Sat, Weekday::Sun), 6);
        assert_eq!(column_of(Weekday::Mon, Weekday::Sun), 1);
        assert_eq!(column_of(Weekday::Fri, Weekday::Sat), 6);
        assert_eq!(column_of(Weekday::Thu, Weekday::Wed), 1);
    }
}
