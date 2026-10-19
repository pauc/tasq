# Progress: plugin extension points

Plan: [PLAN.md](PLAN.md). Task: tasq 59. ADR: 0030.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| PE-001 | Plugins are enabled by name | ready | |
| PE-002 | The `store-open` event | ready | |
| PE-003 | The `post-write` event | ready | |
| PE-004 | Lasting plugin warnings and the `doctor` event | ready | |
| PE-005 | `tasq store lock -- <cmd>` | ready | |
| PE-006 | The TUI reloads when the store changes | ready | |
| PE-007 | The docs describe plugins and events | ready | |

## Learnings

- 2026-10-19: scope settled by grilling task 57. Store sync ships as the `tasq-git` plugin, so
  the extension points it needs come first, as their own task, designed and tested with a fake
  plugin. ADR 0030 written; CONTEXT.md gained Sync, Store sync, Plugin, Event and Hook.

## Blockers

None.

## Deviations

None yet.
