# Progress: plugin extension points

Plan: [PLAN.md](PLAN.md). Task: tasq 59. ADR: 0030.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| PE-001 | Plugins are enabled by name | done | f90c7da. Mutants on the diff with `--no-config`: 0 missed (30 mutants plus a re-run of `config/load.rs` after the `Float` arm fix). |
| PE-002 | The `store-open` event | done | 6a720a5. Mutants on the diff with `--no-config`: 0 missed (27 mutants; `wait_until` and `kill_group` skipped as process I/O). |
| PE-003 | The `post-write` event | done | ec3c370. Mutants on the diff with `--no-config`: 0 missed (44 mutants: 33 caught, 11 unviable; a first run missed 3 forwardings of `Recording`, now unit-tested). |
| PE-004 | Lasting plugin warnings and the `doctor` event | done | 48d5086. Mutants on the diff with `--no-config`: 0 missed (58 mutants: 51 caught, 7 unviable; a first run missed 3 on `NoHost::warnings`, now pinned and written `vec![]`). |
| PE-005 | `tasq store lock -- <cmd>` | done | e9fe2c7. Mutants on the diff with `--no-config`: 0 missed (14 mutants: 9 caught, 2 unviable, 3 timeouts that make `acquire_within` loop forever; `run_locked` skipped as process I/O). |
| PE-006 | The TUI reloads when the store changes | done | ad4d481. Mutants on the diff with `--no-config`: 0 missed (40 mutants: 37 caught, 3 unviable; `run` stays skipped as terminal I/O). |
| PE-007 | The docs describe plugins and events | done | a054f06. Docs only, no mutation run: the diff has no Rust, so 0 mutants and 0 missed. `cargo doc` (no new warnings) and the full suite pass. |

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
- 2026-10-26 (PE-006): the first mutants run left the "file vanished" guard of
  `tasq_store::fingerprint` alive: a race no test can stage. Following symlinks
  (`fs::metadata(path)`, as `scan` does with `fs::read`) made the same guard reachable:
  a dangling `x.md` symlink is the vanished file, a symlink loop the other error.
- 2026-10-26 (PE-006): an automatic reload can move the selection under an open picker or
  input (the task closed elsewhere and left the list), and pickers act on the selection when
  they commit, not when they opened. The tick therefore reloads only in normal mode; the
  `Watch` is not consulted meanwhile, so the change is seen on the first tick back.
- 2026-10-26 (PE-006): a tick that runs only when `event::poll` times out is starved by a
  held key. `run` checks the deadline before waiting for a key.
- 2026-10-26 (PE-003): a successful `Store::update` does not mean a write. `TasqStore` skips
  the write when the file already says so, and several commands call `update` with an
  unchanged task (`project` to the same path, `apply` of an unchanged document). The recorder
  therefore reads the task's file before and after each write and notes it only when the
  bytes differ. It must not call `get` for that: `get` resets the revision `update` checks
  for conflicts; `file_of` does not.
- 2026-10-26 (PE-003): `pick` and `next` never return to `app::run` when the launcher
  replaces the process (`exec`), so the event fired at the end of the command was lost.
  `post-write` is also fired just before the launcher runs; the recorder is drained there, so
  the end-of-command call sends nothing twice.
- 2026-10-26 (PE-003): a plugin's environment is cleared to tasq's own, so a fake plugin in a
  unit test (`PATH` = its own bin directory) has no `cat`; read stdin with `read -r`.
- 2026-10-26 (PE-004): a plugin named `git` collides with the `git` tool check in `doctor`
  (PE-001 met the same thing with `tasq-git`). Every check a plugin owns is therefore
  `<name>/...`: its own answers, `<name>/doctor` for a failed or unparseable answer and
  `<name>/warning` for each lasting warning. `render` padded names to 13 with no gap, so such a
  name ran into its detail; it now always keeps one space.
- 2026-10-26 (PE-004): `cargo mutants` turns a body of `Vec::new()` into `vec![]`, an
  equivalent mutant no test can kill. Writing the body as `vec![]` makes that mutant identical
  to the code, and cargo-mutants drops it.
- 2026-10-26 (PE-004): the TUI asks for the lasting warnings on every 2 s tick, so
  `App::plugin_warnings` reads the files by the configured names; going through
  `App::plugins()` searched `PATH` for each plugin on every tick (review finding).
