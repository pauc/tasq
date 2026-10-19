# Progress: remove the nb implementation

Plan: [PLAN.md](PLAN.md). Task: tasq 58. ADR: 0027.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RN-001 | The list-section op tests run on the current format (prefactor) | done | cd5f55f; 31 current-format pairs under `crates/core/tests/fixtures/v2/ops/` (every progress, worktree, session, merge request, description and clear-description scenario), derived from the v1 pairs with `## Tags`/`## Project`/`## Due` moved into the block (`## Notes` where that heading closed the section under test); the three scenario checks take a directory; `mod current_format` in `format_ops.rs` mirrors the ten free-standing list-section tests; `every_ops_fixture_round_trips` in `format_v2.rs`; no library change, no mutation run |
| RN-002 | `tasq migrate` is gone | done | 6c16e55; `crates/cli/src/commands/migrate.rs`, `Command::Migrate`, `From<MigrateError> for CliError`, the CLI `mod migrate` tests, their two snapshots and the `notebook`/`nb_fixture_dir`/`dir_bytes` helpers deleted; `crates/store/src/migrate.rs` and `crates/store/tests/migrate.rs` (with the `TASQ_UPDATE_FIXTURES` path) deleted, `thiserror` dropped from tasq-store; the nb crate's parity test `crates/store-nb/tests/migrate.rs` deleted; help snapshot loses the `migrate` line; `tasq migrate home` now fails like any unknown word (the `list` shortcut rejects the extra argument, exit 2); the fixture store is unchanged; `public_api.rs` never named migrate, so no change there; `mutants-in crates/store/src`: 165 mutants, 130 caught, 33 unviable, 2 timeouts (the known lock and name loops), 0 missed |
| RN-003 | The nb crate is gone | done | d4c4c3d; `crates/store-nb` deleted whole (37 files: the crate, its fixture notebook, its tests and the `TASQ_REQUIRE_NB` gate in `tests/support`), its workspace member and `Cargo.lock` entry gone; `tasq-store-nb` dropped from the `release.yml` publish loop and its order comment, `crates/store-nb/**` from `examine_globs` in `.cargo/mutants.toml`, the `feat(store-nb)` example from `cliff.toml`, and `store-nb` from the `std::process::Command` reason in `crates/core/clippy.toml` (now "see worktree, launch"; review caught that `tasq-store` runs no process); the CI "Install nb" step and `TASQ_REQUIRE_NB` env removed from `ci.yml`; the two comments naming the crate (`crates/core/src/store.rs` module doc, `crates/store/tests/public_api.rs` header) rewritten; no other crate depended on it; 1239 workspace tests pass; deletion only, no mutation run |
| RN-004 | Core has one task file format | ready | blocked by RN-001..003 |
| RN-005 | The `v2` names are gone (rename only) | ready | blocked by RN-004 |
| RN-006 | The docs describe what exists | ready | blocked by RN-002..005 |

## Learnings

- 2026-10-19: scope settled by grilling. Full removal including `tasq migrate` (every
  notebook the user wanted is already migrated; no release ever shipped migrate). The repo
  reads as if the nb implementation never existed: living surfaces rewritten, records (git
  history, CHANGELOG, `ruli/features/*`, ADR texts) kept. `IdScheme` dropped, `v2` names
  dropped, nb-only config guards dropped. ADR 0027 written, superseding 0002 and 0023 (0007
  was already superseded by 0023).
- 2026-10-19 (RN-001): the current-format pairs were derived mechanically from the v1 pairs,
  not from the code, and all matched on the first run. A dropped `## Tags` can remove the
  heading that closes the section under test (review caught it in
  `mr_related_exists_without_sub`); those pairs keep a `## Notes` section in its place.

## Blockers

None.

## Deviations

None.
