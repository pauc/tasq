# Progress: worktrees as an opt-in core feature

Plan: [PLAN.md](PLAN.md). Decision: [ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md)
(Proposed). nb todo 44. Started 2026-10-07.

## Status

**Planning.** Scope agreed with the author on 2026-10-07: core feature behind `worktree.enabled`,
full gwm replacement, then improvements from the prior-art survey (PLAN section 7). Plugin task
fields split out to nb todo 46. Open questions in PLAN section 8.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| WT-101 | The store can stop tracking a worktree | todo | |
| WT-102 | `[worktree]` config and the toggle | todo | |
| WT-103 | Invisible when disabled | todo | |
| WT-104 | Templates, variables and naming in core | todo | |
| WT-201 | `tasq-worktree` crate and `create` | todo | |
| WT-202 | Provisioning: symlinks, copies, templates | todo | |
| WT-203 | Hooks and trust | todo | |
| WT-204 | `add`, `ls`, `rm`, `switch`, `path`, `shell-init`, completions | todo | |
| WT-205 | Worktrees from `tasq create` | todo | |
| WT-206 | `import-gwm` and parity acceptance | todo | |
| WT-301 | Merged detection and safe removal | todo | |
| WT-302 | `prune` | todo | |
| WT-303 | Cleanup when a task closes | todo | |
| WT-304 | Status-rich `ls` | todo | |
| WT-305 | Fast copies of ignored files | todo | |
| WT-306 | Port blocks and database names | todo | |

## Learnings

- gwm facts (from `~/.local/bin/gwm`, read-only): `branch_safe` replaces every non-alphanumeric
  with `_`, collapses runs and trims; undefined `{{vars}}` are left in the output; `create`
  without `-b` falls back to `origin/<branch>` with `--track -b`; `remove` runs `post_remove`
  before `git worktree remove` and refuses a dirty tree without `-f`; template sources resolve
  against the workspace root (the directory holding `gwm.yml`).

## Blockers

None.

## Deviations from the plan

None yet.
