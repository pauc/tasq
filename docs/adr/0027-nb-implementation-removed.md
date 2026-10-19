# ADR-0027: tasq owns its store: the nb implementation is removed

- **Status:** Accepted
- **Date:** 2026-10-19

## Context

ADR-0026 moved the CLI to a tasq-owned store directory and kept `crates/store-nb`
only as the dependency of `tasq migrate`, "until a follow-up task deletes it
together with the v1 reader and the parity tests". That follow-up is tasq 58.

By then every notebook the author wanted converted had been migrated (Tasq,
Silverfin). The notebooks left in nb are not going to be converted. No tasq
release exists (version `0.1.0`, no tag), so nobody else has a `tasq migrate`
to depend on.

What the nb implementation still costs:

1. **A second file shape in core.** Every scalar op in `format::ops` dispatches
   on `Document::is_v2`; `Document::from_task`, the v1 scalar-section reader and
   the v1 fixtures exist only for migrate and for tests.
2. **A crate nothing runs.** `crates/store-nb` (NbStore, the bookkeeper and its
   job, git, index, resolve, doctor) is built, tested, mutated, published by the
   release workflow and installs nb in CI.
3. **Vocabulary that no longer means anything.** `IdScheme::Positional`,
   `ids_may_change_on_reconcile`, the removed `store.*` config keys with their
   tailored errors, `TASQ_NOTEBOOK`, "notebook" in the glossary and the docs.
4. **v2 names.** `parse_v2`, `from_task_v2`, `fixtures/v2/`: a version suffix
   on the only format there is.

## Decision

The nb implementation is removed entirely, `tasq migrate` included. After tasq
58 the code, tests, fixtures, docs, glossary and plugin prompts read as if tasq
never had an nb-backed store:

- `crates/store-nb`, its fixtures, the parity tests, `TASQ_REQUIRE_NB`, the CI
  nb install and its entries in the workspace, the release publish list, the
  mutants config and `cliff.toml` are deleted.
- `tasq migrate`, `tasq_store::migrate` and the fixture store's generator are
  deleted. The committed fixture store stays as a hand-maintained v2 store.
- Core keeps one task file format: the v1 reader, `Document::from_task`, the v1
  branches of `format::ops`, the v1 fixtures and the v1 property arm go, and
  the `v2` suffixes are dropped from names, files and docs.
- `IdScheme` and `ids_may_change_on_reconcile` go: `StoreInfo` is `name`,
  `location`, `task_count`, `bookkeeper`, and `store info` loses its `ids:`
  line and the two JSON keys.
- The guards that existed only because of nb go: the `Removed` config keys
  (`store.kind`, `store.notebook`, `store.sync_interval`), the `TASQ_NOTEBOOK`
  rejection, the plugin test that bans `.todo.md`, `.index` and `notebook`.
  An old key gets the generic unknown-field error.

Records stay as they are: git history, `CHANGELOG.md`, `ruli/features/*` and
the ADRs. `original/tasks` is still described as the nb-based bash script tasq
rewrites, because it is.

## Consequences

### Positive

- One format, one store, one reader in core; the ops lose their shape branch.
- Less to build, test, mutate and publish: one crate and nb in CI gone.
- The docs describe what exists, without a "v1, read by migrate only" layer.

### Negative

- An nb notebook can no longer be imported. Mitigated: no notebook is waiting,
  and the last commit with `tasq migrate` stays in git history.
- The fixture store can no longer be regenerated; it is edited by hand like any
  other fixture.
- The list-section op tests ran on v1 fixtures only; they are moved onto v2
  fixtures before anything is deleted, so coverage does not drop.
- `store info --json` loses `id_scheme` and `ids_may_change_on_reconcile`.
  Nothing in the repo reads them and no release has shipped them.

## Alternatives considered

- **Keep a minimal read-only v1 parser for `tasq migrate`** — keeps a second
  parser in core, its fixtures and its tests for conversions nobody needs.
- **Keep `IdScheme` with `Stable` as its only variant** — a constant in the
  JSON; worth it only if the JSON could stay unchanged, and it cannot.
- **Delete the nb ADRs too** — removes the reason the store is a ULID
  directory with the id in the file; ADRs are superseded, not deleted.

## References

- Supersedes ADR-0002 (nb-compatible store) and ADR-0023 (no nb at runtime;
  it already superseded ADR-0007). Completes ADR-0026.
- tasq 58 ("Delete the nb store: tasq-store-nb, the v1 format and the parity
  tests"); plan in `ruli/features/remove-nb/`.
