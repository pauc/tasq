# Plan: worktrees as an opt-in core feature

Task: nb todo 44 ("Create a plugin to manage worktrees associated to tasks"). Decision record:
[ADR 0022](../../../docs/adr/0022-worktrees-opt-in-core-feature.md). Progress:
[PROGRESS.md](PROGRESS.md). Started 2026-10-07.

## 1. Introduction

`tasq` already tracks worktrees on a task (`## Worktrees`), starts sessions in the first one that
exists, and creates one with `tasq worktree <id> --create <branch>` through a `WorktreeManager`
(`git worktree add`, or a user command such as `gwm create ...`). Provisioning a worktree (linked
files, rendered `.envrc`, `direnv allow`, per-branch databases) is delegated to `gwm`, a 1.9k-line
zsh script driven by a `gwm.yml` per workspace.

This feature makes worktrees a first-class, **opt-in** part of `tasq` that fully replaces `gwm`
and improves on it, borrowing from the tools surveyed in section 7. A user who does not enable it
sees nothing about worktrees anywhere: not in `--help`, not in JSON, not in the TUI, not in the
Claude prompt, not in the files.

## 2. Goals

1. One toggle, `worktree.enabled` (default `false`). Off means invisible; on means the whole
   feature below.
2. Everything worktree-related lives behind it: the `tasq worktree` command family, the config,
   the `## Worktrees` section's display and use in `next`/`pick`.
3. Name a worktree when creating a task (`tasq create --worktree [BRANCH]`), or let it be named
   automatically from the task (`{id}-{slug}` by default).
4. Configurable location: siblings of the project, subfolders of it, or a global tree, through a
   path template.
5. Sessions for a task with a worktree start in it (already true; kept, and extended to recreate
   a missing one through the new provisioning).
6. gwm parity: symlinks, copies, rendered templates, custom variables, `post_create` and
   `post_remove` hooks, `switch` with a shell `cd` wrapper and fzf, `list` with dirty state,
   `remove` refusing a dirty tree, completions. The author's three `gwm.yml` workspaces
   (`~/code/SF/silverfin_worspace`, `~/code/zantop_atelier`, `~/code/tasq_lab`) are the
   acceptance tests.
7. Improvements over gwm: merged-branch detection that understands squash and rebase merges,
   `prune`, cleanup offered when a task is closed, a status-rich `ls` joined with task data,
   trust for hooks that come from a repo, reflink copies of gitignored files, a stable port block
   per worktree.

## 3. Non-goals

- Any dependency on gwm. After this feature nothing in the repository runs, reads, imports or
  mentions gwm as a supported tool: no `command` worktree manager, no `gwm.yml` reader or
  importer, no test that needs gwm on `PATH`, no example config that uses it. The author's three
  `gwm.yml` files are converted to `.tasq.toml` by hand once (WT-206), outside the repository.
  The plan and PROGRESS of the rust rewrite keep their historical mentions.
- tmux/herdr window handling inside the worktree commands: the existing launchers own that
  (`tasq pick --detached`).
- Plugin-defined task fields (nb todo 46). Worktrees are core, not a plugin.
- Non-git VCS.

## 4. Architecture

### 4.1 Where the code goes

| Layer | Content | Rule |
|---|---|---|
| `tasq-core::worktree` (new, replaces `work.rs`) | `WorktreeConfig`, template rendering and variables, naming (`branch_safe`, `branch_db`, `slug`), path resolution, the **provisioning plan** (`Vec<Step>`: `GitAdd`, `Symlink`, `Copy`, `Render`, `Hook`), merged-check decision logic over git outputs, prune selection, hook trust fingerprints | Pure. No I/O, no processes. Everything mutation-tested to zero missed. |
| `tasq-core::format::ops` | `remove_worktree` | Lossless, fixture round trips. |
| `tasq-store-nb::diff` | worktree removals | `Store::update` with fewer worktrees removes the lines; removing the last one removes the section. |
| `tasq-worktree` (new crate, replaces `tasq_launch::worktree`) | Executes a plan: runs `git`, creates links, copies (reflink when possible), writes rendered files, runs hooks; reads `git worktree list --porcelain -z`, `git status --porcelain=v2 --branch`, `git cherry`/`patch-id` for status and merged checks | The only new crate that runs `git`. Injected PATH/env like the launchers. `#[mutants::skip]` only on the pure I/O wrappers. |
| `tasq-cli::commands::worktree` | The command family, `create --worktree`, `done` cleanup prompt, shell-init and completions | Thin: plan in core, execute in `tasq-worktree`, render. |
| `tasq-tui` | Detail section and later actions, only when enabled | Through the CLI (ADR 0009). |

