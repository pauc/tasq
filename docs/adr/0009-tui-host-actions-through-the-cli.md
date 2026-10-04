# ADR-0009: TUI edits through core, host actions through the CLI binary

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

The plan's Phase 8 asks for a ratatui TUI that depends on `tasq-core` only
(ADR-0005: the second consumer proves the core/UI boundary) and whose
every edit goes through the same functions the CLI uses (FR-10). Three of
its actions need the outside world, which the core does not have: open the
task's file in `$EDITOR`, start a work session (`next`/`pick`, the
`tasq-launch` crate), and refresh from the sources (`sync`, the
`tasq-sources` crate).

Two facts shape the decision. First, the `set`/`log`/`done` logic lived in
the CLI's command modules (read the task, change a field, log the note,
write the whole task back; log first, then `set_done`), so a TUI could only
reuse it by depending on the CLI crate or by copying it. Second, the
`claude` and `shell` launchers replace the process with `exec`; a TUI that
called them in-process would never return to the screen.

## Decision

- The edit operations move into the core as `tasq_core::edit` (`Value`,
  `set`, `log`, `done`), taking `&mut dyn Store` and `&dyn Clock`. The CLI
  commands and the TUI's `s`, `p`, `l` and `d` keys call them; neither
  front end holds edit logic of its own.
- The TUI receives the outside world as a `Host` trait with three methods
  (`edit(id, file)`, `launch(id)`, `sync()`), plus `Store::file_of(id)` so
  a store can say which file holds a task. The TUI releases the terminal
  (leaves raw mode and the alternate screen) around every host call and
  takes it back afterwards.
- The CLI's `Host` runs **the `tasq` binary itself** as a child process:
  `tasq pick <id>` and `tasq sync`, with the `--profile`, `--config` and
  `--set` flags passed on, and `$VISUAL`/`$EDITOR`/`vi` for the editor. The
  TUI crate does not depend on `tasq-launch` or `tasq-sources`.
- Colour semantics (`Color`, the status colour table, `[ui.colors]`
  overrides) move into `tasq_core::theme`; the CLI maps them to SGR codes,
  the TUI to ratatui styles. An in-memory `Store` (`MemoryStore`) lives in
  core as the test double of every front end.

## Consequences

### Positive

- The dependency rule holds exactly: `tasq-tui` → `tasq-core` → nothing.
  A different UI (or a plugin host) gets the same `Host` seam.
- A session started from the TUI is literally `tasq pick`: the in-progress
  transition, the worktree resolution, the launcher selection, the
  `direnv` wrapping and the prompt are one code path with one set of tests.
  The `exec` launchers work unchanged because the exec happens in the child.
- The TUI's logic is testable without a terminal: `update` is pure,
  `dispatch` runs against `MemoryStore` and a recording host, rendering is
  snapshotted with `TestBackend`. Only the event loop and the mode
  switching are `#[mutants::skip]`.
- The CLI commands lost code rather than gaining a twin.

### Negative

- A child process per session or sync costs a process start and re-reads
  the configuration. Negligible against a Claude session or a network
  sweep; the in-process `set`/`log`/`done` path is untouched.
- The child's output goes to the released terminal, so the TUI pauses for
  Enter after `pick` and `sync` before redrawing. Accepted as the usual
  TUI behaviour for external commands.
- A `tasq` binary that is renamed or moved while the TUI runs breaks the
  child calls; `std::env::current_exe` is resolved once at start and the
  failure is shown in the status bar.

## Alternatives considered

- **Link `tasq-launch` and `tasq-sources` into the TUI** — would need
  non-exec variants of every launcher and a second wiring of the sources
  (config → transport → registry) outside the CLI; breaks the core-only
  rule the TUI exists to prove.
- **A fourth core trait for "actions"** — the actions are not domain
  concepts; they are what a shell does. A three-method `Host` in the TUI
  crate is the smallest seam that keeps them out of core.
- **Leave the edit logic in the CLI and depend on it** — a UI depending on
  a binary's crate inverts the layering, and `tasq_cli` pulls in every
  adapter.

## References

- Plan Phase 8 (T-801 to T-803), FR-10; ADR-0005.
- `crates/core/src/edit.rs`, `crates/core/src/theme.rs`,
  `crates/core/src/store.rs` (`MemoryStore`, `Store::file_of`),
  `crates/tui/src/msg.rs` (`Host`), `crates/cli/src/commands/ui.rs`.
