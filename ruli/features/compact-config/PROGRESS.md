# Progress: configuration only in the config directory

Plan: [PLAN.md](PLAN.md). Task: tasq 60. ADR: 0031.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| CC-001 | The config directory | done | 5c4944c; mutants 35: 34 caught, 1 unviable, 0 missed |
| CC-002 | Profiles are files | done | 982b55f; mutants 39: 34 caught, 5 unviable, 0 missed |
| CC-003 | `worktrees_enabled` | done | 42bf3ae; mutants 8: 6 caught, 2 unviable, 0 missed |
| CC-004 | Repository configs replace `.tasq.toml` | done | 674522d; mutants 22: 12 caught, 10 unviable, 0 missed |
| CC-005 | Hooks run without trust | done | f561dff; mutants 48: 38 caught, 10 unviable, 0 missed |
| CC-006 | `tasq worktree config [DIR]` | done | ffc1b8e; mutants 18: 18 caught, 0 missed |
| CC-007 | `tasq doctor` checks repository configs | done | 7ba50f1; mutants 18: 12 caught, 6 unviable, 0 missed |
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
- 2026-10-27 (CC-003): every reader already went through `App::worktrees` (TUI, prompt,
  JSON, help) or `app::worktrees_enabled` (before parsing), so the move was two lines in
  `app.rs`. One reader outside Rust: the wrapup skill read `.config.worktree.enabled` from
  `config show --json`; now `.config.worktrees_enabled`. A grep over `plugins/` belongs in
  CC-008's guard.
- 2026-10-27 (CC-003): a leftover `TASQ_WORKTREE_ENABLED` is now ignored without a word,
  like `TASQ_CONFIG` after CC-001: worktrees quietly turn off. Same open decision for the
  user (error, warning or a CC-008 migration note).
- 2026-10-27 (CC-003): `README.md` (lines 111, 251), `docs/json.md:70` and `docs/config.md`
  (the "Worktrees" section and the env table) still say `worktree.enabled`, which the new
  error sends users to. CC-008 rewrites them.
- 2026-10-27 (CC-003): a `deserialize_with` that ignores its deserializer still gets the
  value's span, so the error points at the value (`3:11` for `enabled = true`).
- 2026-10-27 (CC-004): `Repo::of` canonicalizes the main checkout, core compares `path`
  without canonicalizing (as planned), so a `path` written through a symlink (`~/code`
  linked elsewhere, macOS `/tmp`) never matches and nothing says so. CC-007's doctor check
  should warn when `path` differs from its canonical form, not only when it is not a main
  checkout.
- 2026-10-27 (CC-004): every repository config is parsed on each worktree load, so one broken
  or duplicate file fails `worktree up`, `create` (auto worktree) and `done` (release) for
  every repository, by design. `worktree ls` used to swallow load errors per task
  (`repo_settings(..).ok()`), which would have listed every repository with default
  settings; it now fails with the error.
- 2026-10-27 (CC-004): `repo_settings` reloads the whole config once per task in `worktree ls`
  and `prune` (pre-existing with `config_at`); each load now also parses every
  `repos/*.toml` three times (switch probe, typed check, table). Fine at today's sizes;
  memoize by main checkout if `ls --all` gets slow.
- 2026-10-27 (CC-004): `#[serde(untagged)]` buffers the input and loses `toml::Spanned`, so
  two probes with spans need two structs tried in turn.
- 2026-10-27 (CC-004): `CLAUDE.md`, `README.md`, `docs/config.md`, `docs/json.md` and
  `docs/plugins.md` still describe the `.tasq.toml` walk, now silently ignored. CC-008.
- 2026-10-27 (CC-004): the trust store still applies, now to the repository config's hooks
  (`RepoSettings::repo_file` is the matched file, `untrusted` the kinds it sets), so the
  approvals key by `repos/<name>.toml`. CC-005 removes it.
- 2026-10-27 (CC-005): `--yes` lived on seven commands, not the five ADR 0031 lists:
  `worktree prune` and `worktree switch` had it too. All gone; an old `--yes` is now clap's
  exit 2 (`unexpected argument`), so a script passing it breaks loudly.
