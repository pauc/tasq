# ADR-0029: `--window`, not `--detached`, for a session in a new window

- **Status:** Accepted
- **Date:** 2026-10-20

## Context

ADR-0012 named the "open the session in a new window" flag `--detached`,
with its launcher under `launch.detached` and the TUI actions
`launch-detached` and `launch-detached-stay`. The name says the wrong
thing. In tmux, "detached" means created in the background, without
switching to it, but `--detached` switches to the new window by default.
Only `--detached --no-focus` behaves as the word says. The flag is about
*where* the session opens; focus is `--no-focus`'s job.

Nothing with the old name has been released yet (the CHANGELOG still
lists it under Unreleased).

## Decision

The concept is named after what gets created, a window:

| Was | Is |
|---|---|
| `tasq pick\|next --detached` | `tasq pick\|next --window` (`-w`) |
| `launch.detached` | `launch.window` |
| `TASQ_LAUNCH_DETACHED` | `TASQ_LAUNCH_WINDOW` |
| `[ui.keys]` `launch-detached`, `launch-detached-stay` | `launch-window`, `launch-window-stay` |
| `LaunchTarget::Detached { focus }`, `Msg::LaunchDetached` | `LaunchTarget::Window { focus }`, `Msg::LaunchWindow` |
| `resolve_detached`, `DETACHED_LAUNCHER_NAMES` | `resolve_window`, `WINDOW_LAUNCHER_NAMES` |

"Window" is tmux's word and a generic one for herdr, whose workspace or
tab is still chosen by `launch.herdr.placement`. `--no-focus` is unchanged
and still requires `--window`. The old names are removed outright, with no
hidden alias: no release carries them.

Everything else in ADR-0012 stands.

## Consequences

### Positive

- The flag and the focus behaviour are independent words: `--window`
  says where, `--no-focus` says whether you follow.
- `-w` makes the common case short: `tasq pick 219 -w`.

### Negative

- A local config or `[ui.keys]` table that set `launch.detached` or
  `launch-detached` now fails to load with the unknown key named; the fix
  is the rename above.

## Alternatives considered

- **`--away`** — short, but reads as "send it away and stay here", which
  is the `--no-focus` behaviour, not the default one.
- **`--new-window`** — the most explicit, too long for a daily flag.
- **`--aside`, `--elsewhere`** — say nothing about what gets created.
- **Keep `--detached` as a hidden alias** — no release has it, so there
  is no user to keep working.

## References

- ADR-0012 (sessions in a new window from the TUI), ADR-0013 (key bindings).
- `crates/cli/src/cli.rs` (`Next`, `Pick`), `crates/launch/src/registry.rs`
  (`resolve_window`), `crates/tui/src/msg.rs` (`LaunchTarget`).
