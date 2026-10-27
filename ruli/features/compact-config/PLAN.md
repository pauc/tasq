# Plan: configuration only in the config directory

Task: tasq 60 ("Compact config"), GitHub issue pauc/tasq#13. Decision record:
[ADR 0031](../../../docs/adr/0031-config-only-in-the-config-directory.md), partly superseding
[ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md). Terms:
[CONTEXT.md](../../../CONTEXT.md) (Config directory, Profile, Repository config, Main
checkout). Progress: [PROGRESS.md](PROGRESS.md). Planned 2026-10-27 while grilling task 60.

## Problem Statement

Configuration is read from the global file and from the nearest `.tasq.toml` above the current
directory, and profiles are `[profile.<name>]` tables inside any of them. The worktree commands
load it again from the repository's main checkout, so a `.tasq.toml` next to a repository
supplies its `[worktree]` settings. Hooks from such a file run only once approved, because it
may have come with a clone.

The user wants every setting in `~/.config/tasq`. Today that is one global file with three
inline profiles and two `.tasq.toml` files in workspace directories, each with `[worktree]`
settings only. The trust machinery guards against files the user never receives.

## Solution

```
~/.config/tasq/                  --config-dir / TASQ_CONFIG_DIR replace it
├── config.toml                  global config
├── profiles/sf.toml             the profile "sf": [store], [work], [[source]] ...
└── repos/silverfin.toml         path = "~/code/SF/silverfin_worspace/silverfin"
                                 [worktree] symlinks, templates, variables, hooks
```

- Layers: defaults, `config.toml`, the selected profile, the repository config (worktree
  commands only), environment, `--set`.
- `.tasq.toml` is never read. `--config` / `TASQ_CONFIG`, `[profile.<name>]` tables and
  `worktree.enabled` are gone; the last two are errors that name the replacement.
- `worktrees_enabled` (top level, `TASQ_WORKTREES_ENABLED`) is the feature switch.
- Hooks run without approval: no `trust.toml`, prompt, `tasq worktree trust` or `--yes`.
- `tasq config show` never shows a repository config; `tasq worktree config [DIR]` does.
- `tasq doctor` warns about a repository config whose `path` is not a main checkout.

## User Stories

1. As a user, I want every setting in one directory, so that I know where to look and can
   version or copy it as a whole.
2. As a user, I want a profile to be one file named after it, with the same shape as
   `config.toml`, so that a long profile does not hide inside the global file.
3. As a user, I want a leftover `[profile.x]` table to fail with its file:line and the file to
   move it to, so that a half-migrated config never silently loses a profile.
4. As a user, I want to keep selecting a profile with `TASQ_PROFILE` from `.envrc`, as today.
5. As a user, I want a repository's worktree settings in a file that names its main checkout,
   so that they apply wherever tasq runs and whichever profile is selected.
6. As a user, I want a repository config to win over the profile's `[worktree]` settings, so
   that a repository can differ from the profile's defaults.
7. As a user, I want a relative template `source` in a repository config to start at that
   file's directory, so that templates live in the config directory too.
8. As a user, I want a key other than `path` and `[worktree]` in a repository config, or two
   files with one `path`, to be an error, so that a mistake is caught on load.
9. As a user, I want my hooks to run without prompts or `--yes`, since I wrote all of them.
10. As a user, I want `worktrees_enabled = true` at the top level, separate from the
    `[worktree]` settings table, and an old `worktree.enabled` to tell me the new key.
11. As a user, I want `--config-dir` / `TASQ_CONFIG_DIR` to point tasq at another config
    directory, profiles and repository configs included, so that a second setup or a test is
    one directory.
12. As a user, I want `tasq worktree config [DIR]` to show the worktree settings that apply to
    a repository and where each came from.
13. As a user, I want `tasq doctor` to warn me when a repository config's `path` is not a main
    checkout, since such a file never applies.
14. As a plugin or hook, I want to receive `TASQ_CONFIG_DIR` when tasq was given one, so that
    the `tasq` I call reads the same configuration (replaces `TASQ_CONFIG`).

## Implementation Decisions

- **Config directory.** `LoadOptions::explicit_file` becomes `config_dir: Option<PathBuf>`
  (`--config-dir`, wins over `TASQ_CONFIG_DIR`). `global_file` becomes `config_dir(opts)`:
  explicit, else `$XDG_CONFIG_HOME/tasq`, else `~/.config/tasq`. A named directory that does not
  exist is `ConfigError::MissingExplicit` (reworded for a directory); the default one may be
  absent. `PROJECT_FILE`, `project_file` and the cwd walk are deleted; `LoadOptions::cwd` stays
  only if something else needs it (check the callers, otherwise remove).
