# Plan: plugin extension points

Task: tasq 59 ("Plugin extension points: events, lasting warnings, store lock, TUI reload").
Blocks tasq 57 (store sync, plan in [`../git-sync/PLAN.md`](../git-sync/PLAN.md)). Decision
record: [ADR 0030](../../../docs/adr/0030-store-sync-as-a-plugin.md), building on
[ADR 0006](../../../docs/adr/0006-plugin-mechanism.md). Terms: [CONTEXT.md](../../../CONTEXT.md)
(Plugin, Event, Hook, Store). Progress: [PROGRESS.md](PROGRESS.md). Planned 2026-10-19 while
grilling task 57.

## Problem Statement

A plugin today is a `tasq-<name>` executable reached through `tasq <name>`, plus hooks: command
lines under `[hooks]` for `post-create`, `post-done` and `pre-launch`. That is enough for a
command such as `tasq tlogs`, and not enough for anything that has to follow the store:

- Most writes fire nothing: `set`, `log`, `apply`, `sync`, `mr`, `session`, `project`,
  `worktree`, `rank`, and every TUI edit except create and done.
- No hook is told which store it is about.
- Nothing runs when a command or the TUI opens the store.
- A plugin's problem can be reported only once, as a failing post-hook, and is gone on the next
  command; there is no `doctor` check for it.
- A plugin cannot take the store lock, so it cannot safely rewrite task files.
- The TUI never notices files changed by anything else until the user presses `r`, and an edit
  in that state fails with a conflict.
- Hooking a plugin into several events means wiring its command under each `[hooks]` key.

Store sync (task 57) needs all of it, and so would any plugin that mirrors, indexes or backs up a
store.

## Solution

Plugins are enabled by name and told about events:

```toml
[plugins]
enabled = ["git"]

[plugin.git]          # passed to the plugin as is; tasq does not validate it
interval = 60
```

For every event tasq runs `tasq-<name> hook` with a JSON document on stdin
(`{"schema": 1, "event": ..., "store": {...}, "config": {...}, ...}`), synchronously, with a
timeout. Three events:

- `store-open`: once per command and at TUI start, after the store is opened.
- `post-write`: once per command, after its last write, with the command name and every task
  written (id, file path); in the TUI once per action that wrote.
- `doctor`: during `tasq doctor`; the plugin answers with checks on stdout, listed under its
  name.

A plugin keeps the problems it wants shown in `<store>/.tasq/plugins/<name>/warnings`, one per
line; every command, the TUI status bar and `doctor` show them until the plugin clears the file.
`tasq store lock -- <cmd>` runs a command holding the store lock. The TUI polls the store
directory and reloads when it changed. An enabled plugin that is not installed is a warning on
every command and a `doctor` failure, never a failed command. `[hooks]` stays as it is.

## User Stories

1. As a user, I want to turn a plugin on with one line (`[plugins] enabled = ["git"]`), so that
   I do not wire it under each event by hand.
2. As a user, I want a plugin's settings under `[plugin.<name>]` in the same layered config, so
   that profiles, `.tasq.toml` and `--set` apply to them too.
3. As a user, I want a typo in `[plugins]` itself to be an error with file, line and column, as
   for every other key, while `[plugin.<name>]` is left to the plugin.
4. As a user, I want `tasq plugins list` to show which plugins are enabled and whether each is
   installed, so that I can see what runs on my writes.
5. As a user, I want an enabled plugin that is missing from `PATH` to warn on every command and
   fail in `doctor`, so that a missing plugin never blocks my tasks and never goes unnoticed.
6. As a plugin author, I want one `store-open` event per command and at TUI start, with the
   store's name and directory, so that I can start background work for that store.
7. As a plugin author, I want one `post-write` event per command, after its last write, listing
   every task written with its id and file path and the command that wrote, so that a command
   that writes twelve tasks costs me one call.
8. As a plugin author, I want `post-write` to fire for every writing command (`create`, `set`,
   `log`, `done`, `reopen`, `apply`, `sync`, `move`, `mr`, `session`, `project`, `worktree`,
   `rename`, `tag`, `due`, `desc`, `next`/`pick` setting in-progress) and every writing TUI
   action, so that no write escapes me.
9. As a plugin author, I want no `post-write` when a command wrote nothing (a no-op `set`, a
   failed command before any write, `--dry-run`).
10. As a plugin author, I want `post-write` after a command that failed half-way to list the
    writes that did happen, so that I see the files that changed.
11. As a plugin author, I want every payload to carry `"schema": 1`, with fields only added
    within a version, so that I can refuse a version I do not know.
