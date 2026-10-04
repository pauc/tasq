# ADR-0012: Opening a work session in a new window from the TUI

- **Status:** Accepted
- **Date:** 2026-10-05

## Context

`Enter` in the TUI runs `tasq pick <id>` in the TUI's own terminal: the
alternate screen is left, the launcher takes the pane, and the UI comes
back when the session ends (ADR-0009). With `launch.default = "claude"`
that is Claude Code in the pane the TUI was in. The author works in herdr,
where a session belongs in its own workspace, and wants to decide per
launch whether to leave the TUI for it or keep browsing: open the task in
a new window and switch to it, or open it in the background and pick the
next one.

The herdr launcher already opens a window and moves focus to it, with one
rule for what the window is (a tab when a workspace already holds the
directory, else a workspace) that the user cannot change. The tmux
launcher opens a window and always selects it. Neither knows about focus
as a choice.

ADR-0006 settled where herdr-specific code lives: in-process Rust behind
the `herdr` cargo feature, not a plugin. A plugin could add neither a TUI
key nor a `--launcher` value.

## Decision

- Three keys, three intents. `Enter` opens the session **here**, as
  before, with `launch.default`. `Ctrl+Enter` opens it in a **new window
  and switches to it**. `Shift+Enter` opens it in a **new window and
  stays**. The TUI expresses this as `LaunchTarget::{Here, Detached {
  focus }}` on `Cmd::Launch` and `Host::launch`; it never names herdr or
  tmux.
- The CLI grows `tasq pick|next --detached [--no-focus]`. `--detached`
  selects the launcher from a new key, `launch.detached` (default `auto`:
  `herdr` when `HERDR_ENV` is set, else `tmux` when `TMUX` is set, else
  an error naming the key), instead of `launch.default`. A launcher that
  takes over the current terminal (`claude`, `shell`) is refused for
  `launch.detached`. The resolution happens before the task is set to
  in-progress, so a refused detached launch writes nothing.
- `LaunchContext` gains `focus: bool`. The herdr launcher skips its three
  focus calls when it is false; the tmux launcher passes `-d` to
  `new-window`. In-pane launchers ignore it.
- What a herdr window is becomes configuration, `launch.herdr.placement`:
  `auto` (the existing rule), `workspace` (always a new workspace), `tab`
  (a tab in the holding workspace, else in the current one,
  `HERDR_WORKSPACE_ID`). The keys choose focus per launch; the config
  chooses the shape once.
- For a detached target the CLI host runs `tasq pick <id> --detached
  [--no-focus]` with its output captured instead of handing it the
  terminal: the UI keeps the screen, and the child's last stdout line
  (the launcher's "Opened herdr workspace ..." outcome) or last stderr
  line (the CLI's error, `tasq: error: ` stripped) lands in the status
  bar. `Cmd::releases_terminal` and `pauses_after` are false for it.
- The runtime asks the terminal for the kitty keyboard protocol
  (`PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES)`) when
  `supports_keyboard_enhancement()` says yes, and pops it on every exit
  path. Without the protocol `Ctrl+Enter` and `Shift+Enter` are
  indistinguishable from `Enter` and behave as such. herdr 0.9.3 answers
  the `CSI ? u` query, so it supports the protocol.

## Consequences

### Positive

- The TUI/CLI boundary stays generic: one target enum and two flags, no
  window-manager name outside `tasq-launch`. tmux gets the same keys for
  free.
- `tasq pick --detached --no-focus` is useful from a shell and from
  plugins too, and `--dry-run` shows the focus step being skipped.
- The `auto` placement keeps today's behaviour for everyone who did not
  set anything; the author sets `placement = "workspace"` once.

### Negative

- Two more config keys (`launch.detached`, `launch.herdr.placement`) and
  two more env variables (`TASQ_LAUNCH_DETACHED`, `TASQ_HERDR_PLACEMENT`).
- The two chords depend on the terminal. In one without the kitty
  protocol they silently do what `Enter` does; the help overlay and
  `tasq help ui` say so, but there is no runtime warning.
- `Host::launch` and `RecordingHost` carry a second argument; every host
  changes.

## Alternatives considered

- **Make herdr a plugin** — rejected by ADR-0006 for in-repo adapters, and
  a plugin cannot bind a key or register a launcher name without a new
  extension point.
- **Keys that choose the placement (`Ctrl+Enter` workspace, `Alt+Enter`
  tab, with Shift variants)** — four bindings that only make sense in
  herdr; the placement is a preference, not a per-launch decision, so it
  went to the config.
- **Resolve a detached `auto` to `claude` in the current pane, like
  `launch.default`** — defeats the key's purpose; an error is more honest.
- **Letter keys (`o`/`O`) instead of `Ctrl+Enter`/`Shift+Enter`** — work
  in every terminal, but the author asked for the chords and herdr
  supports them; the letters stay available if another terminal needs
  them.

## References

- ADR-0006 (plugins vs built-in adapters), ADR-0009 (host actions through
  the CLI binary).
- `crates/tui/src/msg.rs` (`LaunchTarget`), `crates/tui/src/keys.rs`,
  `crates/tui/src/runtime.rs` (keyboard enhancement),
  `crates/cli/src/commands/launch.rs` (`How`),
  `crates/launch/src/registry.rs` (`resolve_detached`),
  `crates/launch/src/herdr.rs` (`Placement`), `docs/config.md`
  ("Detached sessions").