- **Profiles.** `profiles/<name>.toml` is read only when `<name>` is selected; the list of
  names (`Loaded::profiles`, the "unknown profile" error) comes from the directory listing.
  `FileShape` loses its `profile` field; a `profile` key in `config.toml` or a profile file is
  a dedicated error naming file:line:column and `profiles/<name>.toml`, not the generic unknown
  key. `Origin::Profile { name, file }` keeps its shape; `Display` becomes `profile <name>
  (<file>)`.
- **Repository configs.** A new `RepoFileShape { path: String, worktree: WorktreeConfig }`
  with `deny_unknown_fields`; `WorktreeConfig` loses `enabled`, so the repository config cannot
  carry it. `LoadOptions` gains `repo: Option<PathBuf>` (the main checkout, already resolved by
  the caller through git; core never runs git). Loading lists `repos/*.toml`, expands `~` in
  each `path`, compares it with `repo` exactly (both made absolute, no canonicalisation in
  core), errors on two files with one `path` (all files are parsed, not only the match, so
  the duplicate shows whichever repository is asked about), and merges the match after the
  profile. New `Origin::Repo(PathBuf)`. `App::config_at(dir)` becomes
  `App::config_for_repo(main)`; `repo_settings` takes `templates_dir` from the file that set
  `worktree.templates`, as today.
- **`worktrees_enabled`.** A top-level `Config` field; every `config.worktree.enabled` read
  moves to it (`App::worktrees_enabled`, launch prompt, TUI model, JSON). `worktree.enabled`
  is a dedicated error naming file:line:column and the new key. Env `TASQ_WORKTREES_ENABLED`
  replaces `TASQ_WORKTREE_ENABLED`.
- **Trust removal.** Delete the trust store in `tasq_core::worktree::hooks` (pending,
  approvals, the prompt text), `RepoSettings::{repo_file, untrusted}`, the `Approver` passed
  from the CLI, `crates/cli/src/commands/worktree_hooks.rs` if only trust lives there, the
  `Trust` subcommand and every `--yes`. Hooks of every layer run.
- **`tasq worktree config [DIR]`.** Resolves `DIR` (default cwd) with `Repo::of` to its main
  checkout, loads with `repo` set, prints the `[worktree]` keys with their origin (the same
  rendering as `config show`, filtered to `worktree.`), then the matched repository config or
  `none`. `--json`. Hidden and refused like the other `worktree` commands when the switch is
  off.
- **Doctor.** One check per repository config: `Warn` when `path` does not exist, is not a git
  checkout, or is a linked worktree or a subdirectory rather than the main checkout. Runs git
  through the existing `tasq-worktree` helpers (injected env).
- **Plugins and hooks.** `plugins::env_for` forwards `TASQ_CONFIG_DIR` (absolute) instead of
  `TASQ_CONFIG`.

## Testing Decisions

- `crates/core/tests/config_load.rs`: the `.tasq.toml` tests are rewritten against a config
  directory (profile file, repository config match and no-match, duplicate `path`, unknown
  key in a repository config, `[profile.x]` error, `worktree.enabled` error, layer order
  profile < repository < env < `--set`). Exact error strings are asserted.
- CLI harness (`crates/cli/tests/support/mod.rs`): the isolated `HOME` gets
  `.config/tasq/{profiles,repos}`; helpers write a profile or a repository config. Snapshots
  for `config show`, `worktree config` (text and JSON), `doctor`.
- `crates/worktree/tests/hooks.rs`: trust cases deleted; a hook from a repository config runs
  without approval.
- A guard test or CI grep that no test, example or current doc mentions `.tasq.toml` outside
  ADRs, `docs/migration.md` and the plans (like the gwm grep of ADR 0022).
- Mutation bar per sub-task: zero missed in the diff (`--in-diff` with `--no-config`).

## Out of Scope

- Selecting a profile by path inside tasq (Q3 of the grilling; direnv does it).
- A migration command; the moves are listed in `docs/migration.md` and done by hand.
- Warning about leftover `.tasq.toml` files.
- Repository configs inside profiles, or matching by ancestor directory.

## Further Notes