12. As a plugin author, I want my `[plugin.<name>]` table in every payload as `config`, so that
    I need no second config reader.
13. As a plugin author, I want `TASQ_BIN`, `TASQ_PROFILE`, `TASQ_CONFIG`, `TASQ_SET` and
    `TASQ_EVENT` in my environment, so that my own `tasq` calls see the same store.
14. As a user, I want a plugin that hangs to be cut off after a timeout with a warning, so that a
    broken plugin cannot freeze a command or the TUI.
15. As a user, I want a plugin that exits non-zero on an event to be a warning, never a failed
    command, so that my write stands.
16. As a plugin author, I want to answer `doctor` with checks (`name`, `status`, `message`,
    `fix`) as JSON, so that `tasq doctor` explains my state next to the store's.
17. As a plugin author, I want lasting warnings: lines in `.tasq/plugins/<name>/warnings` that
    every command prints until I remove them, so that a failure in a background job reaches the
    user on the next command.
18. As a user, I want plugin warnings in the TUI status bar and as `Warn` checks in `doctor`,
    so that I see them whichever way I work.
19. As a plugin author, I want `tasq store lock -- <cmd>` to run my command holding the store
    lock, with tasq's stale rule, so that I can rewrite task files without racing a write.
20. As a plugin author, I want `tasq store lock` to exit with my command's exit code, and to fail
    without running it when the lock cannot be taken, so that I can tell the two apart.
21. As a user, I want the TUI to reload by itself when task files change on disk, so that
    another terminal, a hand edit or a plugin is visible without pressing `r`.
22. As a user, I want the TUI to keep my selection and scroll across such a reload, so that a
    background change does not move me.
23. As a user, I want `store info` to list the enabled plugins, so that the store's description
    includes what follows its writes.
24. As a maintainer, I want the events tested with a fake plugin (a script recording its stdin),
    so that the contract is pinned without any real plugin.
25. As a reader of the docs, I want `docs/plugins.md` to describe plugins, events, payloads,
    warnings and the lock normatively, so that a plugin in another repository can be written
    from it alone.

## Implementation Decisions

- **Config.** Core gains `PluginsConfig { enabled: Vec<String> }` under `[plugins]`
  (`deny_unknown_fields`) and a `[plugin]` table of tables kept as raw TOML values per name, not
  checked by the per-file unknown-field pass. Plugin names follow the `tasq-<name>` rule of
  ADR-0006 (no `/`, not a built-in command name). `HooksConfig` is unchanged; hooks keep their
  three events and their payloads.
- **The event runner** lives in the CLI next to the hook runner and shares its process code:
  `tasq-<name> hook`, stdin the payload, stdout and stderr piped, the environment of hooks plus
  `TASQ_EVENT`. One plugin at a time in `enabled` order; for each event, the plugins run before
  the `[hooks]` of the same moment. Timeout 2 s by default (`plugins.timeout`), then the child is
  killed and a warning names the plugin and the event. A non-zero exit is a warning with the
  plugin's stderr. Payloads are built in core (pure, serde) and executed in the CLI.
- **Payload.** `{"schema": 1, "event": "store-open" | "post-write" | "doctor", "store": {"name",
  "location"}, "config": {...}}`; `post-write` adds `"command"` (the subcommand name, `ui` in the
  TUI) and `"writes": [{"id", "file", "kind": "create" | "update"}]`. No task bodies: a plugin
  that wants them asks `tasq view --json`.
- **Collecting writes.** `App::open_store` wraps the `Box<dyn Store>` in a recording decorator
  that forwards every call and records each successful `create`, `update`, `update_many` and
  `set_done` (id and file via `file_of`). After the command's last output (the `report` step in
  `commands/mod.rs`), the CLI fires one `post-write` if anything was recorded, including when
  the command is returning an error. The TUI's `CliHost` drains the recorder after each action.
  The store crate and core stay unaware of plugins.
- **`store-open`** fires in `App::open_store` after the store opens and its open-time warnings
  are printed, so a plugin's own warnings come after the store's. Commands that never open the
  store (`config`, `completions`, `plugins list`, plugin dispatch) fire nothing.
- **Lasting warnings.** `<store>/.tasq/plugins/<name>/warnings`, UTF-8, one warning per
  non-empty line, owned by the plugin (tasq never writes or deletes it). tasq reads it for each
  enabled plugin after `store-open` returns and prints each line as `warning: <name>: <line>`;
  the TUI shows the first in the status bar with a count; `doctor` lists each as a `Warn` check
  named after the plugin. A missing file or directory is no warnings.
