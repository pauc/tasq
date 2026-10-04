# Progress: tasq rewrite — Phases 0 and 1

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-001 | Workspace and quality gates | in progress | wave 1 |
| T-002 | CI pipeline | pending | wave 2 |
| T-003 | ADRs | pending | wave 2 |
| T-004 | cargo-mutants setup | pending | wave 2 |
| T-101 | Domain model | pending | wave 3 (with T-105) |
| T-105 | Clock and progress logging | pending | wave 3 |
| T-102 | Markdown parser | pending | wave 4 (with T-103) |
| T-103 | Markdown writer and round trip | pending | wave 4 |
| T-104 | Queries and grouping | pending | wave 4 |
| T-106 | Configuration loading | pending | wave 4 |

## Wave plan

1. T-001 alone: everything else needs the workspace. Pre-creates the `justfile` and stub
   modules so later parallel agents own disjoint files.
2. T-002, T-003, T-004 in parallel (disjoint files: `.github/`, `docs/adr/`, `.cargo/mutants.toml` + justfile recipes).
3. T-101 + T-105: the model everything in Phase 1 depends on. Declares `format`, `query`, `config` modules as stubs.
4. T-102+T-103 (one agent), T-104, T-106 in parallel, each owning one module directory.

## Learnings

(none yet)

## Blockers

(none yet)

## Deviations from the plan

(none yet)

## Open questions raised during implementation

(none yet)
