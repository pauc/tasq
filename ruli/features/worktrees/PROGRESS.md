# Progress: worktrees as an opt-in core feature

Plan: [PLAN.md](PLAN.md). Decision: [ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md)
(Proposed). nb todo 44. Started 2026-10-07.

## Status

**Phase 1 done (2026-10-07); Phase 2 done (2026-10-08); Phase 3 started (WT-301 done 2026-10-09).** Scope agreed with the author on
2026-10-07: core feature behind `worktree.enabled`, full gwm replacement, then improvements from the
prior-art survey (PLAN section 7). Plugin task fields split out to nb todo 46. Lifecycle settled
the same day (PLAN 4.2): declare at creation, create at launch, release at close, never remove the
entry.

Where things stand (2026-10-08, after WT-207):

- Branch `feat/worktrees`, 33 commits ahead of `main` (with the WT-207 progress commit), not pushed, no PR. It also carries
  commits that are not worktree work: four from another session, e9e01ff (store-nb never runs
  nb, ADR 0023), d667330, 674eec3, eca2fe0, and one side commit, a6abbd9 (`tasq apply`
  help). Two more side commits (`docs/agents/*`, the CLAUDE.md agent skills section) went to
  `main` through PR #8 as be1a1c2 and 292feb6 and left the branch in the 2026-10-13 rebase.
  ADR 0022 is still Proposed.
- 2026-10-12: rebased onto `main` for PR #7 (review S-5) and dropped fbeda83 and its revert
  56b683b (see Deviations). That pre-rebase branch is kept as `backup/feat-worktrees-pre-drop`.
