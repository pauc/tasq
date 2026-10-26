# Progress: plugin extension points

Plan: [PLAN.md](PLAN.md). Task: tasq 59. ADR: 0030.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| PE-001 | Plugins are enabled by name | done | f90c7da. Mutants on the diff with `--no-config`: 0 missed (30 mutants plus a re-run of `config/load.rs` after the `Float` arm fix). |
| PE-002 | The `store-open` event | ready | |
| PE-003 | The `post-write` event | ready | |
| PE-004 | Lasting plugin warnings and the `doctor` event | ready | |
| PE-005 | `tasq store lock -- <cmd>` | ready | |
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
