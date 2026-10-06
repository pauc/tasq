# Architecture decision records

An architecture decision record (ADR) captures one significant design decision:
the problem, the option chosen, why, and what it costs. ADRs are written when the
decision is taken and are never edited to say something else afterwards. When a
decision changes, a new ADR supersedes the old one and the old one keeps its text.

## Status vocabulary

| Status       | Meaning                                                                 |
|--------------|-------------------------------------------------------------------------|
| Proposed     | Under discussion. Lists the options; the decision is not final.         |
| Accepted     | In force. Code is expected to follow it.                                |
| Superseded   | Replaced by a later ADR. The record names its successor and stays as history. |

## Adding an ADR

1. Copy `template.md` to `NNNN-short-slug.md`, where `NNNN` is the next free number.
2. Fill every section. Keep it under about 120 lines; link to the plan or code for detail.
3. Set the status (`Proposed` when options are still open, `Accepted` otherwise) and the date.
4. Add a row to the index below.
5. When an ADR supersedes another, set the old one to `Superseded by ADR-NNNN` and
   list the old one under the new one's References.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-rust.md) | Rust as implementation language | Accepted |
| [0002](0002-nb-compatible-markdown-store.md) | nb-compatible markdown files as the day-one store | Accepted |
| [0003](0003-store-source-launcher-traits.md) | Store, Source and Launcher extension traits | Accepted |
| [0004](0004-adapter-decides-sync-strategy.md) | Each Source adapter decides its sync strategy | Accepted |
| [0005](0005-cli-first-tui-second.md) | CLI first, TUI second | Accepted |
| [0006](0006-plugin-mechanism.md) | Plugin mechanism: external executables for users, in-process adapters built in | Accepted |
| [0007](0007-nb-under-the-hood.md) | nb under the hood: hybrid native/nb bookkeeping | Accepted |
| [0008](0008-license.md) | GPL-3.0-or-later license | Accepted |
| [0009](0009-tui-host-actions-through-the-cli.md) | TUI edits through core, host actions through the CLI binary | Accepted |
| [0010](0010-post-done-hook-from-the-tui.md) | The TUI's close reaches the `post-done` hooks through the `Host` | Accepted |
| [0011](0011-create-from-the-tui.md) | Creating a task from the TUI: title only, through the `Store`, hooks through the `Host` | Accepted |
| [0012](0012-detached-sessions-from-the-tui.md) | Opening a work session in a new window from the TUI: `Ctrl+Enter`/`Shift+Enter`, `--detached`, `launch.herdr.placement` | Accepted |
| [0013](0013-configurable-key-bindings.md) | Configurable TUI key bindings: `[ui.keys]`, action = key or list, parsed by the TUI | Accepted |
| [0014](0014-sync-sources-on-demand.md) | Choosing which sources a sync runs: `source[].auto`, repeatable `--source`, a source picker in the TUI | Accepted |
| [0015](0015-edit-form-in-the-tui.md) | Editing a task in a form inside the TUI: `e`, `edit::revise`, title/due/tags rewrites in the store | Superseded by ADR-0016 |
| [0016](0016-full-screen-edit-view.md) | The edit view takes the whole screen and edits the description: `form::Text`, `Ctrl+S`, `set_description` | Accepted |
| [0017](0017-calendar-picker-for-the-due-date.md) | A calendar picker for the Due box of the edit view: `Enter` opens a month grid over the view (`ui.week_start`), arrows/PgUp/PgDn/`t`, `Enter` picks as ISO | Accepted |
| [0018](0018-named-color-themes.md) | Named colour themes for the CLI and the TUI: every coloured thing is a `Role`, `ui.theme.preset` (dark/light/solarized/gruvbox/mono) under `[ui.theme.colors]` under `[ui.colors]` | Accepted |
| [0019](0019-relative-due-dates.md) | Relative, coloured due dates: `dates::Due`, `ui.due_format` (relative/iso/both), `overdue` and `due-soon` theme roles; ISO in `--json` and the detail pane | Accepted |