- **`doctor` event.** stdout is a JSON array of `{"name", "status": "ok" | "warn" | "fail",
  "message", "fix"?}`; checks are listed as `<plugin>/<name>`. Unparseable output is one `Fail`
  check for that plugin with the parse error. The plugin's checks come after the store's and
  before the tool checks.
- **Missing plugin.** An enabled plugin not found on `PATH`: one warning per command
  (`plugin git is enabled but tasq-git is not on PATH`), a `Fail` doctor check with the fix
  "install tasq-git or remove it from [plugins] enabled", and no event is sent to it.
- **`tasq store lock -- <cmd> [args...]`** takes the store lock with `StoreLock::acquire` on the
  configured clock, runs the command with the hook environment plus `TASQ_STORE_DIR` and
  `TASQ_LOCKED=1`, releases the lock, and exits with the command's code. A lock that cannot be
  taken (I/O error) is exit 1 without running anything. A `--timeout <secs>` (default: none; the
  stale rule already bounds a dead holder) fails with exit 1 if the lock is still held.
- **TUI reload.** A tick every 2 s compares a fingerprint of the store directory (the names,
  lengths and mtimes of its `.md` files) with the last one, and reloads through the existing `r`
  path when it differs, keeping the selected task by id and the scroll offset. The fingerprint
  is taken through the `Host` (the TUI stays free of I/O); the interval is not configurable.
- **`store info`** lists the enabled plugins (`plugins: git`; JSON `"plugins": ["git"]`).
  `StoreInfo::bookkeeper` goes: the store has no bookkeeper, and nothing reads the key.
- **Docs.** `docs/plugins.md` gains Enabling a plugin, Events (with every payload), Lasting
  warnings, Doctor checks and The store lock; `docs/config.md` documents `[plugins]` and
  `[plugin.<name>]`; `docs/json.md` lists the event payloads and the `store info` change.
  `examples/plugins/tasq-echo` (bash) logs every event it gets to a file and is the fake plugin
  the tests use.

## Testing Decisions

- A good test drives a real `tasq` command (or the TUI's host) against the fixture store with a
  fake plugin on `PATH`, and asserts on what the plugin received (its recorded stdin and
  environment) and on what the user saw (stdout, stderr, exit code). It never asserts which
  internal function fired the event.
- **Core (pure).** Payload construction and its JSON (`insta` snapshots per event), the
  `[plugins]`/`[plugin.<name>]` config merge across layers, and the doctor-check parser (valid,
  unknown status, not JSON). Prior art: `HookDocument` serialisation and the config load tests.
- **CLI integration (main seam).** `crates/cli/tests/plugin.rs` with `examples/plugins/tasq-echo`
  on `PATH`: one `store-open` per command; one `post-write` per writing command with the exact
  writes (a table over every writing command); none after a no-op, `--dry-run` or a read; the
  writes before a failure; payload `config` from `[plugin.echo]` and `--set`; timeout and
  non-zero exit as warnings with the exit code unchanged; the missing plugin warning; lasting
  warnings printed and in `doctor`; plugin doctor checks; `store info` and `plugins list`
  snapshots. Prior art: the `[hooks]` tests in the same file and `plugins::example_tlogs_plugin_runs_through_dispatch`.
- **`tasq store lock`.** The command sees `TASQ_LOCKED=1` and the lock file exists while it runs;
  a write started meanwhile waits (a second `tasq log` blocked until the locked command exits,
  checked with a marker file); the exit code is passed through; a stale lock is broken by the
  configured clock. Prior art: `crates/store/tests` lock tests.
- **TUI.** A host double that returns a changed fingerprint makes the app reload and keep the
  selection; an unchanged one does nothing; the `post-write` drain runs after a writing action
  and not after navigation. Prior art: the TUI's `Host` doubles in `crates/tui`.
- **Mutation bar.** Zero missed in the diff (`cargo mutants --in-diff`). The function that
  spawns the plugin process and waits on it is the only `#[mutants::skip]` candidate (pure I/O).
- Every cargo command goes through `scripts/guard`; one build at a time.

## Out of Scope

- The git plugin itself (task 57).
- Plugins adding subcommands under built-ins (`tasq store <x>`): a plugin keeps `tasq <name>`.
- Plugin key bindings or actions in the TUI, status-bar items beyond the warnings, panes.
- `pre-write` events, or any event that can veto a write.
- Running plugins asynchronously from tasq (a plugin detaches its own work).
- Changing `[hooks]`, their events or their payloads.
- A file watcher (inotify); the TUI polls.

## Further Notes

- `post-write` is per command so that `tasq sync` closing many tasks yields one event; the
  recording decorator is what makes that possible without touching core's edit functions.
- The TUI fires `store-open` at start and on every manual `r`, not on the automatic reload, so a
  plugin is not woken every time its own job changes the files.

## Sub-tasks

Blockers first; work the frontier, one build at a time. Each sub-task passes fmt, clippy and the
full suite and closes with a row in PROGRESS.md.

### PE-001: plugins are enabled by name

**What to build:** `[plugins] enabled` and `[plugin.<name>]` load from every config layer;
`tasq plugins list` shows enabled plugins and whether each is on `PATH`; an enabled plugin that
is missing warns on every command that opens the store and fails in `doctor`; `store info`
lists the enabled plugins and `bookkeeper` is gone.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] An unknown key under `[plugins]` fails with file:line:column; anything under `[plugin.<name>]` loads.
- [x] `plugins list` and `store info` (text and JSON) snapshots show the enabled plugins.
- [x] Missing plugin: warning on stderr, exit code unchanged; `doctor` `Fail` with the fix text.
- [x] `mutants-in` the diff: zero missed.

