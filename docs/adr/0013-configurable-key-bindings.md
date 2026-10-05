# ADR-0013: Configurable TUI key bindings (`[ui.keys]`)

- **Status:** Accepted
- **Date:** 2026-10-05

## Context

ADR-0012 bound `Ctrl+Enter` and `Shift+Enter` to the detached launches.
Terminals take chords for themselves: Ghostty on Linux keeps `Ctrl+Enter`
for fullscreen, so the key never reaches the TUI, and the only fix today
is on the terminal side (`keybind = ctrl+enter=unbind`). The author
refused alias bindings (an `Alt+Enter` next to `Ctrl+Enter`): the
bindings are a deliberate, minimal surface, and a collision with one
terminal is not a reason to grow it for everyone. A user who hits a
collision should rebind, in the config, like they would in any editor.

Today the map is a fixed `match` in `crates/tui/src/keys.rs` (`normal`,
`text`, `picker`), and the help overlay and the status-bar hints are
string constants in `crates/tui/src/view.rs`. `[ui.colors]` already shows
the shape for a user-facing table the TUI owns: core stores the strings
and the TUI interprets them, since `tasq-core` has no terminal
dependency and must not learn crossterm's key types.

## Decision

- **`[ui.keys]` maps action names to keys.** A value is one key or a list
  of keys; `[]` unbinds the action. Unset actions keep their defaults, so
  an empty table changes nothing. Layers deep-merge per action like any
  table, and `--set ui.keys.<action>=k1,k2` works through the existing
  comma-separated array coercion (`tasq ui` is the only consumer; the
  CLI help is static).
- **Actions are normal-mode actions plus the navigation the pickers
  share.** `up`, `down`, `page-up`, `page-down`, `top`, `bottom`,
  `filter`, `status`, `priority`, `log`, `done`, `create`, `edit`,
  `launch`, `launch-detached`, `launch-detached-stay`, `sync`, `reload`,
  `help`, `toggle-detail`, `cancel`, `quit` apply in normal mode; the
  status and priority pickers use `up`, `down`, `confirm`, `cancel` and
  close on `quit`. Typing (filter, note, title) is not configurable:
  characters, `Enter`, `Esc` and `Backspace` do what they always do. The
  help overlay closes on any key. `Ctrl+C` quits in every mode and cannot
  be rebound or unbound, so no configuration can lock the user in.
- **One key syntax.** `[ctrl+][alt+][shift+]<key>`, modifiers in any
  order, case-insensitive. `<key>` is a single character (`j`, `G`, `/`,
  `?`) or a name: `enter`, `esc`, `tab`, `backspace`, `space`, `up`,
  `down`, `left`, `right`, `home`, `end`, `pgup`, `pgdn`, `del`, `ins`,
  `f1`..`f12`. `shift` combines only with a named key; a shifted
  character is written as the character (`G`, not `shift+g`), because
  that is what the terminal sends. A key bound to two actions of the
  same mode is an error.
- **Core stores, the TUI parses.** `UiConfig.keys:
  BTreeMap<String, KeySpec>` (`KeySpec` is a string or a list of strings,
  `serde(untagged)`), with a `<action>` wildcard in the `--set` template
  like `ui.colors.<name>`. `tasq-tui` owns `keys::{Action, Chord,
  KeyMap}`: `KeyMap::default()` is today's map, `KeyMap::from_config`
  overlays the table and returns `KeyError` for an unknown action, a bad
  key or a conflict. `translate(&KeyMap, &Mode, &KeyEvent)` consults the
  map; `Model` carries the map so `view` can render it.
- **Help follows the map.** The `?` overlay rows and the status-bar hints
  are rendered from the `KeyMap` (first key of each action for the hints,
  every key for the overlay, `none` for an unbound action), so they can
  never disagree with what the keys do. `tasq ui --help` and the README
  describe the defaults and point to `[ui.keys]`.
- **`tasq ui` fails at startup** on a bad table, naming the config path
  (`ui.keys.<action>`), the offending value and the file it came from
  (`Loaded::file_for`). It does not start with a partial map.

## Consequences

### Positive

- A terminal collision is fixed in the user's config with one line, and
  the overlay shows the key they chose.
- The defaults and their help text live in one table (`keys.rs`), not in
  a `match` plus three string constants; adding an action is one row.
- Every binding stays a one-line test: parse a spec, translate an event.

### Negative

- One more config table and a key grammar to document and keep stable.
- Exact modifier matching: `Ctrl+Shift+Enter` was `launch-detached`
  (ctrl checked first) and is now unbound unless configured. Nothing
  documented it; the test that pinned it goes.
- `Model` grows a field every host and test constructs through
  `Model::new` (defaults) or `with_keys`.

## Alternatives considered

- **Key-keyed table (`"alt+enter" = "launch-detached"`)** — reads like a
  terminal keybind file, but keeps the default keys bound unless each is
  also set to `none`, and a list per action is the common case (`j` and
  `Down`). Action-keyed with a list says the whole binding in one line.
- **Per-mode tables (`[ui.keys.normal]`, `[ui.keys.picker]`,
  `[ui.keys.text]`)** — the most flexible, but text input has nothing
  worth rebinding and the pickers only navigate; one flat table with
  shared navigation actions covers what anyone has asked for.
- **Parse and validate in core** — would give file:line errors at load
  time for every command, but core would have to know the TUI's action
  names and a key model; `[ui.colors]` set the precedent of the TUI
  owning its names, and `tasq ui` is the only reader.
- **Alias bindings in `keys.rs`** — rejected before this ADR; see
  Context.

## References

- ADR-0012 (the chords this makes configurable), ADR-0005 (core/TUI
  boundary).
- `crates/tui/src/keys.rs` (`Action`, `Chord`, `KeyMap`, `translate`),
  `crates/tui/src/view.rs` (help and hints from the map),
  `crates/core/src/config/mod.rs` (`UiConfig.keys`, `KeySpec`),
  `crates/cli/src/commands/ui.rs` (startup error), `docs/config.md`
  ("Key bindings").
