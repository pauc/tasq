# Progress: manual rank within a status group

Plan: [PLAN.md](PLAN.md). Task: tasq 43.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RK-001 | `Store::update_many` (prefactor) | ready | blocked by: none |
| RK-002 | Rank in the file orders lists and `next` | ready | blocked by: none |
| RK-003 | `tasq move --top` / `--unrank` | blocked | blocked by: RK-002 |
| RK-004 | `tasq move --before` / `--after` | blocked | blocked by: RK-001, RK-003 |
| RK-005 | Renumber a group when the gap runs out | blocked | blocked by: RK-004 |
| RK-006 | `ctrl+k` / `ctrl+j` in the TUI | blocked | blocked by: RK-004 |

## Learnings

- 2026-10-17: design settled by grilling; ADR 0025 and `CONTEXT.md` committed (d621eae).

## Blockers

## Deviations from the plan
