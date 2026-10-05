# ADR-0016: The edit view takes the whole screen and edits the description

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

ADR-0015 put the edit form in a popup over the list, six single-line rows,
`Enter` to save. Used once, the author rejected the popup: a small floating
box in the middle of the window, when the task's description, the one field
that needs room, was still only reachable through the external editor. The
bar he set is the one terminal tools like lazygit, broot and hunk clear: a
view of its own, a cursor, a key bar that says what the keys do here.

Two constraints from ADR-0015 hold: the TUI depends on `tasq-core` and
ratatui only, and `update` is pure. A text area with a cursor is a small
editor; writing one in the crate keeps both, and keeps it under mutation
testing. The store had no operation for the description either, the same
gap ADR-0015 closed for the title, due date and tags.

## Decision

- **A full-screen edit view.** `e` replaces the list and the detail with
  one bordered panel: the six single-line rows at the top (Title, Status,
  Priority, Due, Project, Tags), a rule labelled `Description`, and the
  description below it taking the rest of the height. The focused row's
  label is highlighted (cyan, bold); the terminal cursor sits in the
  focused text, so the terminal shows it the way it shows every editor's;
  a focused choice row shows its value between chevrons. Long single-line
  values scroll horizontally under the cursor; the description wraps at
  the width (by character) and scrolls vertically to keep the cursor in
  view.
- **An editor of the crate's own: `form::Text`.** Lines and a cursor
  (line, character); insert, newline, backspace, delete, the four arrows,
  Home and End, paste. The single-line rows are a `Text` that never gets a
  newline (the `Form` refuses it and flattens pasted line breaks); the
  description keeps them.
- **Keys, fixed as in every typing mode.** `Tab`/`Shift+Tab` move between
  rows; `Up`/`Down` move the cursor inside the description and otherwise
  move between rows (leaving the description upwards from its first
  line); `Left`/`Right` move the cursor in a text row and cycle a choice
  row; `Home`/`End`; `Enter` is a newline in the description and the next
  row elsewhere; **`Ctrl+S` saves**, `Esc` cancels, `Ctrl+C` quits. Enter
  had to give up saving once it meant a newline somewhere; one key for
  saving everywhere is clearer than two.
- **The status bar is a key bar** while the view is open: each key in
  bold with what it does in dim, labels dropped when the terminal is too
  narrow for them. An error replaces it, as before, and the focus moves to
  the bad row.
- **Store and core.** `format::ops::set_description` and
  `clear_description` (rules in `docs/file-format.md`: the first
  `## Description` is rewritten, further ones removed, a missing one
  inserted right after the title, blank lines trimmed at both ends because
  the projection trims them); `diff.rs` applies them; `edit::Fields` gains
  `description` and `revise` writes it. `tasq apply` can change a
  description too.

## Consequences

### Positive

- Everything the file holds about a task except the lists is edited in
  one place, with room for the description and a real cursor.
- The view is a template for later full-screen views (the detail of a
  task, a sync report): one panel, highlighted focus, a key bar.
- `Text` is 150 lines of pure code with exhaustive unit tests and no
  dependency; the wrapping and the cursor mapping are pure functions in
  `view.rs` the snapshots and cursor assertions pin.

### Negative

- Character wrapping, not word wrapping: a word can break at the edge. Good
  enough for an editor whose lines are usually short; word wrapping would
  need the cursor mapping to follow it.
- `Esc` discards without asking, even a long description. The external
  editor (`E`) remains the safer place for a long rewrite.
- ADR-0015's view section is superseded; its store and core decisions
  stand.

## Alternatives considered

- **Keep the popup and add a description area to it** — the box would have
  to be most of the screen anyway, and a popup suggests a quick choice,
  not an editing session.
- **A text-area crate (`tui-textarea`)** — a dependency for 150 lines, with
  its own key map to reconcile with ours.
- **`Enter` saves on single-line rows, newline only in the description**
  — two meanings for one key, and a saved form when the user meant to
  move on.
- **Word wrapping** — see Negative; deferred until someone asks.

## References

- ADR-0015 (superseded in its view section; the store and core parts
  stand), ADR-0013 (fixed typing modes).
- `crates/tui/src/form.rs` (`Text`, `Form`), `crates/tui/src/view.rs`
  (`render_form`, `wrapped`, `wrapped_cursor`, `window`, `form_hints`),
  `crates/tui/src/keys.rs` (`form`), `crates/core/src/format/ops.rs`
  (`set_description`, `clear_description`), `docs/file-format.md`.
