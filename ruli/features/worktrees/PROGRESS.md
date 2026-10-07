# Progress: worktrees as an opt-in core feature

Plan: [PLAN.md](PLAN.md). Decision: [ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md)
(Proposed). nb todo 44. Started 2026-10-07.

## Status

**Planning.** Scope agreed with the author on 2026-10-07: core feature behind `worktree.enabled`,
full gwm replacement, then improvements from the prior-art survey (PLAN section 7). Plugin task
fields split out to nb todo 46. Lifecycle settled the same day (PLAN 4.2): declare at creation,
create at launch, release at close, never remove the entry.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| WT-101 | Lifecycle decisions in core | done | `tasq_core::worktree::lifecycle`: `declare`, `on_launch`, `on_close` (`DirSeen`/`BranchSeen` in, `Release { WorktreeFate, BranchFate }` out); 10 table tests; 14 mutants, 11 caught, 3 unviable |
| WT-102 | `[worktree]` config and the toggle | done | `WorktreeConfig` (+ `OnCreate`, `OnDone`, `TemplateFile`, `PortsConfig`, `WorktreeHooks`), 7 `TASQ_WORKTREE*` vars, `BadPorts`; `work.worktree_*` rejected by a `deserialize_with` guard (file:line:column); `CommandManager`/`manager_for`/`expand_template` deleted; docs, examples, CI `no-gwm` job; 17 mutants, 16 caught, 1 unviable |
| WT-103 | Invisible when disabled | todo | |
| WT-104 | Templates, variables and naming in core | todo | |
| WT-201 | Declaring: `tasq create --worktree` and `tasq worktree <id> [BRANCH]` | todo | |
| WT-202 | Materializing at launch: the `tasq-worktree` crate | todo | |
| WT-203 | Provisioning: symlinks, copies, templates | todo | |
| WT-204 | Hooks and trust | todo | |
| WT-205 | Integration checks | todo | |
| WT-206 | Releasing at close | todo | |
| WT-207 | `ls`, `switch`, `path`, `shell-init`, completions | todo | |
| WT-208 | Switch the author's workspaces off gwm (manual acceptance) | todo | |
| WT-301 | `prune` | todo | |
| WT-302 | Status-rich `ls` | todo | |
| WT-303 | Fast copies of ignored files | todo | |
| WT-304 | Port blocks and database names | todo | |

## Learnings

- WT-101: clippy's `struct_excessive_bools` shaped the close input into two enums. `DirSeen`
  encodes the precedence main > locked > dirty, so the caller checks in that order and can skip
  `git status` for the main checkout. `on_launch` reuses the first existing worktree before
  creating the first declared one (today's `resolve_workdir` order). For WT-206: a directory
  removed by hand leaves git's worktree registration behind, and `git branch -d` refuses a
  branch "checked out" there, so release must run `git worktree prune` before deleting a branch
  whose directory is `Absent`.
- WT-102: removed keys are private `Removed` fields whose `deserialize_with` always fails, so
  the error keeps toml's span. Via `--set`/env they are simply unknown keys. Validating that
  templates only name known variables waits for the renderer (WT-104). An empty `[work]` table
  now "explains" its unset children (`Loaded::explain`), as `[launch.claude]` already did.
- Defaults confirmed by the author 2026-10-07: `on_create = "auto"`, `on_done = "release"`,
  `delete_branch = "safe"`.
- gwm facts (from `~/.local/bin/gwm`, read-only): `branch_safe` replaces every non-alphanumeric
  with `_`, collapses runs and trims; undefined `{{vars}}` are left in the output; `create`
  without `-b` falls back to `origin/<branch>` with `--track -b`; `remove` runs `post_remove`
  before `git worktree remove` and refuses a dirty tree without `-f`; template sources resolve
  against the workspace root (the directory holding `gwm.yml`).

## Blockers

None.

## Deviations from the plan

- 2026-10-07: the first WT-101 (a `format::ops::remove_worktree` op plus store-diff removals,
  fbeda83) was reverted in 56b683b after the author settled that entries are never removed; the
  plan was rewritten around the declare / materialize / release lifecycle.
