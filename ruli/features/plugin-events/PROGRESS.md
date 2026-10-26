# Progress: plugin extension points

Plan: [PLAN.md](PLAN.md). Task: tasq 59. ADR: 0030.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| PE-001 | Plugins are enabled by name | done | f90c7da. Mutants on the diff with `--no-config`: 0 missed (30 mutants plus a re-run of `config/load.rs` after the `Float` arm fix). |
| PE-002 | The `store-open` event | done | 6a720a5. Mutants on the diff with `--no-config`: 0 missed (27 mutants; `wait_until` and `kill_group` skipped as process I/O). |
| PE-003 | The `post-write` event | ready | |
| PE-004 | Lasting plugin warnings and the `doctor` event | ready | |
| PE-005 | `tasq store lock -- <cmd>` | done | e9fe2c7. Mutants on the diff with `--no-config`: 0 missed (14 mutants: 9 caught, 2 unviable, 3 timeouts that make `acquire_within` loop forever; `run_locked` skipped as process I/O). |
| PE-006 | The TUI reloads when the store changes | ready | |
| PE-007 | The docs describe plugins and events | ready | |

## Learnings

- 2026-10-19: scope settled by grilling task 57. Store sync ships as the `tasq-git` plugin, so
  the extension points it needs come first, as their own task, designed and tested with a fake
  plugin. ADR 0030 written; CONTEXT.md gained Sync, Store sync, Plugin, Event and Hook.
- 2026-10-26 (PE-001): `cargo mutants --in-diff` with `.cargo/mutants.toml` loaded examines no
  file under `crates/cli` (`exclude_globs`), so a CLI-heavy diff yields a handful of core
  mutants and looks clean. Run `--in-diff` with `--no-config` plus the `-E` excludes to cover
  the CLI; the rest of this plan lives mostly there.
- 2026-10-26 (PE-001): `[plugin.<name>]` holds `toml::Table`s, and `toml::Value` is not `Eq`
  (floats), so `Config` and `Loaded` now derive `PartialEq` only. Nothing needed `Eq`.
- 2026-10-26 (PE-001): an env or `--set` value for `plugin.<name>.<key>` cannot be typed from
  the template, so `coerce` types it from the layers merged so far, and as a string when no
  layer set it. Parsing it as TOML first (the first cut) turned `1.0` or `2026-10-26` into a
  float or a datetime behind the plugin's back.
- 2026-10-26 (PE-002): `--in-diff <(git diff HEAD)` sees only tracked files, so a new module
  or test file is silently left out. `git add -N` the new files first.
- 2026-10-26 (PE-002): an emptied kill or wait function shows as `TIMEOUT`, not `MISSED`: the
  plugin it should have stopped keeps the test waiting. That is what earns those two their
  `#[mutants::skip]`.
- 2026-10-26 (PE-002): a plugin that calls `$TASQ_BIN list` during `store-open` makes that
  `tasq` fire `store-open` again, without end (each level killed at the timeout, its children
  left running). A `tasq` started with `TASQ_EVENT` set now fires no event. A plugin's detached
  job inherits the variable too, so its own writes will not wake it either (PE-003).
- 2026-10-26 (PE-002): killing the plugin is not enough at the timeout. A background job
  (`git fetch &`) keeps the stdout pipe open after the plugin exits, so reading the output
  blocked past the timeout, and `kill` on the plugin left its children running. Plugins now
  run in their own process group (`process_group(0)`), the output is read on threads up to the
  same deadline, and the whole group is killed.
- 2026-10-26 (PE-005): a lock held by `tasq store lock` lives as long as its command, so the
  mtime stale rule broke a live holder after two minutes (a slow `git rebase`). The holder now
  refreshes the lock's mtime every 30 s (`StoreLock::refresh`); only a dead `tasq` lets it go
  stale. The refresh loop is not covered by a test (it would take 30 s); `refresh` is.
