# ADR-0017: A calendar picker for the Due box of the edit view

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

ADR-0016's edit view takes the due date as text: ISO, or `today`,
`tomorrow`, `yesterday`, resolved on save. Typing is the fast path and
stays; what it lacks is a way to pick "the Friday after next" without
counting days or knowing the date. Every date field in a desktop form
has a month popup for that.

The constraints of ADR-0016 hold: the TUI depends on `tasq-core` and
ratatui only, `update` is pure, and the keys of a typing mode are fixed
(ADR-0013). ratatui ships a month widget (`Monthly`, behind the
`widget-calendar` feature) that styles each day through a `DateStyler`.
Two things ruled it out once tried: its weeks always start on Sunday
with no option for Monday, which the author rejected outright for a tool
used in Europe, and it works on the `time` crate's `Date` while the
model and the core use chrono, so every date would cross a crate
boundary. The widget's layout (a title, a weekday header, rows of seven
two-character cells with a one-column gutter) is a hundred lines.

## Decision

- **`Enter` on the Due box opens the picker.** A new mode,
  `Mode::Calendar { form, calendar }`, keeps the edit view underneath as it
  was and draws one month centred over it: the month and year above a
  dim weekday header, the day under the cursor reversed, today bold,
  Saturdays and Sundays dim, a cyan border like the focused box, the
  same `Due` title. The grid is the crate's own (`calendar::month_grid`,
  after ratatui's `Monthly`, MIT): rows of seven cells, the first column
  the configured weekday.
- **The week starts where the user says: `ui.week_start`.** A weekday
  name (`monday` ... `sunday`), `monday` by default, `TASQ_WEEK_START` in
  the environment; the CLI passes it to the model as a chrono `Weekday`
  and the grid rotates its columns. The header names the columns, so a
  grid is never ambiguous. The terminal cursor is hidden while the picker has
  the keys. The picker opens on the day the box holds when it parses
  (ISO or one of the words, against the model's `today`), else on today.
  `Enter` on the other rows keeps its meaning (the next row, a newline in
  the description).
- **Fixed keys, the smallest set that covers a month.** The arrows move
  by a day and a week, `PageUp`/`PageDown` by a month with the day of the
  month clamped to the target month's length, `t` jumps to today, `Enter`
  picks, `Esc` closes without touching the box, `Ctrl+C` quits. Every
  other key does nothing: typing belongs to the box, not the picker.
- **Picking writes ISO into the box.** The picked day replaces the box's
  text as `YYYY-MM-DD` with the cursor at its end, and the view takes
  over again with the focus still on Due. Save validates the box as
  before; the picker never bypasses `Form::fields`.
- **Pure state, chrono throughout.** `calendar::Calendar` is the day
  under the cursor plus the date arithmetic the keys drive;
  `calendar::month_grid` is a pure layout function (title, header, weeks
  of `Option<NaiveDate>`) the view turns into styled spans
  (`view::day_style`). No new dependency: the `time` crate and the
  widget feature stay out.
- **The status bar is a key bar** here too: `form_hints` became
  `key_bar(hints, width)` and the picker has its own table, labels
  dropped when they do not fit.

## Consequences

### Positive

- A date a few weeks out is three or four keys away, and the month grid
  answers "which weekday is that" without leaving the view.
- Typing stays untouched: nothing changed for `tomorrow` or an ISO date
  typed straight in, and a picked day is just text in the box, so the
  same validation and the same `edit::revise` apply.
- The date arithmetic, the grid layout and the key translation are pure
  and under mutation testing; the popup is pinned by snapshots (Monday
  and Sunday starts) and buffer-style assertions.

### Negative

- A hundred lines of layout the crate now owns instead of the widget:
  the month name, the header, the row count. Pinned by unit tests for
  four-, five- and six-row months and both ends of chrono's range.
- No typing inside the picker (a year, a month name): `Esc` and type
  into the box instead, which is as fast.
- The popup covers the middle of the view, including part of the
  description, while it is open. It is small (25 by up to 10 cells) and
  closes on the next key that matters.

## Alternatives considered

- **ratatui's `Monthly` widget** — the first cut used it; Sunday-only
  weeks and the `time` dates made it a worse deal than a hundred lines
  of layout.
- **A fixed Monday start** — right for the author, wrong for anyone
  else; a weekday name in `[ui]` costs one enum.
- **A separate `date` action and key** (`Ctrl+D` or so) instead of
  `Enter` — one more chord to remember; `Enter` on a field that is not
  multi-line was the natural "open" and had no other job on this row.
- **Opening the picker on focus** — the box would be unusable for typing
  without closing the popup first, and typing is the fast path.
- **Keep the widget and convert chrono to `time` at the boundary** —
  what the first cut did; one more date type in the crate for a grid
  that still started on Sunday.

## References

- ADR-0016 (the edit view), ADR-0013 (fixed typing modes).
- `crates/tui/src/calendar.rs` (`Calendar`, `month_grid`),
  `crates/tui/src/update.rs` (`calendar_mode`, the `Enter` branch of
  `form_mode`), `crates/tui/src/keys.rs` (`calendar`),
  `crates/tui/src/view.rs` (`render_calendar`, `day_style`, `key_bar`,
  `CALENDAR_HINTS`), `crates/tui/tests/render.rs` (`calendar_picker`),
  `crates/core/src/config/mod.rs` (`WeekStart`, `UiConfig::week_start`),
  `docs/config.md`.
- ratatui `Monthly`, the layout the grid follows:
  <https://docs.rs/ratatui/0.30/ratatui/widgets/calendar/struct.Monthly.html>.
