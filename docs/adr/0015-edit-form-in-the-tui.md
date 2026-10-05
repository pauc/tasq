# ADR-0015: Editing a task in a form inside the TUI (`e`)

- **Status:** Accepted
- **Date:** 2026-10-05

## Context

The TUI changes one field at a time: `t` and `p` are pickers, `l` and `d`
are one-line prompts, `c` takes a title. Changing a title, a due date, a
project or the topic tags meant `E` (the external editor on the raw
markdown) or the CLI. ADR-0011 deferred a multi-field form as "a new widget
kind for the TUI"; the 2026-10-05 key reshuffle (ADR-0014) freed `e` for it.

Two things stood in the way. The nb store could only write what the script
could: status, priority, a project (set, never cleared) and appends. A whole-
task `update` with a new title, due date or tag set came back as
`StoreError::Unsupported`, because `format::ops` had no operation for it
(the operations mirror the script's awk passes, and the script had none).
And `update` is pure and clock-free, while a due date typed as `today`
needs a date to resolve against.

## Decision

- **Store side: four rewrites of `tasq`'s own.** `format::ops` gains
  `set_title`, `set_due`, `clear_due`, `clear_project` and `set_tags`,
  with rules written in `docs/file-format.md` under "Edits the script never
  made": a title keeps its marker; a due date is written like a project
  path and a missing `## Due` goes after `## Project`, else after
  `## Description`, else after the title; clearing removes the whole
  section; topic tags go at the front of the first `#` line with status
  and priority tags left where they are. `store-nb/src/diff.rs` applies
  them, so `Store::update` (and therefore `tasq apply`) can now change the
  title, due, project (both ways) and tags. Description and the lists
  still cannot be rewritten; they are reported as before.
- **Core: `edit::Fields` and `edit::revise`.** `Fields { title, status,
  priority, due, project, tags }` is what the form shows (`Fields::of(&task)`)
  and what it saves. `revise(store, id, &fields)` refuses an empty title
  (the `tasq create` message), writes only when a field differs and returns
  the task as stored plus the changed field names, in form order, for the
  status line (`[13] updated: title, due`; `[13] unchanged`). FR-10 holds:
  the TUI calls core, and a later `tasq edit` command can call the same
  function.
- **TUI: `Mode::Form`, a new widget.** `e` on the selected task opens a
  bordered popup with six rows: Title, Status, Priority, Due, Project,
  Tags. One row has the focus. `Up`/`Down` and `Tab`/`Shift+Tab` move it;
  a text row (title, due, project, tags) takes characters, `Backspace` and
  paste; a choice row (status, priority) cycles with `Left`/`Right`. `Enter`
  saves from any row, `Esc` cancels, `Ctrl+C` quits. Like the other typing
  modes (ADR-0013) the form's keys are fixed, not in `[ui.keys]`.
  - Status offers the workflow's statuses and `none` (an open task without
    a status tag is legal and has its own group in the list).
  - Due takes what `tasq create --due` takes (`dates::parse_day`: ISO,
    `today`, `tomorrow`, `yesterday`); empty clears. The date it resolves
    against is `Model::today`, set by the CLI from the clock
    (`with_today`), so `update` stays pure.
  - Tags are space-separated, `#` optional, validated with `Tag::from_str`.
    Project is a path, empty clears; it is not checked for existence.
  - A bad due date, a bad tag or an empty title keeps the form open with
    the focus on the offending row and the error in the status bar.
- **Save is `Cmd::Revise(TaskId, Box<Fields>)`**, run by `dispatch` through
  `edit::revise` like every other edit; a `Conflict` or `Unsupported` from
  the store is shown as it is. No hook follows an edit and `Host` is
  unchanged.
- **Keys.** `e` is the form; the external editor stays on `E`. In
  `[ui.keys]` the action `edit` now means the form and the external editor
  is the action `editor`. `t` and `p` stay as quick pickers.

## Consequences

### Positive

- One screen edits everything the list shows about a task, without
  leaving the TUI or touching raw markdown.
- `tasq apply` gains the same fields for free, and the store's "cannot
  express" set shrinks to description and the lists.
- The rules for the new rewrites are written down before any code depends
  on them, and tested like the script's (`fixtures/ops` byte pairs).

### Negative

- Five format operations with no script to check against; their shape is
  a convention, pinned by fixtures and the spec text.
- `[ui.keys]`: `edit` changes meaning and `editor` is new. ADR-0013 is a
  day old and unreleased, so no configuration exists to break.
- `Model` carries one more field (`today`) that every front end sets.
- The form is a third input widget (after the prompts and the pickers)
  with its own key handling and rendering to keep in step with the others.

## Alternatives considered

- **More one-field prompts (`T` title, `D` due, ...)** — four more keys
  and four more prompts for what is one edit; the key surface is kept
  minimal on purpose.
- **Only the external editor (`E`)** — already there, but it leaves the
  TUI and edits markdown by hand; it stays for the description and the
  lists.
- **Description and a progress note in the form** — the description is
  multi-line (the editor's job) and a note is `l`; the form stays single-
  line per row.
- **Configurable form keys** — the pickers and prompts are not
  configurable either; the form's keys are the conventional ones.
- **Resolving `today` in the runtime** — would close the form before the
  error is known; a bad date must keep the form open, so the parse happens
  in `update` with a date the model carries.

## References

- ADR-0011 (deferred the form), ADR-0013 (fixed typing modes, `[ui.keys]`),
  ADR-0014 (freed `e`).
- `crates/core/src/format/ops.rs` (`set_title`, `set_due`, `clear_due`,
  `clear_project`, `set_tags`), `crates/store-nb/src/diff.rs`,
  `crates/core/src/edit.rs` (`Fields`, `revise`), `docs/file-format.md`
  ("Edits the script never made"), `crates/tui/src/form.rs`,
  `crates/tui/src/update.rs` (`form`), `crates/tui/src/view.rs`
  (`render_form`).
