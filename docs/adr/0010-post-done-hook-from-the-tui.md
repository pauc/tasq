# ADR-0010: The TUI's close reaches the `post-done` hooks through the `Host`

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

ADR-0006 made `[hooks]` command lines that the CLI runs around its events:
`post-create`, `post-done`, `pre-launch`. ADR-0009 gave the TUI a `Host`
trait with three methods for what needs the outside world (`edit`,
`launch`, `sync`), and kept every edit, the `d` key included, on the
in-process `tasq_core::edit` path. The two together left a gap that both
ADRs record as a known negative: `tasq done` fires `post-done`, the TUI's
`d` key does not, because the hook runner lives in the CLI crate and the
TUI may depend on `tasq-core` only.

Two more facts matter. The hook runner reports a failing `post-*` command
with a warning on stderr; inside the alternate screen that line would be
drawn over the UI. And a `post-done` hook never blocks: by the time it
runs the task is closed, so the TUI has nothing to undo, only something
to show.

## Decision

- `Host` gains a fourth method, `after_done(&Task) -> Result<(), String>`,
  called by the TUI's `dispatch` right after `edit::done` succeeded, with
  the task as written (`done: true`, the final note). `Ok` means nothing
  to report; `Err` is a warning, shown in the status bar appended to the
  usual `[id] done: title` line in the failure style. The close itself is
  not affected by the result. `NoHost` answers `Ok(())`.
- The CLI's `CliHost` holds a reference to the `App` and implements
  `after_done` by running the same `post-done` hooks as `tasq done`, with
  the same document and environment, **in-process** (no child `tasq`):
  the hooks already run as their own child processes with piped stdio,
  so there is no terminal to release and nothing to pause for.
- The hook runner is split in two: `run_hooks_with` takes a reporting
  callback and `run_hooks` (unchanged behaviour for every CLI command)
  passes one that prints at `-v` and warns on stderr. The TUI host's
  callback collects the warnings and drops the stdout lines, which have
  nowhere to go in the UI.

## Consequences

### Positive

- `d` and `tasq done` are observably the same event to a hook: one
  document shape, one environment, one failure policy.
- The `d` key stays instant and in-process; no screen clear, no pause,
  unlike `Enter` and `S`.
- The dependency rule of ADR-0009 holds: the TUI still knows nothing of
  hooks or configuration, only that a host may want a word after a close.
- The hook runner's reporting is now injectable, which is what made it
  testable from a unit test with a hand-built `App` and `/bin/sh` hooks.

### Negative

- `Host` is four methods, not the three ADR-0009 states; every host
  implementation (today two doubles and the CLI) carries one more method.
- A hook's stdout is invisible from the TUI even at `-v`; a hook that
  wants to be heard from the UI must exit non-zero.
- Several failing hooks share one status-bar line, joined with `; `.
- `post-create` still has no TUI counterpart (the UI cannot create), and
  `tasq apply`/`tasq sync` closing a task still fire nothing, as before.

## Alternatives considered

- **Run `tasq done <id> [note]` as a child, like `pick` and `sync`** — one
  code path with the CLI, but it moves the only in-process edit out of
  `tasq_core::edit`, against FR-10 and the point of ADR-0009, and makes
  every `d` release the terminal and pause for Enter so the child's output
  can be read.
- **Teach `tasq-core` about hooks** — hooks spawn processes; the core has
  no I/O and no process spawning by design (ADR-0003, ADR-0009).
- **A generic `after_edit(kind, task)` method** — `post-done` is the only
  post-edit hook that exists; a kind parameter would be speculation.

## References

- ADR-0006 (hooks, and its "Hooks run in the CLI only" negative),
  ADR-0009 (`Host`, "three methods").
- `crates/tui/src/msg.rs` (`Host::after_done`), `crates/tui/src/runtime.rs`
  (`close`), `crates/cli/src/commands/ui.rs` (`CliHost`),
  `crates/cli/src/plugins.rs` (`run_hooks_with`, `HookEvent`),
  `docs/plugins.md`.