The crate split keeps the "core has no I/O" rule and makes the provisioning logic cheap to test:
a test asserts the plan for a config and a task, without touching git.

### 4.2 The toggle

With `worktree.enabled = false` (default):

- `tasq worktree ...` is hidden from `--help` and exits 1 with
  `worktrees are disabled; set worktree.enabled = true (docs/config.md#worktrees)`.
- `tasq create --worktree` is hidden and rejected the same way.
- `--json` task documents leave out `"worktrees"` (`docs/json.md` documents the key as present
  only when enabled; `tasq apply` accepts documents with or without it and keeps the stored
  worktrees when the key is absent).
- The TUI detail pane never shows a Worktrees heading.
- The default Claude prompt drops its worktree line (`{{#worktrees_enabled}}...{{/worktrees_enabled}}`);
  the `/tasq:wrapup` skill skips worktrees when `tasq config show --json` says they are off.
- `resolve_workdir` ignores tracked worktrees: sessions start in the project.
- Files are untouched: a `## Worktrees` section written while enabled stays byte for byte, it is
  simply not used. Re-enabling brings it back.

`[worktree]` keys other than `enabled` are still validated when the feature is off, so turning it
on never surfaces an old typo.

### 4.3 Configuration

Two kinds of settings, in the existing layered config (no second config system):

- **Personal defaults** in `~/.config/tasq/config.toml`: `enabled`, layout, naming, cleanup policy.
- **Per-repo provisioning** (what `gwm.yml` holds today) in a `.tasq.toml` next to the repo or in
  the workspace directory above it. The worktree commands load config **from the project
  directory**, not from the current directory, so the right `.tasq.toml` applies even when the
  command runs elsewhere. Relative `source` paths resolve against the file that declared them.

```toml
[worktree]
enabled = true
path = "{{project_parent}}/{{project_name}}-{{branch_safe}}"   # gwm default layout
# path = "{{project}}/.worktrees/{{branch_safe}}"              # subfolders (add to .git/info/exclude)
# path = "~/worktrees/{{repo}}/{{branch_safe}}"                # a global tree
branch = "{{id}}-{{slug}}"     # branch name when none is given
base = "origin/HEAD"           # start point for a new branch (--from overrides)
on_create = "never"            # never | ask | always: make one in `tasq create`
on_done = "ask"                # keep | ask | remove: in `tasq done` and the TUI's d key

symlinks = ["node_modules", "CLAUDE.local.md"]
copies = [".envrc"]
copy_ignored = false           # reflink-copy gitignored files, narrowed by .worktreeinclude
templates = [{ source = "templates/.envrc.template", target = ".envrc" }]

[worktree.variables]
db_prefix = "zantop"

[worktree.ports]               # {{port}}: base + hash(branch) mod span, rounded to block
base = 4000
span = 4000
block = 10

[worktree.hooks]
pre_create = []
post_create = ["direnv allow ."]
pre_remove = []
post_remove = []
```

Template variables: `id`, `title`, `slug`, `branch`, `branch_safe` (non-alphanumerics to `_`, runs collapsed, trimmed), `branch_db`
(lowercase, `[a-z0-9_]`, 63 bytes), `project`, `project_name`, `project_parent`, `repo` (from
`remote.origin.url`, else `project_name`), `worktree`, `worktree_name`, `port`, and every
`[worktree.variables]` entry. The renderer is the `{{name}}` one already used for the Claude
prompt, moved from `tasq-launch` to core; an unknown placeholder is an error, never left in the
output (gwm leaves it, which silently produces broken `.envrc` files).

