# Progress: worktrees as an opt-in core feature

Plan: [PLAN.md](PLAN.md). Decision: [ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md)
(Proposed). nb todo 44. Started 2026-10-07.

## Status

**Phase 1 done (2026-10-07).** Scope agreed with the author on 2026-10-07: core feature behind `worktree.enabled`,
full gwm replacement, then improvements from the prior-art survey (PLAN section 7). Plugin task
fields split out to nb todo 46. Lifecycle settled the same day (PLAN 4.2): declare at creation,
create at launch, release at close, never remove the entry.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| WT-101 | Lifecycle decisions in core | done | `tasq_core::worktree::lifecycle`: `declare`, `on_launch`, `on_close` (`DirSeen`/`BranchSeen` in, `Release { WorktreeFate, BranchFate }` out); 10 table tests; 14 mutants, 11 caught, 3 unviable |
| WT-102 | `[worktree]` config and the toggle | done | `WorktreeConfig` (+ `OnCreate`, `OnDone`, `TemplateFile`, `PortsConfig`, `WorktreeHooks`), 7 `TASQ_WORKTREE*` vars, `BadPorts`; `work.worktree_*` rejected by a `deserialize_with` guard (file:line:column); `CommandManager`/`manager_for`/`expand_template` deleted; docs, examples, CI `no-gwm` job; 17 mutants, 16 caught, 1 unviable |
| WT-103 | Invisible when disabled | done | `App::worktrees`/`task_json` + `json::task(s)` (key dropped), `cli::command` hides `worktree` and swaps help paragraphs, `cli::completion_command` prunes it, `early_globals` loads config before clap, `LaunchContext::worktrees` + `{{#worktrees_enabled}}`, `Model::with_worktrees`, apply keeps stored worktrees on a missing key; `worktrees_disabled` CLI tests; 7 mutants, 7 caught |
| WT-104 | Templates, variables and naming in core | done | `tasq_core::template` (hand parser replacing the launch regexes, `names()`), `worktree::vars` (`Scope`, `check`, `Vars`, `branch_safe`, `branch_db`, `slug`, `repo_name`, `port` via FNV-1a); config rejects unknown template variables per scope and bad `[worktree.variables]` names; 102 mutants in template + worktree, 96 caught, 6 unviable |
| WT-201 | Declaring: `tasq create --worktree` and `tasq worktree <id> [BRANCH]` | done | af9b74c, 012c28b, then the TUI create view. `worktree::declaration` (`declare`, `wanted`, `is_path`), `worktree::git` (`work_tree_root`, `gitdir`, `origin_url`), `lifecycle::branch_name`; CLI `Repo::of` probes `.git` without a process; `--create` hidden; TUI `c` opens the edit view empty (`Form::new_task`, Worktree box), `Host::worktree`, `edit::Fields::draft`; 906 tests green; 0 missed mutants (core 82+8, CLI 23, TUI slice 64) |
| WT-202 | Materializing at launch: the `tasq-worktree` crate | done | fa33080 + CLI wiring. `tasq_worktree::Git` (`branch_state` local / `ls-remote` + fetch / offline `origin/<b>` / absent, `resolve_start`, `add` with cleanup, `current_branch`); `materialize` in `next`/`pick` before the status change, `tasq worktree up [--from] [--dry-run]`, `--create` = declare + create; recreate prompt, `GitManager`, `WorktreeManager`, `work::{branch_slug, sibling_worktree}` deleted; 915 tests green; 0 missed mutants (crate 27, diff 33) |
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
- WT-103: `--help` and completions run before any command, so `main` loads the config once
  early (`cli::early_globals` pre-scans `--config/--profile/--set`; a broken config counts as
  disabled there and the command reports it). clap_complete's zsh generator lists hidden
  subcommands, so completions use a rebuilt command without `worktree`. When disabled,
  `{{markdown}}` in the prompt still quotes the file verbatim, `## Worktrees` included; only
  users who enabled and then disabled the feature can see that.
- WT-104: the first `next_tag` advanced an index by hand; three mutants of that arithmetic only
  timed out (infinite loops). Iterating `char_indices` removed them. `port` is FNV-1a 64 over
  `project NUL branch`; the pinned values were checked against a Python implementation, not the
  Rust one. Variable scopes follow the lifecycle: `branch` cannot use `{{branch}}`, `path`
  cannot use `{{worktree}}`, and `{{port}}` exists only with `[worktree.ports]`. `~` in a
  rendered path is the caller's to expand (WT-201).
- WT-201: the default branch needs `{{id}}`, which the store assigns, so `tasq create` writes the
  task and then updates it with the worktree (two writes). Templates see the repository's main
  checkout as `{{project}}` (the parent of the common `.git` dir), so a project that is a linked
  worktree or a subdirectory does not nest names; a `--separate-git-dir` checkout is its own
  main. An argument of `tasq worktree <id>` is a path when it is a directory, starts with `/` or
  `.`, or contains `~` (git refuses those in branch names), so typos are not declared as
  branches. clap_complete's zsh generator lists hidden arguments too: completions rebuild
  `create` without `--worktree` when disabled. `.cargo/mutants.toml` excludes the CLI and TUI
  crates; their slices were run with `--no-config`. A rendered path keeps `..` as written.
- WT-202: `on_launch` passes over declarations without a branch (directories tracked by hand):
  entries are never removed, so an error there would block the task's launches forever; the
  CLI warns and starts in the project as before. A new branch uses `--no-track`: started from
  `origin/main` with the default `autoSetupMerge` it would otherwise push to `main`. `git init` +
  `remote add` leaves no `origin/HEAD` (only clones set it), so the default base falls back to
  `HEAD` with a warning, decided before the progress line so `--dry-run` shows the real start.
  `ls-remote --exit-code` exits 2 when the remote answers without the branch; any other failure
  (no `origin`, offline) falls back to an already fetched `origin/<b>`. The worktree is created
  before the task is set in-progress, so a git failure leaves the file untouched. `tasq worktree
  up` needs `args_conflicts_with_subcommands` next to the `<ID> [TARGET]` positionals.
- Defaults confirmed by the author 2026-10-08 (TUI): `c` opens a full create view like the edit
  view, not the title prompt; the title prompt and its history are gone.
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
- 2026-10-08: WT-201's "TUI create form gets a worktree field" grew, at the author's request, into
  replacing the `c` title prompt with a full create view (the edit view, empty).
- 2026-10-08: WT-202 dropped `LifecycleError::NoBranch`; `on_launch` returns a `LaunchAction`
  (no `Result`) and skips entries without a branch instead of failing (see Learnings).
