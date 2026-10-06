# ADR-0019: Relative, coloured due dates

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

Both front ends rendered a due date as `(due 2026-10-03)` in the theme's `dim` role, whether
the date was three days past or three weeks away (`list.rs` `row`, `view.rs` `task_lines` and
`detail_lines`). The 2026-10-06 UX review found that an overdue task did not stand out from
the rest of the list, and that reading an ISO date and doing the arithmetic against today is
the user's job every time.

Constraints: the core stays pure (today comes from the `Clock`), the JSON contract
(`docs/json.md`) keeps the ISO date, and colours go through theme roles (ADR 0018) so presets,
`[ui.theme.colors]` and `NO_COLOR` keep working.

## Decision

`tasq_core::dates::Due::of(due, today)` classifies a date as `Overdue(n)`, `Today`,
`Tomorrow` or `Later(n)`, and `Due::phrase` renders it as `overdue 3d`, `due today`,
`due tomorrow` or `due in 4d`. `dates::due_label(due, today, format)` renders a date in one
of three formats, chosen by `ui.due_format` (`TASQ_DUE_FORMAT`): `relative` (the default),
`iso` (`due 2026-10-03`, the old rendering) or `both` (`overdue 3d, 2026-10-03`).

Two theme roles join `Role`: `overdue` (bold is added by the front ends) and `due-soon`, for
today and tomorrow. `Role::of_due` maps `Due` to `overdue`, `due-soon` or `dim`. Presets:
`dark` red / yellow, `light` 124 / 130, `solarized` 160 / 136, `gruvbox` 167 / 214 (each
preset's `blocked` and `waiting` shades), `mono` plain (so overdue is bold only).

The list rows of `tasq list` and `tasq ui` use `ui.due_format`. The TUI detail pane always
shows `both`, so the date is never more than a glance away. A done task in `tasq list --all`
/ `--done` always shows its ISO date, dim: a closed task is not overdue. `--json` is
unchanged.

## Consequences

### Positive

- Overdue and imminent tasks stand out in both front ends with the same colours.
- The classification is a pure function shared by the CLI and the TUI; the front ends only
  pick spans and escape codes.
- `ui.due_format = "iso"` restores the old text for anyone who prefers it.

### Negative

- List output now depends on today. CLI integration tests pin `TASQ_NOW` in the test harness
  and TUI render tests pin `Model::today`, so snapshots do not move with the calendar.
- A long-running `tasq ui` keeps the `today` it started with; past midnight the relative
  forms are one day off until it is restarted.

## Alternatives considered

- **Reuse `error` and `waiting` for the colours** — rejected: tying due dates to the error
  colour or a status colour means a user who restyles one restyles the other.
- **`due in 1d` for tomorrow** — rejected in favour of `due tomorrow`, which reads better and
  matches the `tomorrow` word `--due` accepts.
- **Colour past days in the calendar picker** — dropped: picking a past date is rare and the
  saved row shows it as overdue anyway.

## References

- `crates/core/src/dates.rs` (`Due`, `due_label`), `crates/core/src/theme.rs` (`Role::Overdue`,
  `Role::DueSoon`, `Role::of_due`), `crates/core/src/config/mod.rs` (`DueFormat`).
- `crates/cli/src/commands/list.rs` (`Look`, `row`), `crates/tui/src/view.rs` (`due_span`).
- ADR 0018 (named colour themes), `docs/config.md` ("Colours", `ui.due_format`).