`work.worktree_manager` and `work.worktree_command` are removed: the `command` manager existed to
call gwm. A config that still sets them fails with a message pointing to `[worktree]` in
`docs/config.md`. Every gwm mention in code, tests, examples and current docs goes with them
(WT-102).

### 4.4 Hook trust

Hooks from `~/.config/tasq/config.toml` or `--config` run without asking. Hooks from a
`.tasq.toml` (which can come from a cloned repo) run only after approval: the first time, `tasq`
lists the commands and asks; the approval is stored in `~/.local/state/tasq/trust.toml` keyed by
the file path and a hash of each command line, so editing a command asks again. `--yes` approves
for one run, `tasq worktree trust [DIR]` approves ahead of time, non-interactive runs without
approval skip the hooks with a warning.

### 4.5 Commands

```
tasq worktree create <id> [BRANCH] [--from REF] [--path DIR] [--no-hooks] [--switch]
tasq worktree add <id> <PATH>                 # track an existing worktree (today's form)
tasq worktree ls [<id>] [--all] [--json]      # worktrees joined with task id, title, status
tasq worktree rm <id|path> [--force] [--with-branch] [--keep-tracking]
tasq worktree prune [--gone] [--merged] [--remote-deleted] [--min-age 1d] [--dry-run]
tasq worktree switch [TARGET]                 # TARGET: task:<id>, branch, path, ^ (main), - (previous); fzf without one
tasq worktree path <TARGET>                   # prints the path, for scripts
tasq worktree shell-init <bash|zsh|fish>      # wrapper that turns switch into a cd
tasq worktree trust [DIR]
tasq create ... --worktree [BRANCH]
```

The current forms `tasq worktree <id> <path>` and `tasq worktree <id> --create <branch>` stay as
hidden aliases of `add` and `create` for one release, because the Claude prompt of existing
sessions and the `/tasq:wrapup` skill call them.

## 5. Tasks

### Phase 1: core foundations

#### WT-101: The store can stop tracking a worktree
**Description:** `format::ops::remove_worktree` and removal support in `store-nb/src/diff.rs`.

**Acceptance criteria:**
- [ ] Removing one of several entries removes exactly its line; removing the last removes the
      `## Worktrees` section and the blank line before it; fixtures in
      `crates/core/tests/fixtures/ops/worktree_remove_*.{before,after}.md`
- [ ] `Store::update` with a task whose `worktrees` lost an entry writes the file, and
      `Task::remove_worktree(path) -> bool` exists
- [ ] Zero missed mutants in `format/ops.rs` and `store-nb/src/diff.rs`

#### WT-102: `[worktree]` config and the toggle
**Description:** `WorktreeConfig` with every key of 4.3, `TASQ_WORKTREE_*` env mapping,
validation (unknown template variables, `on_*` enums, port ranges), removal of
`work.worktree_manager`/`worktree_command` and the `CommandManager` with the migration error, and
removal of every gwm mention outside historical records.

**Acceptance criteria:**
- [ ] Defaults: `enabled = false`, layout and branch templates as in 4.3
- [ ] Errors carry file:line:column like the rest of the config
- [ ] `docs/config.md` documents every key with its default; `config_load` tests cover each layer
- [ ] `grep -rni gwm` over `crates/`, `examples/`, `plugins/`, `README.md` and `docs/` finds
      nothing except ADRs and the one `docs/migration.md` row saying the original script called
      gwm and `[worktree]` replaces it; a CI check keeps it so

#### WT-103: Invisible when disabled
**Description:** Gate every surface listed in 4.2.

**Acceptance criteria:**
- [ ] CLI test: with the default config, `tasq --help`, `tasq create --help`, `tasq view --json`
      and `tasq list --json` contain no `worktree` (case-insensitive)
- [ ] TUI snapshot of the detail pane with and without the toggle
- [ ] Prompt snapshot with and without the toggle; `/tasq:wrapup` skill updated
- [ ] `resolve_workdir` ignores worktrees when disabled (unit test)
- [ ] `tasq apply` round trip of a document without `"worktrees"` keeps the stored entries

#### WT-104: Templates, variables and naming in core
**Description:** Move the `{{name}}` renderer to core; implement the variables of 4.3,
`branch_safe`, `branch_db`, `slug`, `port`.