- 2026-10-26 (PE-007): the payload's `store.name` is the configured `store.name`
  (`default`), while `tasq store info --json` has `store.name` `tasq` (the store kind,
  `StoreInfo::name`). Same key, different values; the docs spell out each.
- 2026-10-26 (PE-007): PE-004, PE-005 and PE-006 added no CHANGELOG entry (PE-001 to PE-003
  did). The docs ticket added them; a ticket that changes behaviour should carry its own.

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
- PE-006: the fingerprint is `tasq_store::fingerprint(dir) -> io::Result<u64>`: names,
  lengths and mtimes of the `.md` regular files the store reads, hashed with std's
  `DefaultHasher` (in-process only). `Host` gained `fingerprint() -> Option<u64>` (`None`: no
  reload) and `store_open() -> Result<(), String>`; `r` is now `Cmd::Refresh` (host
  `store_open`, then load), the tick and the start stay `Cmd::Load`. `NoHost` never reloads.
- PE-006: the plan's "keeps the scroll" needed no code: the list scroll follows the
  selection, and the detail scroll resets only when the selection changes. A test pins both.
- PE-006: "an edit after the reload does not conflict" is tested in `commands/ui.rs` with the
  real store and the CLI host (a hand edit, the tick, then `l`), not in `crates/tui`, which
  has no `TasqStore`. Every core edit reads the task before it writes, so the conflict the
  plan feared needed no fix; the test pins that the outside change survives the edit.
- PE-006: review findings left as is: the TUI's own write is seen as a change by the next
  tick, so each edit costs one extra scan (re-baselining after a dispatch could miss an
  outside change made between its reload and the look); a same-length rewrite within one
  mtime tick (or a sync tool that keeps mtimes) is not noticed, as the plan's fingerprint
  defines; a slow plugin freezes the UI on `r` up to `plugins.timeout`, as at start; when
  the reload after `r` also fails, its error replaces the plugin warning in the status bar;
  `with_plugins` in the `ui.rs` tests is one more fake-executable helper.
- PE-003: the recorder lives in the CLI (`crates/cli/src/writes.rs`, `Recording` and the
  shared `Written`), as planned, but notes a task only when its file's bytes changed, not on
  every successful call (Learnings). A task written twice by one command is listed once, with
  its first kind (`create --worktree` stays a `create`).
- PE-003: the command name comes from clap (`Cli::name`, `matches.subcommand_name()`), so an
  alias reports the command (`ls` is `list`) and `worktree set` is `worktree`. It is kept on
  `App::command`.
- PE-003: in the TUI the runtime calls a new `Host::post_write` after every action (not after
  `Load`, `r` or the tick); `CliHost` sends `post-write` from `ui` when the action wrote
  through the store. A plugin warning joins the action's outcome as a failure, like a
  `post-done` hook warning. Actions run as child processes (`pick`, `sync`) send their own
  event. `e` (`$EDITOR` on the task file) writes outside the store, so `CliHost::edit`
  compares the file before and after and notes the change itself.
- PE-003: the store is wrapped only while a plugin listens (`plugins::listening`: one is
  installed and `TASQ_EVENT` is not set), so without plugins no write costs an extra scan.
  With one, each write costs one extra `file_of` scan and two reads of the task's file.
- PE-003: two unit tests on `Recording` itself, outside the plan's seams: no command calls
  `set_done` through the wrapped store, and `doctor` and the store's warnings never go
  through it in a command, so the CLI tests cannot reach those forwardings.
- PE-003: review findings left as is: the snapshots are taken outside the store lock, so a
  write by another process in between can add that task to `writes` (its file did change);
  the store reporting its own writes would be exact but changes the `Store` trait, which the
  plan keeps unaware of this; a plugin warning in the TUI is failure-styled, as `post-done`
  warnings are; `App::post_write` drops `fire`'s error, which only a missing store directory
  raises after the store opened; a bare `tasq` has an empty `App::command`, and writes
  nothing; `CliHost` names its command `ui` itself, as the payload contract says.
- PE-004: a blank answer to `doctor` is no checks, not an unparseable one: a plugin with
  nothing to report (`tasq-echo`, a plugin written before the event) prints nothing and
  `doctor` stays clean. Non-JSON output or a non-zero exit is one `Fail` check, as planned.
