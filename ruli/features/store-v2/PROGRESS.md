# Progress: store v2

Plan: [PLAN.md](PLAN.md). Task: tasq 56. ADR: 0026.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| SV-001 | CLI talks to the `Store` trait; `store sync` / `store reconcile` go (prefactor) | ready | |
| SV-002 | `tasq-store` crate skeleton, revision and diff moved (prefactor) | ready | |
| SV-003 | Core reads and renders a v2 task file | ready | |
| SV-004 | Core writes v2: new documents and one op per key | blocked (SV-003) | |
| SV-005 | The tasq store reads a directory | blocked (SV-002, SV-003) | |
| SV-006 | Create under the store lock | blocked (SV-004, SV-005) | |
| SV-007 | Update with conflict detection, and doctor | blocked (SV-006) | |
| SV-008 | Migrate as a library, and the committed v2 fixture store | blocked (SV-006) | |
| SV-009 | The CLI opens the tasq store | blocked (SV-001, SV-007, SV-008) | |
| SV-010 | Creation time and store provenance surfaces | blocked (SV-009) | |
| SV-011 | `tasq migrate <notebook> [--to] [--dry-run]` | blocked (SV-008, SV-009) | |
| SV-012 | Docs, agent surfaces and the hand-over | blocked (SV-010, SV-011) | |

## Learnings

- 2026-10-18: design settled by grilling (four rounds); ADR 0026 and the `CONTEXT.md` terms
  written and committed (868a1c7). Git is out of the store entirely (task 57 brings it back as opt-in); ULID file names
  carry the creation time so there is no `created:` key; no marker file, no `store.kind`, no
  `store init`; stores live at `<data_dir>/<store.name>`.

## Blockers

- One cargo build at a time on this machine (`scripts/guard`); SV-001, SV-002 and SV-003 are
  independent but are worked one after another, not in parallel agents.
