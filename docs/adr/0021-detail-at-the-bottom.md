# ADR-0021: The TUI detail under the list (`ui.detail_position`)

- **Status:** Accepted
- **Date:** 2026-10-07

## Context

`Right` (or `Tab`) shows the selected task's detail beside the list from 100 columns
(T-803) and instead of the list below that. In a tall, narrow pane (a herdr or tmux split,
a vertical monitor) the side-by-side split never happens, so the list disappears every time
the detail is shown. The user asked for the detail under the list, as a setting.

## Decision

`ui.detail_position` (`TASQ_DETAIL_POSITION`) is `right` (the default, unchanged) or
`bottom`. With `bottom`, `Model::layout` returns `LayoutKind::Stacked` from
`STACKED_MIN_HEIGHT` (20) rows whatever the width, and the view splits the screen above the
status bar in two halves: the list on top, the detail under it. Below 20 rows it falls back
to the one-pane layout, as `right` does below 100 columns. Keys do not change: `Right`
shows, `Left`/`Esc` hide, `Tab` toggles.

## Consequences

### Positive

- The list stays visible with the detail open in narrow panes.
- One setting, read by the TUI only; the CLI and `--json` are untouched.

### Negative

- Half the height goes to the detail; long detail text is clipped, as on the right.

## Alternatives considered

- **Choose by aspect ratio automatically** — rejected: a 120x40 pane fits either layout and
  the choice is taste, so it is a setting rather than a guess.
- **A key to switch the position at runtime** — not now; the setting can be overridden per
  run with `TASQ_DETAIL_POSITION=bottom tasq ui`.

## References

- T-803 (layout), `crates/tui/src/model.rs` (`Model::layout`), `docs/config.md`.
