# Progress: configuration only in the config directory

Plan: [PLAN.md](PLAN.md). Task: tasq 60. ADR: 0031.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| CC-001 | The config directory | done | 5c4944c; mutants 35: 34 caught, 1 unviable, 0 missed |
| CC-002 | Profiles are files | done | 982b55f; mutants 39: 34 caught, 5 unviable, 0 missed |
| CC-003 | `worktrees_enabled` | todo | |
| CC-004 | Repository configs replace `.tasq.toml` | todo | |
| CC-005 | Hooks run without trust | todo | |
| CC-006 | `tasq worktree config [DIR]` | todo | |
| CC-007 | `tasq doctor` checks repository configs | todo | |
| CC-008 | The docs describe the config directory | todo | |

## Learnings

- 2026-10-27: scope settled by grilling task 60. ADR 0031 written; CONTEXT.md gained Config
  directory, Profile, Repository config and Main checkout.

- 2026-10-27 (CC-001): launched sessions (`launch.rs`, the `sync` briefing) forward
  `TASQ_PROFILE` but never forwarded `TASQ_CONFIG`, and worktree hooks (`worktree_hooks.rs`,
  `tasq-worktree` create/release) get no `passthrough_env` at all. Pre-existing, left as is;
  with CC-002 a session started under `--config-dir` would look for its profile in the
  default directory. Worth folding into CC-002 or a follow-up.
- 2026-10-27 (CC-001): a leftover `TASQ_CONFIG` in the environment is now ignored without a
  word. The plan gives dedicated errors only to `[profile.x]` and `worktree.enabled`; a
  warning or error for `TASQ_CONFIG` is a decision for the user (CC-008 migration notes at
  least).
- 2026-10-27 (CC-001): `commands::ui::tests::sync_passes_the_picked_sources_as_source_flags`
  failed once and passed on rerun (it writes a script and runs it at once; likely
  `ETXTBSY` under parallel tests). Not touched.
- 2026-10-27 (CC-002): `toml::Spanned` keys order by value, so finding the first
  `[profile.x]` in a file takes the smallest span start, not the first map key.
- 2026-10-27 (CC-002): `App::config_at` clones the options with `cwd` set to the main
  checkout, so a relative `--config-dir` is resolved against the repository in the worktree
  commands. Pre-existing from CC-001; CC-004 replaces `config_at` with `config_for_repo`,
  which should keep the real cwd. Likewise `plugins::config_env` makes a relative directory
  absolute against the process cwd, not `LoadOptions::cwd` (the same directory outside tests).
- 2026-10-27 (CC-002): worktree hooks now get `TASQ_PROFILE` and `TASQ_CONFIG_DIR`, but not
  `TASQ_SET` or `TASQ_BIN`, which task hooks and plugins get through `passthrough_env`. Left
  as is; a `tasq` in a worktree hook drops `--set` overrides.
- 2026-10-27 (CC-002): `docs/config.md` still describes `[profile.<name>]` tables, now an
  error; CC-008 rewrites it.

## Blockers

None.

## Deviations

- CC-001: the error variant is `ConfigError::MissingDir { dir, named_by }`, not
  `MissingExplicit`; the message is `config directory <dir> does not exist or is not a
  directory (named by --config-dir|TASQ_CONFIG_DIR)`, since a file passed by habit from
  `--config` exists but is not a directory.
- CC-001: a relative named directory is joined to `LoadOptions::cwd` (injected), so errors
  and `config show` provenance show an absolute path.
- CC-001: `TASQ_CONFIG_DIR` is forwarded absolute to plugins and hooks whether it came from
  `--config-dir` or from the environment (before, an inherited `TASQ_CONFIG` passed through
  unchanged, so a relative one broke in a hook running elsewhere).
- CC-001: `docs/config.md`, `docs/plugins.md` and `README.md` still describe `--config` /
  `TASQ_CONFIG`; left for CC-008 as planned. The `--help` text (`PLUGINS_HELP`, the flag)
  is updated here.
- CC-002: the `[profile.x]` error is `ConfigError::InlineProfile`, message
  `<file>:<line>:<column>: profiles are files now: move [profile.<name>] to
  <config dir>/profiles/<name>.toml`, at the profile's name. A `profile` key that is not a
  table of tables stays the generic unknown field.
- CC-002: `UnknownProfile` gained `file` and reads `profile "x" is not defined: no file
  <config dir>/profiles/x.toml; available profiles: ... | none`, since an empty list is now
  the common case. A name is accepted only if it is in the listing, so `--profile ../x` is
  unknown rather than a path.
- CC-002: the env for sessions and hooks is a new `plugins::config_env` (profile and config
  directory), split out of `passthrough_env`; worktree hooks get it through `lifecycle_git`
  in `commands/worktree.rs` (the `Git` env that create and release pass to hooks).
