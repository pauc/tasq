# ADR-0018: Named colour themes for the CLI and the TUI

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

T-803 made the colours of the status groups configurable under
`[ui.colors]`, keyed by status name, and put the decision of which group
gets which colour in `tasq_core::theme` so `tasq list` and `tasq ui`
agree. Everything else stayed where the original script had it: the tag
chip (palette 231 on 24), the red `#A`, the cyan focus border of the edit
view, the reversed selection, the dim weekends of the calendar, the red
error line and the light blue underlined links of `tasq view` were
literals in `crates/tui/src/view.rs`, `crates/cli/src/output.rs` and
`crates/cli/src/commands/view.rs`. Two problems came out of the
2026-10-06 UX review:

- Dim text is the `faint` attribute, which many light-background
  terminals render as unreadable light grey. There was no way to swap it
  for a real colour.
- A user who wants a palette (solarized, gruvbox) or no colours beyond
  bold and reverse had to set every status and still could not touch the
  chip, the border or the links.

The constraints of T-803 hold: the core decides colours and holds no
rendering code, the two front ends turn a `Color` into escape codes or
ratatui styles, `NO_COLOR` and `--color never` turn colours off but keep
the attributes, and `[ui.colors]` keeps working as it is.

## Decision

- **Every coloured thing is a `Role`.** `tasq_core::theme::Role` names
  seventeen of them: the eight groups (`in-progress`, `ready`, `waiting`,
  `blocked`, `later`, `other-status`, `no-status`, `done`), `chip-bg`,
  `chip-fg`, `prio-a`, `focus`, `selection`, `error`, `dim`, `link` and
  `header`. The front ends ask `Theme::color(role)` and never name a colour
  themselves.
- **A `Color` can be an attribute.** Besides the eight ANSI names and the
  256 palette indices, `dim`, `reversed` and `none` are colours, so a role
  can say "faint", "swap the colours" or "leave it alone". `dim` and
  `reversed` survive `--color never`; the rest become plain. This is what
  lets `selection = "reversed"` and `chip-bg = "reversed"` be table
  entries rather than special cases.
- **Three layers under `[ui]`.** `ui.theme.preset` picks a built-in
  table: `dark` (the script's colours, the default), `light` (darker
  shades, grey 245 instead of faint), `solarized`, `gruvbox` (256-colour
  approximations of the dark variants) and `mono` (attributes only).
  `[ui.theme.colors]` overrides single roles. `[ui.colors]` stays on top,
  keyed by status name, so it still colours a status the roles do not know
  (`review = "red"`). The preset is a serde enum, so a typo is a config
  error with a file position; an unknown role name or colour value is
  ignored like an unknown `[ui.colors]` value was. `TASQ_THEME` sets the
  preset from the environment.
- **The front ends decide how a role applies.** The TUI uses `selection`
  as a background (reversed when the colour is an attribute or colours
  are off), `chip-bg` as a background under `chip-fg`, `header` with bold
  added, and falls back to the faint attribute for `dim` when colours are
  off so a light theme's grey does not come out plain. The CLI opens a
  link with `<colour>;4` and closes it with the reset of exactly those
  parameters (`24;39`, `24;22` for `dim`, `24;27` for `reversed`), so glow's
  own styling around the link survives.

## Consequences

### Positive

- One setting (`ui.theme.preset = "light"`) fixes a light terminal for
  `tasq list`, `tasq view` and `tasq ui` at once.
- The dark default renders byte for byte what the script and T-803
  rendered; every existing colour snapshot passed unchanged (only
  `tasq config show` grew the new `[ui.theme]` table).
- The preset tables are data with a pinned test each, so a colour change
  is a one-line diff and a one-line test diff.

### Negative

- Seventeen roles is more surface than `[ui.colors]` had. Mitigated by
  the presets: most users will never write `[ui.theme.colors]`.
- `ui.theme` is a table, not the string the task first imagined
  (`ui.theme = "light"`), because `[ui.theme.colors]` must live under it
  and a key cannot be both a string and a table across layers.
  `TASQ_THEME=light` and `--set ui.theme.preset=light` are the short
  forms.
- The `doctor` verdict colours (green/yellow/red) are not roles; they are
  a check report, not task styling, and stay as they are.

## Alternatives considered

- **`ui.theme = "light"` plus role names inside `[ui.colors]`** — one
  table for statuses and roles; a status called `focus` or `dim` would be
  ambiguous, and the deep merge of layers cannot combine a string and a
  table under the same key.
- **A theme file (`~/.config/tasq/themes/<name>.toml`)** — more machinery
  for the same five tables; `[ui.theme.colors]` in a profile does the job
  until someone ships a theme worth sharing.
- **True-colour (`#rrggbb`) values** — not every terminal supports them
  and ratatui's `TestBackend` cells would need a second representation;
  the 256 palette covers the presets.

## References

- Plan: T-803 (`ruli/features/rust-rewrite/PLAN.md`); nb todo 15,
  "named color themes for the CLI and TUI (ui.theme)".
- Code: `crates/core/src/theme.rs`, `crates/core/src/config/mod.rs`
  (`ThemeConfig`), `crates/tui/src/view.rs`, `crates/cli/src/output.rs`,
  `crates/cli/src/commands/{list,view}.rs`.
- ADR-0013 (configurable key bindings) for the `[ui.<table>]` shape;
  ADR-0017 for the calendar whose weekends the `dim` role colours.
