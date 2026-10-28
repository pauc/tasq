# ADR-0031: Configuration lives only in the config directory

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

tasq reads configuration from two kinds of place: the global file
(`~/.config/tasq/config.toml`, or the file named by `--config` / `TASQ_CONFIG`) and the
nearest `.tasq.toml` walking up from the current directory, stopping at home. Profiles are
`[profile.<name>]` tables inside any of those files. The worktree commands load the
configuration a second time from the repository's main checkout, so a `.tasq.toml` next to
a repository supplies its `[worktree]` settings wherever tasq runs (ADR-0022).

Forces:

1. **Settings are scattered.** The user's setup is one global file with three inline
   profiles (one of them with three `[[source]]` blocks) and two `.tasq.toml` files in
   workspace directories, each holding only `[worktree]` settings. Finding what applies
   means walking directories and reading tables inside tables.
2. **A file in a directory can come from someone else.** A `.tasq.toml` may arrive with a
   clone, so ADR-0022 made its hooks run only once approved: `trust.toml`, a terminal
   prompt, `--yes` on seven commands and `tasq worktree trust`. The user writes every
   `.tasq.toml` they have themselves.
3. **Profile selection already follows directories.** `TASQ_PROFILE` is exported from
   `.envrc` files in each workspace; tasq does not need to look at the directory for that.
4. **Worktree settings are per repository**, and the only directory that names a repository
   unambiguously is its main checkout, which the worktree commands already resolve.
5. **`worktree.enabled` is a feature switch inside a settings table.** It decides whether
   commands exist before any repository is known, unlike every other `worktree.*` key.

## Decision

tasq reads configuration only from the **config directory**: `--config-dir` /
`TASQ_CONFIG_DIR`, else `$XDG_CONFIG_HOME/tasq`, else `~/.config/tasq`. A named directory
must exist; everything inside it is optional. `.tasq.toml` is never read, and nothing warns
about one. `--config` and `TASQ_CONFIG` are removed.

```
~/.config/tasq/
├── config.toml           global config
├── profiles/<name>.toml  one profile per file
└── repos/<any>.toml      one repository config per file
```

**Profiles.** `profiles/<name>.toml` is the profile `<name>`; its body has the shape of
`config.toml` (no wrapper table, no nested profiles). A `[profile.<name>]` table anywhere is
an error naming its file, line and column and the file to move it to. Selection is
unchanged: `--profile` or `TASQ_PROFILE`.

**Repository configs.** A file in `repos/` holds a required `path`, the main checkout of
one repository, and a `[worktree]` table; any other key is unknown. The name of the file is
free. It applies when tasq works on worktrees of the repository whose main checkout is
exactly `path`; there is no walk up. Two files with the same `path` are a load error naming
both. A relative template `source` starts at the directory of the repository config. The
layers are: defaults, `config.toml`, profile, repository config, environment, `--set`, so a
repository's own settings win over a profile's general ones.

**`worktrees_enabled`.** The feature switch becomes a top-level key, `worktrees_enabled`
(`TASQ_WORKTREES_ENABLED`), and cannot be set in a repository config. `worktree.enabled` is
an error naming the new key.

**No hook trust.** Every hook comes from a file the user wrote in their config directory,
so hooks run without approval. `trust.toml`, the prompt, `tasq worktree trust` and `--yes`
on `next`, `pick`, `done`, `worktree up`, `worktree release`, `worktree prune` and
`worktree switch` are removed.

**Seeing it.** `tasq config show` shows the defaults, `config.toml`, the profile, the
environment and `--set`, never a repository config. `tasq worktree config [DIR]` shows the
effective `[worktree]` table for the repository `DIR` is in, the layer behind each value and
the repository config that matched. `tasq doctor` warns about a repository config whose
`path` is not a main checkout.

## Consequences

### Positive

- One directory holds every setting; it can be versioned or copied as a whole.
- Loading no longer depends on the directory tasq runs in, only on the profile and, for the
  worktree commands, the repository.
- The trust store, its prompt, a command and seven `--yes` flags go away.
- Each profile and repository config is its own file, so errors point at short files.

### Negative

- Settings no longer travel with a clone; a teammate cannot share worktree settings through
  the repository. tasq is a personal tool, and nobody shares one today.
- A repository config whose `path` has a typo silently never applies. `tasq doctor` reports
  it.
- Moving a repository means editing its `path`.
- Existing setups break on purpose: inline profiles, `worktree.enabled` and `--config` are
  errors, and `.tasq.toml` stops applying. `docs/migration.md` lists the moves.

## Alternatives considered

- **Keep `.tasq.toml` as an optional extra.** Keeps the walk, the provenance case and the
  trust machinery for a feature the user does not want.
- **Repository settings as path-keyed tables in `config.toml` or a profile.** Same matching,
  one long file; separate files read better and fail with shorter locations.
- **Repository settings inside the profile.** Simplest, but applies to every repository of
  the profile and needs the profile selected when `tasq worktree up` runs.
- **Match a repository config by ancestor directory, like the old walk.** One file could cover a
  workspace, but which file applies becomes a search again; exact main checkouts are
  unambiguous.
- **Select the profile by path inside tasq.** direnv already does it; deferred.
- **`[features] worktrees`.** Only worth a table if more switches appear.

## References

- Task tasq 60, GitHub issue pauc/tasq#13.
- Plan: `ruli/features/compact-config/PLAN.md`.
- ADR-0006 (plugins): plugins get `TASQ_CONFIG_DIR` in place of `TASQ_CONFIG`.
- ADR-0022 (worktrees): the `.tasq.toml` lookup from the main checkout and the hook trust are
  superseded here.
- `docs/config.md`, `docs/migration.md`.
