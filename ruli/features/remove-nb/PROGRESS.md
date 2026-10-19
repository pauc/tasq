# Progress: remove the nb implementation

Plan: [PLAN.md](PLAN.md). Task: tasq 58. ADR: 0027.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RN-001 | The list-section op tests run on the current format (prefactor) | ready | |
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

## Blockers

None.

## Deviations

None.
