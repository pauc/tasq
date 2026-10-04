# ADR-0011: Creating a task from the TUI: title only, through the `Store`, hooks through the `Host`

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

Until now the TUI could change, log, close, open and launch tasks but not
create one; the user had to leave for `tasq create`. ADR-0010 listed that
as a known negative ("`post-create` still has no TUI counterpart"). The
pieces are all there: `Store::create(TaskDraft)` is what `tasq create`
calls, `TaskDraft::new(title)` carries the script's defaults (priority
`B`, open, no note), and the CLI picks the initial status from
`workflow.default_status`.

Two constraints from earlier ADRs hold. The TUI depends on `tasq-core`
only (ADR-0005, ADR-0009), so it cannot run the hook runner or read the
configuration file. And every write the TUI makes is in-process through
the core (ADR-0009), not a child `tasq`.

`tasq create` takes nine optional flags (status, priority, due, project,
tags, related links, merge requests, description, note). A form with
nine fields is a different UI from the one-field-at-a-time prompts the
TUI has (`/`, `l`, `d`), and the status and priority pickers already
exist for a task once it is on screen.

## Decision

- `c` opens a one-line prompt for the **title only**. Enter writes
  `Model::draft(title)`: `TaskDraft::new(title)` with the status set to
  `Model::default_status`, which the CLI fills from
  `workflow.default_status` (the model defaults to `ready`, the script's
  default). Everything else keeps the draft's defaults; the user refines
  with `s`, `p` and `e`, or with the CLI. An empty title is refused with
  the same message as `tasq create`, and the prompt stays open.
- The write is `Cmd::Create(Box<TaskDraft>)`, run by `dispatch` against
  the injected `Store`, like every other edit. After the write `dispatch`
  reports `[id] created: title`, reloads, and sends a new `Msg::Select(id)`
  so the new task is selected when it is visible (a filter that hides it
  leaves the selection alone).
- `Host` gains a fifth method, `after_create(&Task) -> Result<(), String>`,
  with the exact contract of `after_done` (ADR-0010): called after a
  successful write, `Err` is a warning for the status bar, the task exists
  either way. The CLI's `CliHost` runs the `post-create` hooks in-process
  through the same `run_hooks_with` path, so `c` and `tasq create` are the
  same event to a hook.

## Consequences

### Positive

- The last everyday command missing from the TUI is there, with no new
  logic: one store call, one message, one host method.
- `post-create` hooks see one document shape and one environment from
  both front ends, as `post-done` already did.
- The core/UI boundary is unchanged: the TUI still knows nothing about
  hooks or configuration beyond the one status it is handed.

### Negative

- `Host` is five methods. Every host (the CLI's and the two test doubles)
  carries one more.
- A task created from the TUI has no due date, project, tags, links or
  first note until a second step; `tasq create` remains the way to set
  them in one go. A richer prompt can be added later without changing
  the command or the host method.
- `Cmd` grew to hold a `TaskDraft`, which is large; it is boxed to keep
  the enum small (clippy's `large_enum_variant`).

## Alternatives considered

- **Run `tasq create <title>` as a child, like `pick` and `sync`** — one
  code path with the CLI, but it moves a write out of the in-process
  store path (against ADR-0009), and every `c` would release the terminal
  and pause for Enter.
- **A multi-field create form** — the right UI for the full flag set, but
  a new widget kind for the TUI and a second place where status and
  priority are chosen; the existing pickers do that already once the task
  exists.
- **Reuse `after_done` with a kind parameter** — ADR-0010 rejected a
  generic `after_edit(kind, task)` as speculation; with two events it is
  still clearer to have two methods with two names than one method and
  an enum.

## References

- ADR-0009 (`Host`, in-process edits), ADR-0010 (`after_done`,
  `run_hooks_with`), whose "`post-create` still has no TUI counterpart"
  negative this ADR resolves.
- `crates/tui/src/model.rs` (`Mode::Create`, `Model::draft`,
  `with_default_status`), `crates/tui/src/update.rs` (`create`),
  `crates/tui/src/runtime.rs` (`create`, `Msg::Select`),
  `crates/tui/src/msg.rs` (`Host::after_create`),
  `crates/cli/src/commands/ui.rs` (`CliHost::hooks`), `docs/plugins.md`.