- 2026-10-13: rebased onto `main` again (ed41ab2, PR #8 merged); the two `docs/agents` commits
  were dropped as already on `main`, the tree is unchanged. Every sha in this file and in
  PLAN.md is the one after this rebase; the previous branch is kept as
  `backup/feat-worktrees-pre-main-rebase`.
- The whole lifecycle works end to end: declaring (`tasq create --worktree`, `tasq worktree <id>
  [BRANCH|PATH]`), creating at launch with provisioning and hooks (`next`/`pick`, `tasq
  worktree up`), and releasing at close (`tasq done [--yes]`, the TUI's `d`, `tasq worktree
  release <id> [--force] [--dry-run] [--yes]`), and moving around (`tasq worktree ls`,
  `switch`, `path`, `shell-init`).
- The installed `tasq` is bb1e820 from this branch (f9e0a2f after the rebase), reinstalled at the author's request on
  2026-10-08: launching creates worktrees, `tasq done` releases them. Ask before reinstalling.
- 2026-10-09: WT-301 done (`tasq worktree prune`), 40 commits ahead of `main`. Next: WT-302,
  WT-303 or WT-304 (independent). The installed `tasq` is still bb1e820 and has no `prune`;
  ask before reinstalling.
- 2026-10-13: PR #7 review fixes done (every finding in `REVIEW-PR7.md`, up to 626408a). Final
  checks at 626408a: fmt and clippy clean, `TASQ_REQUIRE_NB=1 cargo test --workspace` 1078
  passed, 0 failed, 1 ignored; `cargo mutants --in-diff` on `main...HEAD` 591 mutants, 518
  caught, 73 unviable, 0 missed.
- Cleanup the author must do: a WT-207 test run on 2026-10-08 20:21 created
  `/home/pau/code/tasks-wt/feature-a` (a worktree of a deleted temp repository: `.git`,
  `README`, `.tasq.toml`) and its parent `/home/pau/code/tasks-wt`. Deleting them was refused
  by the permission classifier; `rm -r /home/pau/code/tasks-wt` removes both. Cause and fix in
  WT-207 Learnings.
- Known flake, not worktree work: `commands::ui::tests::sync_passes_the_picked_sources_as_source_flags`
  failed once in a full `cargo test --workspace` (2026-10-08) and passed alone three times. It
  writes a script and runs it without the ETXTBSY probe `TestEnv::fake_tool` has.
- WT-208 needs the author's real `gwm.yml` files, which live outside this repository; reading them
  needs the author's go-ahead (repository rule: never read outside the working directory).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| WT-101 | Lifecycle decisions in core | done | `tasq_core::worktree::lifecycle`: `declare`, `on_launch`, `on_close` (`DirSeen`/`BranchSeen` in, `Release { WorktreeFate, BranchFate }` out); 10 table tests; 14 mutants, 11 caught, 3 unviable |
| WT-102 | `[worktree]` config and the toggle | done | `WorktreeConfig` (+ `OnCreate`, `OnDone`, `TemplateFile`, `PortsConfig`, `WorktreeHooks`), 7 `TASQ_WORKTREE*` vars, `BadPorts`; `work.worktree_*` rejected by a `deserialize_with` guard (file:line:column); `CommandManager`/`manager_for`/`expand_template` deleted; docs, examples, CI `no-gwm` job; 17 mutants, 16 caught, 1 unviable |
| WT-103 | Invisible when disabled | done | `App::worktrees`/`task_json` + `json::task(s)` (key dropped), `cli::command` hides `worktree` and swaps help paragraphs, `cli::completion_command` prunes it, `early_globals` loads config before clap, `LaunchContext::worktrees` + `{{#worktrees_enabled}}`, `Model::with_worktrees`, apply keeps stored worktrees on a missing key; `worktrees_disabled` CLI tests; 7 mutants, 7 caught |
| WT-104 | Templates, variables and naming in core | done | `tasq_core::template` (hand parser replacing the launch regexes, `names()`), `worktree::vars` (`Scope`, `check`, `Vars`, `branch_safe`, `branch_db`, `slug`, `repo_name`, `port` via FNV-1a); config rejects unknown template variables per scope and bad `[worktree.variables]` names; 102 mutants in template + worktree, 96 caught, 6 unviable |
| WT-201 | Declaring: `tasq create --worktree` and `tasq worktree <id> [BRANCH]` | done | e060496, 157af84, 728f01f (TUI create view). `worktree::declaration` (`declare`, `wanted`, `is_path`), `worktree::git` (`work_tree_root`, `gitdir`, `origin_url`), `lifecycle::branch_name`; CLI `Repo::of` probes `.git` without a process; `--create` hidden; TUI `c` opens the edit view empty (`Form::new_task`, Worktree box), `Host::worktree`, `edit::Fields::draft`; 906 tests green; 0 missed mutants (core 82+8, CLI 23, TUI slice 64) |
| WT-202 | Materializing at launch: the `tasq-worktree` crate | done | 5b21eff (crate), 774f91c (CLI wiring); `crates/worktree/**` added to `.cargo/mutants.toml` and the release crate list. `tasq_worktree::Git` (`branch_state` local / `ls-remote` + fetch / offline `origin/<b>` / absent, `resolve_start`, `add` with cleanup, `current_branch`); `materialize` in `next`/`pick` before the status change, `tasq worktree up [--from] [--dry-run]`, `--create` = declare + create; recreate prompt, `GitManager`, `WorktreeManager`, `work::{branch_slug, sibling_worktree}` deleted; 915 tests green; 0 missed mutants (crate 27, diff 33) |
| WT-203 | Provisioning: symlinks, copies, templates | done | 15ed940 (core `provision::plan`, three workspace fixture configs), 9c3de7f (`tasq_worktree::provision::{prepare, apply}`, tree checked against fixture files), e3fb16e (CLI): `RepoSettings` loads `[worktree]` from the main checkout (`App::config_at`), templates relative to the file that set them, rendered before `git worktree add`; dry runs list the steps; 0 missed mutants (core 4+2, crate 7+1, CLI diff 13+10) |
| WT-204 | Hooks and trust | done | 457c559, 6ca514d (core `hooks`: `Kind`, `argv`, `env`, `trust_file`, `TrustStore`) + runner (`tasq_worktree::hooks::run_hook`) and CLI (`worktree_hooks`: pending lines from the repository's own file, `--yes`, terminal prompt, `tasq worktree trust [DIR]`; pre-create in the main checkout before git, post-create in the worktree); `next`/`pick` dispatch moved out of `app::run`; 947 tests green; 0 missed mutants (93: 83 caught, 10 unviable) |
| WT-205 | Integration checks | done | core `worktree::integration` (`Check` cheapest first, `first_passing`, `targets`, `pushed_ref`, `cherry_all_upstream`, `merge_tree`, `Safety { integrated, pushed }` with `seen()` and `reason()`), c08b91c; `tasq_worktree::Git::safety`, e34a43b; 12 temp-repo tests (one per check, pushed with and without upstream, neither, local `main` fallback, the default branch itself, no local branch); 967 tests green; 0 missed mutants (76: 67 caught, 9 unviable) |
| WT-206 | Releasing at close | done | 9fbc1b7 (core), dfcf827 (crate), 6932a22 (CLI, docs). core: `on_close(.., force)`, `DirSeen`/`KeepWorktree` gain `Foreign` and `InUse`, `KeepWorktree::reason`/`KeepBranch::reason`, `status` (`listed`, `changes` without provisioned paths, `dir_seen`), `provision::Step::target`; crate: `Git::{worktrees, changes, remove, prune, delete_branch}`; CLI: `worktree::release` (`Releasing`), `tasq worktree release`, `done --yes`, TUI `after_done`; `worktree_hooks::Approval` (`Skip` for the TUI) and an event sink instead of printing; 982 tests green; 0 missed mutants (diff: 67, 60 caught, 7 unviable) |
| WT-207 | `ls`, `switch`, `path`, `shell-init`, completions | done | 98f5a78 (core `worktree::target`: `parse`, `by_branch`; `hooks::state_file`), 5221cd4 (CLI `worktree_nav`: `ls [ID] [--all]`, `switch [TARGET] [--yes]` with fzf or a numbered list, `path`, `shell-init bash|zsh|fish` through `$TASQ_CD_FILE`, `--json` for each; completions test; docs/json.md, docs/config.md); 994 tests green; 0 missed mutants (diff 61 + nav/status 67) |
| WT-208 | Switch the author's workspaces off gwm (manual acceptance) | done | 6c8f966, 34610f9. tasq_lab and silverfin: `.tasq.toml` written, hooks trusted, full cycle passed twice each on the real notebook (tasks 51, 52; see Learnings). zantop_atelier dropped and gwm kept installed by the author (see Deviations) |
| WT-301 | `prune` | done | b6fb37b (core), 1305bec (crate), 8363670 (CLI, docs). core `worktree::prune::plan` (scope: done tasks to release, repositories to prune, `here` first); `tasq_worktree::Git::prune(repo, dry_run)` now `--verbose` and returns git's lines; CLI `worktree_nav::prune` (`tasq worktree prune [--all] [--dry-run] [--yes]`, `--json`), docs/config.md, docs/json.md; 998 tests green; 0 missed mutants (diff: 19, 18 caught, 1 unviable) |
| WT-302 | Status-rich `ls` | todo | |
| WT-303 | Fast copies of ignored files | todo | `worktree.copy_ignored` is rejected in config since fd5c8f8 (PR #7 review P-5: it parsed but did nothing). WT-303 re-adds it: the `WorktreeConfig` field (`not_yet_copy_ignored` goes), the `docs/config.md` row and example line, the `config show` snapshots, and replaces `copy_ignored_is_rejected_until_it_is_implemented` |
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
- WT-203: the fixtures are modelled on the plan's description of the author's three workspaces,
  not on their `gwm.yml` files (outside the repository; WT-208 converts the real ones). Hook
  keys are kebab-case (`post-create`), as `docs/config.md` always said; PLAN 4.4 showed
  snake_case and is fixed. Templates render in `prepare`, before git runs, so a broken template
  costs nothing; a filesystem failure in `apply` leaves the created worktree and says so. The
  per-repository config is loaded from the main checkout for declaring too, so `worktree.path`
  in a repository's `.tasq.toml` applies when tasq runs elsewhere. Copies come from the main
  checkout, not the task's project subdirectory, since the worktree is a checkout of the whole
  repository.
- WT-205: the targets are the ref `origin/HEAD` points to, then the local branch of that name
  (a merge done locally counts); without `origin/HEAD`, the local `main` else `master`. The
  branch itself is never a target, so a worktree on `main` is never "integrated into itself".
  "Pushed" is the upstream when it is a remote-tracking ref, else `origin/<branch>`: new
  branches use `--no-track`, and `git push` without `-u` sets no upstream. A local upstream does
  not count. Every check reads the remote-tracking refs as last fetched and never fetches; a
  GitLab merge done since the last fetch is only seen through the pushed check. Whether a
  release fetches first is WT-206's call. A git failure in a check counts as not passing, so
  doubt keeps the branch. Commits made in the same second with the same message, parent and
  tree are the same commit: tests that need two distinct commits change the message
  (`cherry-pick -x`). `MergeAddsNothing` needs `git merge-tree --write-tree` (git 2.38); an
  older git fails the check and falls through to `git cherry`.
- WT-301: prune runs `git worktree prune` before releasing (the criteria name it second): a release
  already prunes silently before it deletes a branch, so running git's prune last would report
  nothing. Its scope is `ls`'s (the current directory's repository, everything with `--all` or
  outside a repository), which adds `--all` and `--yes` to PLAN 4.6's `prune [--dry-run]`.
  `on_done = "keep"` does not apply: running the command is the request. No `--force`: the
  criteria say nothing dirty is ever removed, `tasq worktree release <id> --force` exists for
  that. A dirty worktree of a done task is warned about on every prune. git writes
  `prune --verbose` lines to stderr. Tests stripped task 1's real-path declaration as WT-207 does;
  with `--all` its project would otherwise be a repository to prune on the author's machine.
- WT-208 tasq_lab acceptance (2026-10-08, installed tasq bb1e820, the author's real notebook
  and config): throwaway task 51 declared `wt208-check` at `tasq_lab/wt208_check` (the
  `.tasq.toml` path, no git run); `tasq worktree up 51 --from main` created it from `main`
  without an upstream, copied `.envrc` byte for byte and ran the trusted `direnv allow .` (direnv
  reports it allowed); `ls` showed it clean; `pick --dry-run` starts there. `tasq done 51` from
  the main checkout removed the directory and deleted the branch ("same commit as main"), git
  lists no worktree, the `## Worktrees` entry stayed. Reopen + `up` recreated it the same way
  and a second `done` released it again. `--from main` because the repository has no
  `origin/HEAD` (the author left that as is). The author's `~/.zshrc` loads
  `tasq worktree shell-init zsh` next to gwm's line; with zsh's `EXTENDED_GLOB` a bare `^`
  globs, so `switch '^'` must be quoted (the author declined an alias).
- WT-208 silverfin acceptance (2026-10-08, same build and notebook): throwaway task 52
  declared `wt208-check` at `silverfin_worspace/wt208_check`; `tasq worktree up 52` (2.6 s)
  created it from `origin/HEAD` (`origin/main`) without an upstream, linked `node_modules`,
  `ignoreme`, `.nvim.lua` and `CLAUDE.local.md`, skipped `projects` with a warning (the main
  checkout has none), rendered `.envrc` byte for byte as the template with `{{original}}` =
  `silverfin` and `{{branch}}` = `wt208-check`, and ran the trusted `direnv allow`. `direnv exec`
  gave `PGDATABASE_*` = `silverfin_2db75ed4_*` (md5 of the branch), `BUNDLE_GEMFILE` in the
  worktree and only the worktree's `bin` on `PATH`. `ls` showed it clean (links and the rendered
  file do not count). `tasq done 52` from the workspace directory removed it and deleted the
  branch ("same commit as origin/main"); reopen + `up` + `done` repeated it identically. The
  per-branch databases are neither created nor dropped by tasq (gwm did not drop them either).
- WT-207: a fixture task (`id::FULL`, task 1) declares `/home/pau/code/tasks-wt/feature-a`, a
  real path on the author's machine. A first `switch` test picked it and `materialize` ran `git
  worktree add` there. Tests that list or create every task's worktrees now strip that
  declaration from the temp notebook (`worktree_nav::workspace`) and never use task 1. Any new
  test with worktrees enabled that touches task 1 must do the same. `git diff HEAD` leaves out
  untracked files, so `--in-diff` skipped WT-206's new `status.rs`; it was mutation-tested here.
  Use `git add -N` on new files before building the diff. A switch cannot `cd` its shell: the
  shell function sets `TASQ_CD_FILE` only for `... worktree switch ...` (so a session started by
  `tasq next` never inherits it) and `cd`s to what tasq wrote. `-` is the worktree root of the
  directory the last switch left, in `$XDG_STATE_HOME/tasq/previous-worktree`. The no-target
  list leaves out a task's missing worktrees when another of its worktrees exists: `materialize`
  only creates when none exists, so picking one would land in the existing one. `ls` without an
  id is scoped to the current directory's repository (everything outside one); `--all` is the
  WT-302 cross-project view. fish is not installed here: its function is checked as text, bash
  and zsh are run.
- WT-206: decisions taken without asking the author, all in `docs/config.md`: a release never
  fetches (the pushed check covers a branch merged on GitLab since the last fetch, and closing a
  task should not wait on the network); paths provisioning wrote do not make a worktree dirty
  (a copied `.envrc` or a `node_modules` link is untracked in most repositories and would
  otherwise keep every worktree forever), which is why `git worktree remove` always runs with
  `--force` once tasq decided (a locked worktree is still refused by git); a worktree the
  current directory is inside is kept (`/tasq:wrapup` runs `tasq done` from the session's
  worktree), with a hint to release it from elsewhere; a declared directory git does not list
  is kept; `--force` overrides only "dirty" and "neither merged nor pushed", never main, locked,
  in use or `delete_branch = "never"`. Kept branches warn only when unsafe: a branch kept
  because its worktree is kept is already explained, and `never` is the user's choice. A
  worktree without a branch runs its hooks with an empty `{{branch}}`. The TUI never prompts
  for hook approval (it owns the terminal): `Approval::Skip`, and every warning comes back for
  the status bar. `git branch -D`, not `-d`: git's own check refuses squash-merged branches.
- WT-204: a hook line is split into words first and each word rendered after, so a title with
  quotes or spaces cannot change the command; no shell, as for `[hooks]`. Only hook kinds whose
  key the repository's own `.tasq.toml` (`config::project_file` from the main checkout) decided
  need approval; `Loaded::file_for("worktree.hooks.<kind>")` tells. Approvals store each line
  verbatim rather than a hash (see Deviations). The prompt (`approve`) is the only
  `#[mutants::skip]`: it asks the terminal and calls `trust`, which is tested; the question text is
  a tested pure function. `--yes` is hidden on `next`/`pick` while worktrees are off, and pruned
  from their completions like `create --worktree`. `pre-remove`/`post-remove` run in WT-206.
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
  plan was rewritten around the declare / materialize / release lifecycle. Both commits were
  dropped from the branch in the 2026-10-12 rebase (PR #7 review S-5) and survive only on
  `backup/feat-worktrees-pre-drop`.
- 2026-10-08: WT-201's "TUI create form gets a worktree field" grew, at the author's request, into
  replacing the `c` title prompt with a full create view (the edit view, empty).
- 2026-10-08: WT-204 keys approvals by the file and the exact command line instead of "a hash of
  each command line" (PLAN 4.5): a non-cryptographic hash could be matched by a repository that
  wants an edited command to pass as approved, and comparing the text needs no hash at all.
- 2026-10-08: WT-202 dropped `LifecycleError::NoBranch`; `on_launch` returns a `LaunchAction`
  (no `Result`) and skips entries without a branch instead of failing (see Learnings).
- 2026-10-08: WT-208 drops `~/code/zantop_atelier` and leaves gwm installed (its `~/.zshrc` line,
  `~/.local/bin/gwm`, the `gwm.yml` files), both by the author's decision. tasq neither reads
  nor calls gwm; the two coexist (`gwm` and `tasq` shell functions do not clash).
- 2026-10-12: `worktree.copy_ignored` (WT-303) shipped in PR #7 as a parsed key nothing read.
  Rejected in config until WT-303 lands (fd5c8f8), with the same error shape as the removed
  `work.*` keys; WT-303 re-adds it.