The user's own setup moves after the last sub-task is installed: three profiles to
`~/.config/tasq/profiles/`, the two `.tasq.toml` files to `~/.config/tasq/repos/` with
`path = "~/code/tasq_lab/tasq"` and `path = "~/code/SF/silverfin_worspace/silverfin"`, the
silverfin `.envrc` template into the config directory, `worktree.enabled` to
`worktrees_enabled`. Ask before touching `~/.config/tasq`.

## Sub-tasks

### CC-001: the config directory

**What to build:** `--config-dir` / `TASQ_CONFIG_DIR` replace `--config` / `TASQ_CONFIG`;
`config.toml` is read from the config directory; plugins and hooks get `TASQ_CONFIG_DIR`.
`.tasq.toml` is still read in this step.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] `--config-dir` wins over `TASQ_CONFIG_DIR`, which wins over `$XDG_CONFIG_HOME/tasq`.
- [x] A named directory that does not exist fails with an exact message; an absent default does not.
- [x] `--config` and `TASQ_CONFIG` are gone from help and code; `TASQ_CONFIG_DIR` reaches plugins.
- [x] `mutants-in` the diff: zero missed.

### CC-002: profiles are files

**What to build:** `profiles/<name>.toml` loaded when selected; profile names from the
directory; `[profile.<name>]` anywhere is an error naming the file to move it to.

**Blocked by:** CC-001.

**Status:** done

- [x] Selected profile applies over `config.toml`; unknown profile lists the files' names.
- [x] `[profile.x]` in `config.toml` fails with file:line:column and `profiles/x.toml`.
- [x] `config show` names the profile file as the origin.
- [x] `mutants-in` the diff: zero missed.

### CC-003: `worktrees_enabled`

**What to build:** the top-level switch and env variable; `worktree.enabled` becomes an error
naming the new key; every reader moves over.

**Blocked by:** CC-001.

**Status:** done

- [x] `worktrees_enabled = true` turns the feature on (commands, JSON, TUI, prompt).
- [x] `worktree.enabled` fails with file:line:column and the new key; `TASQ_WORKTREES_ENABLED` works.
- [x] `mutants-in` the diff: zero missed.

### CC-004: repository configs replace `.tasq.toml`

**What to build:** `repos/*.toml` matched by exact main checkout, layered after the profile,
template sources relative to the file; the project-file layer and its walk are removed.

**Blocked by:** CC-002, CC-003.

**Status:** done

- [x] A repository config applies to its main checkout from any cwd and any profile, and to no other.
- [x] It wins over the profile and loses to env and `--set`.
- [x] Unknown key (including `enabled`), missing `path` and duplicate `path` fail with exact messages.
- [x] A relative template `source` resolves from the repository config's directory.
- [x] A `.tasq.toml` in the cwd or the repository has no effect.
- [x] `mutants-in` the diff: zero missed.

### CC-005: hooks run without trust

**What to build:** remove the trust store, the prompt, `tasq worktree trust` and every `--yes`.

**Blocked by:** CC-004.

**Status:** done

- [x] Hooks from a repository config run on `worktree up`, `next`, `done` without a prompt.
- [x] `trust` and `--yes` are gone from help; no code reads `trust.toml`.
- [x] `mutants-in` the diff: zero missed.

### CC-006: `tasq worktree config [DIR]`

**What to build:** the command, text and `--json`.

**Blocked by:** CC-004.

**Status:** done

- [x] Text and JSON snapshots: each `worktree.*` key with its origin, and the matched file or none.
- [x] `DIR` inside a linked worktree shows the main checkout's settings.
- [x] Hidden and refused with worktrees disabled.
- [x] `mutants-in` the diff: zero missed.

### CC-007: `tasq doctor` checks repository configs

**What to build:** one doctor check per repository config.

**Blocked by:** CC-004.

**Status:** done

- [x] `Warn` for a missing path, a non-repository, a linked worktree and a subdirectory, naming file and path.
- [x] `Ok` for a main checkout.
- [x] `mutants-in` the diff: zero missed.

### CC-008: the docs describe the config directory

**What to build:** `docs/config.md` (layers, env table, repository configs, worktree section),
`docs/migration.md` (the moves), `docs/plugins.md` (`TASQ_CONFIG_DIR`), `CLAUDE.md` (Config
section), the `.tasq.toml` guard grep.

**Blocked by:** CC-005, CC-006, CC-007.

**Status:** done

- [x] No current doc, test or example mentions `.tasq.toml`, `TASQ_CONFIG` or `worktree.enabled` outside ADRs, plans and the migration section.
- [x] `cargo doc` adds no warnings; the full suite passes.