- 2026-10-26 (PE-005): `acquire` spun at full CPU on a stale lock it could not remove (the
  `remove_file` error was ignored and the loop retried at once). A stale lock that cannot be
  removed is now waited on like a live one, so `--timeout` holds there too.

## Blockers

None.

## Deviations

- PE-001: the CLI tests went into `crates/cli/tests/cli.rs` (`mod plugins`, `mod store`), not
  `crates/cli/tests/plugin.rs`: that file already tests the Claude Code plugin under
  `plugins/claude`. PE-002 onwards can start the event tests in a new file.
- PE-001: the `doctor` check for an enabled plugin is named `tasq-<name>`, not `<name>`:
  `git` would collide with the `git` tool check. An installed plugin gets an `OK` check with its
  path. The missing-plugin warning is printed by every command that opens the store except
  `doctor`, which reports it as the check only (`App::open_store_quietly`).
- PE-001: plugin name rules go beyond the plan's "no `/`, not a built-in": also no whitespace,
  no leading `-` (`tasq -v` is a flag, never dispatched) and no duplicates (every event would
  fire twice). All live in `Loaded::check_plugin_names`; the CLI passes the built-in names.
- PE-001: `store info` text always prints `plugins:`, `(none)` when none is enabled; `plugins
  list --json` names the new list `"enabled": [{name, path}]`, `path` `null` when missing.
- PE-001: a built-in name in `plugins.enabled` fails every command, `tasq config show`
  included, like a bad `store.name` does; the error names the file, so it is fixed by hand.
  Left as is (review finding, not changed).
- PE-002: the event tests are in `crates/cli/tests/events.rs`.
- PE-002: `doctor` fires no `store-open`. It opens the store through `open_store_quietly`;
  PE-004 gives it its own `doctor` event.
- PE-002: `plugins.timeout` is whole seconds (`u64`, default 2). There is no
  `TASQ_PLUGINS_TIMEOUT`: the env map is an explicit list, and `--set plugins.timeout=` covers
  it. A value too large for the clock waits without limit; `0` kills every plugin at once.
- PE-002: plugins differ from `[hooks]` in how they run (hooks unchanged): environment cleared
  to tasq's own plus the passthrough variables and `TASQ_EVENT`; own process group, killed as a
  group; the event lasts until the plugin's output closes, so a background job holding it is a
  timeout. A payload that cannot be serialised (a non-UTF-8 store path) is a warning for that
  plugin, not an error.
- PE-002: `store-open` at TUI start goes through `App::open_store` like every command, but no
  test covers it: `tasq ui` refuses to start without a terminal, before it opens the store.
- PE-002: two `#[mutants::skip]`s where the plan expected at most one: `wait_until` (polls the
  child) and `kill_group` (`kill(2)`). The spawn function itself needs none.
- PE-002: review findings left as is: an env or `--set` value for a `plugin.<name>` key whose
  file value is a datetime or a non-string array becomes a string (PE-001 coercion); a hook
  that never reads its stdin leaves the writer thread blocked (before, it blocked tasq itself).
- PE-005: `store lock` does not open the store: no `store-open` (the command is plumbing a
  plugin's job runs) and no missing-plugin warning. The store directory is created if absent.
- PE-005: a command that cannot start is exit 1 (`tasq: could not run <cmd>: ...`); one ended
  by a signal exits 128 plus the signal. The command's environment is tasq's own plus the
  passthrough variables, `TASQ_STORE_DIR` and `TASQ_LOCKED` (cleared first, as for events).
- PE-005: `StoreLock::acquire_within(dir, clock, Option<Duration>)` is the timed form; the
  limit is wall time, staleness still follows the configured clock.
- PE-005: review findings left as is: `Drop` removes the lock file without checking it is
  still ours (pre-existing; with the refresh only a holder that stopped refreshing loses it);
  Ctrl-C kills `tasq` and leaves the lock until it goes stale (no signal handling without a
  new dependency); a `tasq` write run by the locked command waits on that lock (documented in
  `--help`, `TASQ_LOCKED` is informational); the test files are unix-only like the others.