- 2026-10-27 (CC-005): the TUI's `d` used `Approval::Skip`, so a repository config's
  `pre-remove`/`post-remove` hooks never ran from it. They run now, synchronously, like the
  global ones always did; a slow hook freezes the UI for its duration (code review note,
  not changed).
- 2026-10-27 (CC-005): a hook line that does not render now fails a dry-run release for any
  layer; before, a repository config's unapproved line was skipped unrendered on a dry run.
- 2026-10-27 (CC-005): `tasq_core::worktree::hooks::state_file` is now used only for
  `switch -` (`previous-worktree`); it would sit better in a state/paths module. Left in
  place. `Loaded::repo` lost its last production reader (`repo_settings`); CC-006's
  "matched file or none" is meant to use it.
- 2026-10-27 (CC-005): CHANGELOG.md has no entry for any CC sub-task yet (removed
  `--config`, inline profiles, `worktree.enabled`, `.tasq.toml`, now trust and `--yes`).
  Worth one Removed/Changed block in CC-008.
- 2026-10-27 (CC-005): the first mutants baseline failed on
  `commands::view::tests::render_with_glow_runs_the_program_and_reports_failures`
  (`ETXTBSY`, the same flake as CC-001's); the rerun passed.
- 2026-10-27 (CC-006): hidden came for free: the whole `worktree` command is hidden while
  worktrees are disabled (`cli::command`), so a new subcommand only needs the `disabled()`
  check in its handler.
- 2026-10-27 (CC-006): `worktree config` resolves `DIR` with `existing_dir` (canonical) and
  `Repo::of`, the same main checkout every worktree command loads with, so it shows exactly
  what `up` and `release` use, including CC-004's symlink blind spot: a `path` written through
  a symlink shows `repository config: none` (code review note; CC-007's doctor check is the
  place to report it).
- 2026-10-27 (CC-006): `Repo::of` gives a linked worktree's own root as `main` when the
  common git directory is not named `.git` (a bare repository with worktrees), so each
  worktree of such a repository is its own "main checkout" for repository configs.
  Pre-existing in `tasq_core::worktree::git::main_checkout`, left as is (code review note).
- 2026-10-27 (CC-007): `cargo mutants --in-diff` runs only the tests of the mutated crate, so
  the CLI doctor tests caught nothing in `tasq-core` or `tasq-worktree`; the first run had 6
  missed. The fix was tests at each crate's own seam: exact strings for
  `repo_config::check`, `repo_configs` in `config_load.rs`, `repo::find` in
  `crates/worktree/tests/repo.rs`.
- 2026-10-27 (CC-007): the WARN for "not inside a git repository" is not strictly "never
  applies": `repo_settings` falls back to `main = project` when `Repo::of` finds nothing, so a
  repository config naming a plain directory does merge for a task whose project is that
  directory (code review note). No worktree can be created there, so the warning stays, as
  planned.
- 2026-10-27 (CC-007): the repository config checks run whether or not worktrees are enabled
  (like `check_worktree_templates`, so turning them on surfaces nothing new), and only when
  the config loads: a broken `config.toml` hides them until it is fixed (code review notes,
  not changed).

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
- CC-003: `worktree.enabled` is kept as a private `Removed` field (like `copy_ignored`), so a
  file fails at the value's file:line:column with `worktree.enabled moved to the top level:
  set worktrees_enabled above any table (docs/config.md, "Worktrees")`, whatever the value.
  `--set` and `TASQ_SET` have no position; they get `ConfigError::WorktreeEnabledMoved`,
  `<--set|env>: worktree.enabled moved to the top level: set worktrees_enabled (...)`,
  instead of the generic unknown key.
- CC-004: a relative `path` in a repository config is an error
  (`<file>:<line>:<column>: path must be absolute or start with ~/, the main checkout of a
  repository`), not joined to anything: core does not normalise `..`, so a relative one
  could never compare equal to a main checkout.
- CC-004: `worktrees_enabled` and `worktree.enabled` in a repository config get one dedicated
  error at the value, `worktrees_enabled is not a repository setting: set it in config.toml
  or a profile (docs/config.md, "Worktrees")`, rather than the generic unknown field or
  CC-003's "moved to the top level" (which would send the user to a key the file cannot
  hold). Other unknown keys keep the parser's message (`unknown field `store`, expected
  `path` or `worktree``); a missing `path` is `missing field `path`` at 1:1. Duplicates are
  `ConfigError::DuplicateRepo`: `repository configs <a> and <b> both have path <main>`, in
  file name order.
- CC-004: `Loaded::repo` holds the matched file (for CC-006's "matched file or none");
  `Origin::Repo` displays as the bare path, like `Origin::File`.
- CC-004: `tasq worktree ls` now fails when a repository config does not load, instead of
  listing with default settings (code review finding; the swallowed error predates CC-004
  but CC-004 widened it to every repository).
- CC-004: the CLI tests that used `<home>/.tasq.toml` as a generic config layer
  (`write_project_config`) now write `config.toml`; core tests that stacked it over the
  global file use a selected profile. The trust and `--yes` help texts say "repository
  config" until CC-005 deletes them.
- CC-005: also removed `HookFailure::{NoTrustFile, TrustFile, Ask}`, `Hooks::skipped` and the
  `skip` argument of `Hooks::render` (a dry run no longer prints `skip ... (not approved)`
  lines), and the CLI's unused `worktree_hooks::{prepare, run}`; the `From` impls in
  `error.rs` for the hook, create and release errors collapse to one arm each. `prepare`
  drops its `dry_run` and `report` arguments, `materialize` and `release` their `Approver`.
- CC-005: the trust sentences in `docs/config.md`, `docs/json.md` (the `worktree trust`
  row), `docs/architecture.md`, `README.md` and `CLAUDE.md` are removed here rather than in
  CC-008, since the commands they describe are gone.
- CC-006: the text output puts the matched file first, as a comment header
  (`# repository: <main>`, `# repository config: <file>|none`), then the `[worktree]` table
  rendered as `config show` renders it; it does not list the layers. The JSON is
  `repository`, `repository_config` (`null` for none), `worktree` and `origins`
  (`{"worktree.base": "<origin>", ...}`, an array or an empty table is one leaf); its row is
  added to `docs/json.md` here, not in CC-008.
- CC-006: a `DIR` that does not exist, is not a directory or is outside a git repository is a
  user error (`directory not found: <dir>`, `not a directory: <dir>`, `not inside a git
  repository: <dir>`), rather than falling back to the directory itself as `repo_settings`
  does for a task's project.
- CC-003: also changed outside the Rust readers: `examples/config/*.toml`, the
  `crates/core/tests/fixtures/worktree/*.toml` fixtures (they lose `enabled = true`; they are
  repository settings, which CC-004 forbids it in) and `plugins/claude/skills/wrapup/SKILL.md`.
- CC-007: beyond the four planned warnings, a `path` that resolves to the main checkout through
  a symlink or `..` warns too (`path <p> resolves to the main checkout <main> through a symlink
  or ..`), as the CC-004 and CC-006 learnings asked. The three cases that name a main checkout
  carry the fix `set path = "~/..."` (home shown as `~`, rendered as a TOML string); missing
  and outside git get `set path to the main checkout of a repository`. A file that is not a
  directory counts as missing (`does not exist or is not a directory`).
- CC-007: repository configs that do not load (a parse error, a duplicate `path`) are one
  `FAIL repo-config` check with the load error, since `Config::load` without a repository
  never reads them and every worktree command would fail on them. The checks come right after
  `config`. New public items: `tasq_core::config::{RepoConfig, repo_configs}`,
  `tasq_core::worktree::repo_config::{Found, check}` and `tasq_worktree::repo::find`, which
  reads the disk (no git process, so no injected env is needed, unlike the plan's wording).