**Acceptance criteria:**
- [ ] Property test: `branch_safe` is idempotent and only yields `[A-Za-z0-9_]`; a table of real
      branch names (`feature/x`, `fix--y`, `JIRA-12_z`) pins the output so existing worktree
      directory names keep matching
- [ ] Unknown placeholder is an error naming the variable and the template
- [ ] Zero missed mutants in `core/src/worktree/*`

### Phase 2: parity with today's workflow

#### WT-201: `tasq-worktree` crate and `create`
**Description:** New crate; branch resolution: local branch, else `origin/<branch>` with
tracking, else new from `base`/`--from`, path from the template, `git worktree add`, tracking on
the task. Replaces `tasq_launch::worktree`.

**Acceptance criteria:**
- [ ] Tests against temporary repos with a bare "origin" for all three branch cases
- [ ] Refuses an existing target path; a failed `git worktree add` leaves nothing tracked
- [ ] `next`/`pick` recreate a missing worktree through the same code

#### WT-202: Provisioning: symlinks, copies, templates
**Acceptance criteria:**
- [ ] Fixture configs modelled on the author's three workspaces (a sibling layout with symlinks
      and an `.envrc` template, a `{{project_name}}-` layout with per-branch databases, a copy of a
      gitignored `.envrc`) each produce a recorded expected plan (unit tests in core, no git)
- [ ] Executing a plan in a temp repo produces the expected tree (links, copied files, rendered
      `.envrc` bytes), checked against fixtures, never against another tool's output
- [ ] Missing symlink/copy sources are warnings; a missing template source is an error

#### WT-203: Hooks and trust
**Acceptance criteria:**
- [ ] `pre_*` failure aborts and leaves nothing tracked; `post_*` failure is a warning
- [ ] Hooks run in the worktree with `TASQ_WORKTREE`, `TASQ_BRANCH`, `TASQ_TASK_ID` and the
      variables as `TASQ_VAR_<NAME>`
- [ ] Trust store behaviour of 4.4, including re-asking after a command changes

#### WT-204: `add`, `ls`, `rm`, `switch`, `path`, `shell-init`, completions
**Acceptance criteria:**
- [ ] `rm` refuses a dirty worktree without `--force`, runs `pre_remove`/`post_remove`, untracks
- [ ] `ls --json` documented in `docs/json.md`
- [ ] `shell-init` output tested for bash, zsh and fish; `switch` without a target uses fzf when
      present, a numbered list otherwise

#### WT-205: Worktrees from `tasq create`
**Description:** `--worktree [BRANCH]`, `on_create`, automatic naming.

**Acceptance criteria:**
- [ ] `tasq create "Fix login" --worktree` on a git project creates `<id>-fix-login` and tracks it
- [ ] A failure to create the worktree after the task is written is a warning with the manual
      command, never a lost task
- [ ] TUI create form gets a worktree field when enabled

#### WT-206: Switch the author's workspaces off gwm (manual acceptance)
**Description:** Not shipped code. Write a `.tasq.toml` `[worktree]` table for each of
`~/code/SF/silverfin_worspace`, `~/code/zantop_atelier` and `~/code/tasq_lab` equivalent to its
`gwm.yml`, set `worktree.enabled = true` in the author's config, and stop calling gwm.
**Acceptance criteria:**
- [ ] Each workspace creates, lists, switches to and removes a worktree through `tasq` with the
      same files, links, `.envrc` and hook effects as before; recorded in PROGRESS.md
- [ ] No tasq command, config, skill or prompt the author uses calls gwm; uninstalling it
      (`~/.local/bin/gwm`, the shell-init line) breaks nothing
- [ ] `gwm.yml` files are left for the author to delete; tasq never reads them

### Phase 3: improvements

#### WT-301: Merged detection and safe removal
**Description:** A branch counts as integrated into the default branch when any check passes,
cheapest first: same commit, ancestor, empty three-dot diff, same tree, merge would add nothing,
every commit's patch-id present upstream (squash and rebase merges).
**Acceptance criteria:**
- [ ] One temp-repo test per check, plus a not-merged case
- [ ] `rm --with-branch` deletes the branch only when integrated, unless `--force`

