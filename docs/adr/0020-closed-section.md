# ADR-0020: Recording when a task was closed (`## Closed`)

- **Status:** Accepted
- **Date:** 2026-10-07

## Context

The TUI is getting a DONE group on demand (todo 33), listed newest first, so that "what did
I close this week" and reopening a task no longer need the CLI. Nothing in a task file says
when it was closed: `nb todo do` and the original script only flip `# [ ]` to `# [x]`, and
the optional note of `tasks done` is an ordinary progress entry. Ordering by the last
progress entry is wrong for any task closed without a note, and the file mtime moves with
every later edit (and with `git checkout`).

Constraints: the file stays readable and writable by nb and the original script (ADR 0002),
unknown sections are preserved byte for byte, the core stays pure (time comes from the
`Clock`), and `docs/json.md` is a contract (`tasq apply` must keep reading older documents).

## Decision

`Task` gains `closed_at: Option<NaiveDateTime>`, local time to the minute. The file keeps it
in a new optional section written only by tasq, read like `## Due` (first non-empty line):

```
## Closed

2026-10-07 14:32
```

- **Reading.** A `YYYY-MM-DD HH:MM` line gives `closed_at`; a bare date or anything else
  gives `None` and stays in the document untouched. An open task reads `None` whatever the
  file says, the same way a done task reads no status.
- **Writing.** `Task::close(clock)` marks the task done and stamps `closed_at` unless it was
  already done. `edit::done` (CLI `tasq done`, TUI `d`) logs the note and closes the task in
  one `Store::update`; sync's `Close` change uses the same call, and a draft created done is
  stamped at creation. `edit::reopen` drops `closed_at`.
- **Placement.** `ops::set_closed` replaces an existing section, or inserts one after
  `## Source`, else `## Due`, else `## Project`, else `## Description`, else after the title
  line; `Document::from_task` writes it after `## Source` on done tasks. `ops::clear_closed`
  removes it. The nb store's `update` diffs `closed_at` like any other field, and
  `set_done(id, false)` also drops the section.
- **`Store::set_done` is unchanged.** It has no clock, and it keeps writing exactly what
  `nb todo do` writes (the nb parity tests).
- **JSON.** `closed_at` is `null` or `"YYYY-MM-DD HH:MM"`, last in the `Task` object, and
  optional on input.

## Consequences

### Positive

- The DONE group (and later `tasq list --done`) can sort by the real closing time.
- The time is visible when the file is opened in nb or an editor, and hand-editable.
- `tasq done` makes one write and one git checkpoint instead of two when given a note.

### Negative

- Tasks closed before this change, or closed by `nb todo do` or the original script, have no
  `closed_at`. Consumers fall back to the last progress entry. A backfill from git history is
  possible as a separate one-off command and is not part of this decision.
- A `nb todo undo` leaves a stale `## Closed` in an open file. It reads as `None` and is
  removed the next time tasq closes or reopens the task through `update`; until then it is
  harmless text.

## Alternatives considered

- **`<!-- tasq: {...} -->` metadata comment** — rejected for now: its shape and placement are
  still TBD in `docs/file-format.md`, and a hidden timestamp cannot be checked or fixed by
  hand.
- **A `closed` progress entry on every `done`** — rejected: it adds noise to Progress and to
  `tasq summary`, and a user note on the same minute makes the two indistinguishable.
- **File mtime or git history at read time** — rejected: mtime moves on any edit, and reads
  never spawn a process (`git log` per file would).
- **Pass a timestamp to `Store::set_done`** — rejected: it changes the trait for every store,
  while `update` already carries whole tasks.

## References

- Todo 33 (TUI DONE group and Today view); ADR 0002 (nb-compatible store); `docs/file-format.md`.
- `crates/core/src/model/task.rs` (`Task::close`), `crates/core/src/format/ops.rs`
  (`set_closed`, `clear_closed`), `crates/store-nb/src/diff.rs`.