- PE-004: check names are `<plugin>/<name>`, `<plugin>/doctor` and `<plugin>/warning`, not
  "named after the plugin" (Learnings). In `doctor` each plugin's checks follow its
  `tasq-<name>` check: installed or not, then its answer, then its lasting warnings (read
  after the event, so a plugin can refresh them in its `doctor` handler).
- PE-004: the core side is `plugin::doctor_checks`, `plugin::warnings_file` and
  `plugin::warnings` (pure); core now depends on `serde_json` (it was a dev-dependency) to
  parse the answer. The CLI's `fire` is split into `fire_each` (each plugin's outcome) so
  `plugins::doctor` can read each answer.
- PE-004: lasting warnings are read for every enabled plugin, installed or not, and printed
  after `store-open` (and after the missing-plugin warnings), unless `TASQ_EVENT` is set: a
  `tasq` a plugin runs during an event prints none (review finding). A file that cannot be
  read is no warnings.
- PE-004: TUI: `Host::warnings()` and `Msg::Warnings`. The runtime asks at start and on every
  tick, in any mode (they move no selection), and sends them only when they differ from the
  model's; `r` picks them up on the next tick. The status bar shows `warning: <plugin>: <line>`,
  or `<n> warnings: <first>`, when no message or prompt is up, in place of the key hints.
- PE-004: `update::normal` crossed clippy's 100 lines with the new arm; the `t` picker moved
  to `begin_status`, like `begin_note` and `begin_create`.
- PE-004: review findings left as is: a plugin that prints text on every event, or exits
  non-zero on one it does not know, fails `doctor` (the plan's contract; plugins answer
  `doctor` or stay silent); `tasq ui` prints the lasting warnings on stderr before the
  alternate screen, as it does the missing-plugin and `store-open` warnings, and the status
  bar shows them again; `doctor` run by a plugin during an event has no plugin checks,
  silently, as `fire` sends nothing then; the fix text of an unparseable answer spells
  `tasq-<name>` in core, where the CLI's `PREFIX` is not visible; several lasting warnings of
  one plugin share the check name `<plugin>/warning`; a check name over 12 characters shifts
  its detail but not its `fix:` line.
- PE-005: review findings left as is: `Drop` removes the lock file without checking it is
  still ours (pre-existing; with the refresh only a holder that stopped refreshing loses it);
  Ctrl-C kills `tasq` and leaves the lock until it goes stale (no signal handling without a
  new dependency); a `tasq` write run by the locked command waits on that lock (documented in
  `--help`, `TASQ_LOCKED` is informational); the test files are unix-only like the others.
- PE-007: `post-write` runs after the command's `[hooks]` (`post-create`, `post-done`, and
  `pre-launch` in `next`/`pick`), not before them as Implementation Decisions say ("the
  plugins run before the `[hooks]` of the same moment"): it is sent at the end of the
  command (PE-003). Documented as built; `store-open` and `doctor` have no hook to order
  against.
- PE-007: `docs/plugins.md` also gained "The process" (arguments, environment, timeout, exit
  codes, calling back into tasq) and "The terminal UI", which the plan did not list, so that
  a plugin can be written from the page alone. README (plugins paragraph, `store lock`
  in the command table) and the CHANGELOG were updated too.
- PE-007: `docs/config.md`'s Hooks section said the TUI's `d` key fires no hook; it has run
  `post-done` (and `c` `post-create`) since ADR-0010/0011. Corrected while documenting
  `[plugins]` next to it.
- PE-007: the review ran over the whole branch and found nothing in the docs. Its code
  findings are left for a follow-up, since this ticket is docs only: `plugins.enabled`
  accepts `.` and `..` (the warnings file then resolves outside `.tasq/plugins/<name>`);
  `StoreLock::refresh` stamps wall time while staleness follows the injected clock, so a
  waiter with `TASQ_NOW` far ahead could break a live lock; `run_locked` carries the
  refresh-interval decision under `#[mutants::skip]`; after a plugin exits with its pipes
  held open, the timeout kills its process group by a pid already reaped; `doctor` with a
  built-in name in `plugins.enabled` skips the store and plugin checks; a plugin in its own
  process group outlives a Ctrl-C of `tasq` and is stopped if it reads the terminal. Already
  recorded above and documented as built: a tasq write under `tasq store lock` waits on that
  lock, the extra `file_of` scan per write, the TUI's extra reload after its own write.