#### WT-302: `prune`
**Acceptance criteria:**
- [ ] `--gone` untracks entries whose directory is gone (and runs `git worktree prune`)
- [ ] `--merged` and `--remote-deleted` remove clean, integrated worktrees older than `--min-age`
- [ ] `--dry-run` lists the plan; nothing dirty is ever removed

#### WT-303: Cleanup when a task closes
**Description:** `on_done` in `tasq done` and the TUI's `d`.
**Acceptance criteria:**
- [ ] `ask` lists the task's worktrees with their state and removes the chosen clean ones
- [ ] `remove` only removes clean, integrated worktrees and reports the rest

#### WT-304: Status-rich `ls`
**Acceptance criteria:**
- [ ] Columns: task id, title, status, branch, dirty, ahead/behind upstream, integrated, age
- [ ] `--all` across every project the notebook's tasks track
- [ ] MR state through the configured `[forge.*]`/sources when available

#### WT-305: Fast copies of ignored files
**Acceptance criteria:**
- [ ] `copy_ignored` copies gitignored files from the main worktree, narrowed by
      `.worktreeinclude` (gitignore syntax), skipping VCS and tool-state directories
- [ ] Reflink (`FICLONE`) when the filesystem supports it, plain copy otherwise; measured on a
      real `node_modules` and recorded

#### WT-306: Port blocks and database names
**Acceptance criteria:**
- [ ] `{{port}}` stable per branch and project, collision-checked against other tracked worktrees
- [ ] The zantop workspace's `post_remove` `dropdb` hooks expressed with `{{branch_db}}`

### Later (not scheduled)

Carry uncommitted changes into a new worktree (git-wt); copy Claude Code session data into a new
worktree (ccmanager); `tasq worktree run <cmd>` across worktrees; `git worktree lock` while a
session runs; TUI worktree actions (create, switch, remove) through the CLI.

## 6. Delivery order and rationale

WT-101 first: without removal, nothing else can clean up. WT-102/103 next so the toggle shapes
every later task instead of being retrofitted. Phase 2 reaches parity, at which point the author
uninstalls gwm (WT-206 is the real acceptance). Phase 3 improvements are independent and can land
in any order.

## 7. Prior art

Surveyed 2026-10-07. Ideas taken, by source:

| Tool | Taken |
|---|---|
| gwm (author's, being retired) | the provisioning model (symlinks, copies, templates, variables, hooks), the shell `cd` wrapper, dirty-safe remove. Ideas only: no code, config format or compatibility is kept |
| [Worktrunk](https://github.com/max-sixty/worktrunk) | path template with sanitised names, pre/post hook pairs, hook trust re-asked on change, layered merged detection, `prune` with min age, reflink copy of ignored files with `.worktreeinclude`, `hash_port`, `^`/`-` targets |
| [gtr](https://github.com/coderabbitai/git-worktree-runner) | trust for repo-provided hooks, cleanup by MR state via `glab`/`gh` |
| [wtp](https://github.com/satococoa/wtp) | `--with-branch` removal |
| [gwq](https://github.com/d-kuro/gwq) | global layout, cross-repo status listing |
| [treekanga](https://github.com/garrettkrohn/treekanga) | prune branches deleted on the remote |
| [Conductor](https://docs.conductor.build/core/scripts) | per-workspace port block, archive hook for external resources |
| [phantom](https://github.com/aku11i/phantom), [git-wt](https://github.com/k1LoW/git-wt), [ccmanager](https://github.com/kbwo/ccmanager) | later items: carry WIP, copy agent session data |

What none of them has, and `tasq` does: the worktree belongs to a task, so names come from the
task, `ls` shows task status next to git state, closing the task offers cleanup, and launching
the task lands in the worktree.

## 8. Open questions

- Default `path` template: gwm's sibling layout (`{{project_parent}}/{{project_name}}-{{branch_safe}}`)
  or subfolders? Sibling is proposed because it needs no `.git/info/exclude` entry.
- Is a `.tasq.toml` in the workspace parent (gwm's location) enough, or do we also want a
  committed in-repo file for teams? The layered loader already supports both.
- `on_done = "ask"` default when enabled, or `keep`?
