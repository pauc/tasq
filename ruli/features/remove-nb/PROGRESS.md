# Progress: remove the nb implementation

Plan: [PLAN.md](PLAN.md). Task: tasq 58. ADR: 0027.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RN-001 | The list-section op tests run on the current format (prefactor) | done | cd5f55f; 31 current-format pairs under `crates/core/tests/fixtures/v2/ops/` (every progress, worktree, session, merge request, description and clear-description scenario), derived from the v1 pairs with `## Tags`/`## Project`/`## Due` moved into the block (`## Notes` where that heading closed the section under test); the three scenario checks take a directory; `mod current_format` in `format_ops.rs` mirrors the ten free-standing list-section tests; `every_ops_fixture_round_trips` in `format_v2.rs`; no library change, no mutation run |
| RN-002 | `tasq migrate` is gone | ready | |
| RN-003 | The nb crate is gone | ready | |
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
