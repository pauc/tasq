# Progress: store sync as the `tasq-git` plugin

Plan: [PLAN.md](PLAN.md). Task: tasq 57. ADR: 0030. Blocked by tasq 59
([plugin-events](../plugin-events/PROGRESS.md)).

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| GS-001 | The `tasq-git` crate answers events and does nothing for a plain store | blocked | by tasq 59 |
| GS-002 | `tasq git init` with identity | blocked | |
| GS-003 | Writes are committed in the background | blocked | |
| GS-004 | Push, fetch and rebase in the background | blocked | |
| GS-005 | Conflicts are aborted and reported | blocked | |
| GS-006 | Duplicate ids are explained | blocked | |
| GS-007 | `tasq git sync`, `tasq git clone` and the doctor checks | blocked | |
| GS-008 | The docs describe store sync | blocked | |

## Learnings

- 2026-10-19: design settled by grilling. Store sync is a plugin (`tasq-git`, in this workspace,
  no tasq crate dependency) on extension points split out as tasq 59. A repository per store,
  branch per store on a shared remote, identity in the repository's own config, everything git
  in a detached job, only the rebase under the store lock, conflicts aborted, duplicate ids
  explained and never renumbered. nb 7.25.4 studied (repository per notebook, orphan branches
  on the remote only, blocking sync, no locking, markers committed). ADR 0030.

## Blockers

- tasq 59 (plugin extension points).

## Deviations

None yet.