### PE-002: the `store-open` event

**What to build:** the event runner (`tasq-<name> hook`, payload on stdin, environment, timeout,
warnings on failure) and the `store-open` event fired once per command that opens the store and
at TUI start, with `examples/plugins/tasq-echo` as the fake plugin.

**Blocked by:** PE-001.

**Status:** done

- [x] One `store-open` per command that opens the store, none for commands that do not.
- [x] Payload snapshot: schema, event, store name and location, config.
- [x] Timeout and non-zero exit are warnings naming the plugin and event; the command's exit code is unchanged.
- [x] `mutants-in` the diff: zero missed.

### PE-003: the `post-write` event

**What to build:** the recording decorator around the store, one `post-write` per command with
the command name and every write (id, file, create or update), none when nothing was written,
and the writes before a failure; one per writing action in the TUI.

**Blocked by:** PE-002.

**Status:** ready

- [ ] A table test over every writing command asserts the exact writes received.
- [ ] No event after a read, a no-op write or `--dry-run`.
- [ ] A command failing after its first write still sends that write.
- [ ] The TUI host sends one event after a writing action and none after navigation.
- [ ] `mutants-in` the diff: zero missed.

### PE-004: lasting plugin warnings and the `doctor` event

**What to build:** tasq reads `.tasq/plugins/<name>/warnings` after `store-open` and prints each
line; the TUI shows them in the status bar; `doctor` lists them and the plugin's own checks
from the `doctor` event.

**Blocked by:** PE-002.

**Status:** ready

- [ ] Warnings printed on every command while the file has lines, nothing once it is gone.
- [ ] `doctor` text and JSON snapshots with lasting warnings and plugin checks; unparseable output is one `Fail`.
- [ ] TUI status bar shows the first warning and the count.
- [ ] `mutants-in` the diff: zero missed.

### PE-005: `tasq store lock -- <cmd>`

**What to build:** a command that runs another holding the store lock, passes its exit code
through, sets `TASQ_STORE_DIR` and `TASQ_LOCKED=1`, and fails without running it when the lock
cannot be taken or `--timeout` passes.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] A write started while the locked command runs waits for it.
- [x] Exit code passed through; `--timeout` and I/O failure are exit 1 without running the command.
- [x] `mutants-in` the diff: zero missed.

### PE-006: the TUI reloads when the store changes

**What to build:** a 2 s tick compares the store directory's fingerprint through the `Host` and
reloads when it changed, keeping the selected task and scroll; `store-open` fires on manual `r`.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] A changed fingerprint reloads and keeps the selection by id; an unchanged one does nothing.
- [x] An edit made after the reload does not conflict.
- [x] `mutants-in` the diff: zero missed.

### PE-007: the docs describe plugins and events

**What to build:** `docs/plugins.md` (Enabling a plugin, Events and payloads, Lasting warnings,
Doctor checks, The store lock), `docs/config.md` (`[plugins]`, `[plugin.<name>]`), `docs/json.md`
(event payloads, `store info`), `docs/architecture.md` and CLAUDE.md where they describe hooks.

**Blocked by:** PE-001, PE-002, PE-003, PE-004, PE-005, PE-006.

**Status:** ready

- [ ] A plugin can be written from `docs/plugins.md` alone: every payload has an example.
- [ ] Docs only; no mutation run.
