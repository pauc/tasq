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
3. Declare a worktree when creating a task (`tasq create --worktree [BRANCH]`), or let it be
   named automatically from the task (`{{id}}-{{slug}}` by default). Declaring writes the entry
   only; the worktree is created when the task is launched (4.2).
4. Configurable location: siblings of the project, subfolders of it, or a global tree, through a
   path template.
5. Launching a task (`next`, `pick`, the TUI's Enter) creates its declared worktree if it does
   not exist and starts the session in it. Closing the task deletes the worktree and its branch
   when that loses nothing; the entry stays, so launching it again recreates it the same way.
6. gwm parity: symlinks, copies, rendered templates, custom variables, `post_create` and
   `post_remove` hooks, `switch` with a shell `cd` wrapper and fzf, `list` with dirty state,
   `remove` refusing a dirty tree, completions. The author's three `gwm.yml` workspaces
   (`~/code/SF/silverfin_worspace`, `~/code/zantop_atelier`, `~/code/tasq_lab`) are the
   acceptance tests.
7. Improvements over gwm: merged-branch detection that understands squash and rebase merges,
   `prune`, cleanup when a task is closed, a status-rich `ls` joined with task data,
   trust for hooks that come from a repo, reflink copies of gitignored files, a stable port block
   per worktree.

## 3. Non-goals

- Any dependency on gwm. After this feature nothing in the repository runs, reads, imports or
  mentions gwm as a supported tool: no `command` worktree manager, no `gwm.yml` reader or
  importer, no test that needs gwm on `PATH`, no example config that uses it. The author's three
  `gwm.yml` files are converted to `.tasq.toml` by hand once (WT-208), outside the repository.
  The plan and PROGRESS of the rust rewrite keep their historical mentions.
- tmux/herdr window handling inside the worktree commands: the existing launchers own that
  (`tasq pick --detached`).
- Plugin-defined task fields (nb todo 46). Worktrees are core, not a plugin.
- Non-git VCS.

## 4. Architecture

### 4.1 Where the code goes

| Layer | Content | Rule |
|---|---|---|
| `tasq-core::worktree` (new, replaces `work.rs`) | `WorktreeConfig`, template rendering and variables, naming (`branch_safe`, `branch_db`, `slug`), path resolution, the **lifecycle decisions** (declare, materialize at launch, release at close; 4.2), the **provisioning plan** (`Vec<Step>`: `GitAdd`, `Symlink`, `Copy`, `Render`, `Hook`), merged-check decision logic over git outputs, prune selection, hook trust fingerprints | Pure. No I/O, no processes. Everything mutation-tested to zero missed. |
| `tasq-worktree` (new crate, replaces `tasq_launch::worktree`) | Executes a plan: runs `git`, creates links, copies (reflink when possible), writes rendered files, runs hooks; reads `git worktree list --porcelain -z`, `git status --porcelain=v2 --branch`, `git cherry`/`patch-id` for status and merged checks | The only new crate that runs `git`. Injected PATH/env like the launchers. `#[mutants::skip]` only on the pure I/O wrappers. |
| `tasq-cli::commands::worktree` | The command family, `create --worktree`, materializing in `next`/`pick`, releasing in `done`, shell-init and completions | Thin: plan in core, execute in `tasq-worktree`, render. |
| `tasq-tui` | Detail section and later actions, only when enabled | Through the CLI (ADR 0009). |

The crate split keeps the "core has no I/O" rule and makes the provisioning logic cheap to test:
a test asserts the plan for a config and a task, without touching git.

### 4.2 Lifecycle: declared, materialized, released

The `## Worktrees` entry is a **declaration**: the path and branch a task works in. The file
format does not change (``- /path (`branch`)``), and `tasq` never removes an entry; a worktree
directory comes and goes, the declaration stays.

1. **Declare.** `tasq create --worktree [BRANCH]`, `on_create = "auto"`, or later
   `tasq worktree <id> [BRANCH]` write the entry. The branch defaults to the `branch` template
   (`{{id}}-{{slug}}`); the path is rendered from the `path` template **now** and stored, so a
   later layout change never orphans an existing task. No git command runs; nothing is created
   on disk.
2. **Materialize at launch.** `next`/`pick` (and the TUI's Enter, which runs `pick`) look at the
   first declared worktree. If its directory exists, the session starts there, as today. If not,
   it is created at the stored path before the `pre-launch` hooks run, without asking (today's
   "tracked worktree is gone, recreate?" prompt goes away): the branch is reused when it exists
   locally, tracked from `origin/<branch>` when only the remote has it, else created from
   `base`; then the provisioning plan runs (symlinks, copies, templates, `post_create`). A
   failure aborts the launch with the error; the session never silently starts in the main
   checkout instead. `--dry-run` lists these steps.
3. **Release at close.** `tasq done` and the TUI's `d` (`on_done = "release"`, the default) go
   through every declared worktree that exists:
   - dirty (modified, staged or untracked files) or locked: kept, with a warning naming it;
   - otherwise: `pre_remove`, `git worktree remove`, `post_remove`;
   - then the local branch is deleted only when nothing is lost: its tip is integrated into
     the default branch (the checks of WT-205) or contained in its upstream on the remote.
     Otherwise the branch is kept and named in a warning.
   The entry stays in `## Worktrees`. The main worktree (the project checkout) is never removed.
4. **Rematerialize.** Launching the task again (after `tasq reopen`, or `pick` on any task whose
   worktree is gone) is step 2 again: a kept local branch is reused, a pushed one is tracked
   from the remote, and a branch that is gone everywhere (merged, remote deleted) starts fresh
   from `base` with a note saying so.

Consequences: the store needs no removal operation; `tasq worktree <id> <path>` (tracking a
worktree made outside tasq) follows the same lifecycle; `prune` only releases worktrees of done
tasks that a close left behind (dirty at the time, or closed outside tasq).

### 4.3 The toggle

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

### 4.4 Configuration

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
on_create = "auto"             # auto | never: declare one in `tasq create` without --worktree
on_done = "release"            # release | keep: in `tasq done` and the TUI's d key
delete_branch = "safe"         # safe | never: on release, delete the branch when nothing is lost

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

### 4.5 Hook trust

Hooks from `~/.config/tasq/config.toml` or `--config` run without asking. Hooks from a
`.tasq.toml` (which can come from a cloned repo) run only after approval: the first time, `tasq`
lists the commands and asks; the approval is stored in `~/.local/state/tasq/trust.toml` keyed by
the file path and a hash of each command line, so editing a command asks again. `--yes` approves
for one run, `tasq worktree trust [DIR]` approves ahead of time, non-interactive runs without
approval skip the hooks with a warning.

### 4.6 Commands

```
tasq create ... --worktree [BRANCH]           # declare at creation
tasq worktree <id> [BRANCH] [--path DIR]      # declare on an existing task (no git, nothing on disk)
tasq worktree <id> <EXISTING_DIR>             # declare a worktree made elsewhere (today's form)
tasq worktree up <id> [--from REF]            # materialize now, without launching
tasq worktree release <id> [--force]          # what a close does, without closing
tasq worktree ls [<id>] [--all] [--json]      # declared worktrees: exists or not, git state, task status
tasq worktree prune [--dry-run]               # release what closes left behind on done tasks
tasq worktree switch [TARGET]                 # TARGET: task:<id>, branch, path, ^ (main), - (previous); fzf without one
tasq worktree path <TARGET>                   # prints the path, for scripts
tasq worktree shell-init <bash|zsh|fish>      # wrapper that turns switch into a cd
tasq worktree trust [DIR]
```

`tasq worktree <id> <path>` keeps its meaning (an argument naming an existing directory is a
path, anything else a branch). `tasq worktree <id> --create <branch>` stays as a hidden alias of
"declare, then `up`" for one release, because the Claude prompt of existing sessions and the
`/tasq:wrapup` skill call it.

## 5. Tasks

### Phase 1: core foundations

#### WT-101: Lifecycle decisions in core
**Description:** The pure part of 4.2: given a task, the config and observed state (directory
exists, dirty, locked, branch local/remote/integrated), decide what to do. `declare` returns the
`Worktree` to store; `on_launch` returns `Reuse(path)` or `Materialize { path, branch, start }`
with `start` one of `Local`, `Track(remote)`, `New(base)`; `on_close` returns, per worktree,
`Remove { delete_branch: bool }` or `Keep(reason)`.

**Acceptance criteria:**
- [ ] A table test per decision covering every branch of 4.2 (dirty, locked, main worktree,
      branch integrated, pushed, neither; branch local, remote only, gone)
- [ ] Never `Remove` for a dirty or locked worktree or the main checkout; never
      `delete_branch` when the tip is neither integrated nor on its upstream
- [ ] Zero missed mutants in `core/src/worktree/lifecycle.rs`

#### WT-102: `[worktree]` config and the toggle
**Description:** `WorktreeConfig` with every key of 4.4, `TASQ_WORKTREE_*` env mapping,
validation (unknown template variables, `on_*` enums, port ranges), removal of
`work.worktree_manager`/`worktree_command` and the `CommandManager` with the migration error, and
removal of every gwm mention outside historical records.

**Acceptance criteria:**
- [ ] Defaults: `enabled = false`, the other keys as in 4.4
- [ ] Errors carry file:line:column like the rest of the config
- [ ] `docs/config.md` documents every key with its default; `config_load` tests cover each layer
- [ ] `grep -rni gwm` over `crates/`, `examples/`, `plugins/`, `README.md` and `docs/` finds
      nothing except ADRs and the one `docs/migration.md` row saying the original script called
      gwm and `[worktree]` replaces it; a CI check keeps it so

#### WT-103: Invisible when disabled
**Description:** Gate every surface listed in 4.3.

**Acceptance criteria:**
- [ ] CLI test: with the default config, `tasq --help`, `tasq create --help`, `tasq view --json`
      and `tasq list --json` contain no `worktree` (case-insensitive)
- [ ] TUI snapshot of the detail pane with and without the toggle
- [ ] Prompt snapshot with and without the toggle; `/tasq:wrapup` skill updated
- [ ] `resolve_workdir` ignores worktrees when disabled (unit test)
- [ ] `tasq apply` round trip of a document without `"worktrees"` keeps the stored entries

#### WT-104: Templates, variables and naming in core
**Description:** Move the `{{name}}` renderer to core; implement the variables of 4.4,
`branch_safe`, `branch_db`, `slug`, `port`.

**Acceptance criteria:**
- [ ] Property test: `branch_safe` is idempotent and only yields `[A-Za-z0-9_]`; a table of real
      branch names (`feature/x`, `fix--y`, `JIRA-12_z`) pins the output so existing worktree
      directory names keep matching
- [ ] Unknown placeholder is an error naming the variable and the template
- [ ] Zero missed mutants in `core/src/worktree/*`

### Phase 2: the lifecycle, at parity with today's workflow

#### WT-201: Declaring: `tasq create --worktree` and `tasq worktree <id> [BRANCH]`
**Description:** Step 1 of 4.2. `--worktree [BRANCH]`, `on_create = "auto"`, the `branch` and
`path` templates rendered at declaration time and stored.

**Acceptance criteria:**
- [ ] `tasq create "Fix login" --worktree` writes ``- <rendered path> (`<id>-fix-login`)`` and
      runs no process (test with an empty `PATH`)
- [ ] `on_create = "auto"` declares one for every task whose project is inside a git work tree
      (detected on the filesystem, no `git` process); `never` declares only with `--worktree`
- [ ] `tasq worktree <id> <existing dir>` keeps today's behaviour; any other argument is a branch
- [ ] TUI create form gets a worktree field when enabled

#### WT-202: Materializing at launch: the `tasq-worktree` crate
**Description:** Step 2 of 4.2. New crate executing the `on_launch` decision: branch start
(local, `origin/<branch>` tracking, new from `base`/`--from`), `git worktree add` at the stored
path. Wired into `next`/`pick` before `pre-launch`, replacing the recreate prompt, and into
`tasq worktree up`. Replaces `tasq_launch::worktree`.

**Acceptance criteria:**
- [ ] Temp repos with a bare "origin" cover the three branch starts
- [ ] A launch of a task whose worktree does not exist creates it and starts the session there;
      `--dry-run` lists the steps and creates nothing
- [ ] A failure aborts the launch with the git error; nothing is left half-created at the path

#### WT-203: Provisioning: symlinks, copies, templates
**Acceptance criteria:**
- [ ] Fixture configs modelled on the author's three workspaces (a sibling layout with symlinks
      and an `.envrc` template, a `{{project_name}}-` layout with per-branch databases, a copy of a
      gitignored `.envrc`) each produce a recorded expected plan (unit tests in core, no git)
- [ ] Executing a plan in a temp repo produces the expected tree (links, copied files, rendered
      `.envrc` bytes), checked against fixtures, never against another tool's output
- [ ] Missing symlink/copy sources are warnings; a missing template source is an error

#### WT-204: Hooks and trust
**Acceptance criteria:**
- [ ] `pre_*` failure aborts the step (the launch, or the removal); `post_*` failure is a warning
- [ ] Hooks run in the worktree with `TASQ_WORKTREE`, `TASQ_BRANCH`, `TASQ_TASK_ID` and the
      variables as `TASQ_VAR_<NAME>`
- [ ] Trust store behaviour of 4.5, including re-asking after a command changes

#### WT-205: Integration checks
**Description:** What "nothing is lost" means for a branch. Integrated into the default branch
when any check passes, cheapest first: same commit, ancestor, empty three-dot diff, same tree,
merge would add nothing, every commit's patch-id present upstream (squash and rebase merges).
Or contained in its upstream on the remote (pushed).

**Acceptance criteria:**
- [ ] One temp-repo test per check, a pushed-but-unmerged case, and a neither case

#### WT-206: Releasing at close
**Description:** Step 3 of 4.2 in `tasq done`, the TUI's `d` and `tasq worktree release`;
`on_done`, `delete_branch`.

**Acceptance criteria:**
- [ ] Closing a task with a clean, merged worktree removes the directory and the local branch;
      the `## Worktrees` entry is byte for byte unchanged
- [ ] Dirty, locked, unpushed-and-unmerged cases keep what they must and print one warning per
      kept item naming it and the command to finish (`tasq worktree release <id> --force`)
- [ ] The task is closed even when a removal fails (as with `post-done` hooks today)
- [ ] Reopen and launch rematerializes: kept branch reused, pushed branch tracked, gone branch
      fresh from `base` with a note

#### WT-207: `ls`, `switch`, `path`, `shell-init`, completions
**Acceptance criteria:**
- [ ] `ls` shows every declared worktree, whether it exists, and dirty state; `--json`
      documented in `docs/json.md`
- [ ] `shell-init` output tested for bash, zsh and fish; `switch` without a target uses fzf when
      present, a numbered list otherwise; `switch task:<id>` materializes first if needed

#### WT-208: Switch the author's workspaces off gwm (manual acceptance)
**Description:** Not shipped code. Write a `.tasq.toml` `[worktree]` table for each of
`~/code/SF/silverfin_worspace`, `~/code/zantop_atelier` and `~/code/tasq_lab` equivalent to its
`gwm.yml`, set `worktree.enabled = true` in the author's config, and stop calling gwm.
**Acceptance criteria:**
- [ ] In each workspace, a task declared, launched, closed and launched again goes through the
      whole lifecycle with the same files, links, `.envrc` and hook effects as gwm gave; recorded
      in PROGRESS.md
- [ ] No tasq command, config, skill or prompt the author uses calls gwm; uninstalling it
      (`~/.local/bin/gwm`, the shell-init line) breaks nothing
- [ ] `gwm.yml` files are left for the author to delete; tasq never reads them

### Phase 3: improvements

#### WT-301: `prune`
**Acceptance criteria:**
- [ ] Releases the existing worktrees of done tasks (kept at close, or closed outside tasq) with
      the same rules as WT-206, and runs `git worktree prune`
- [ ] `--dry-run` lists the plan; nothing dirty is ever removed

#### WT-302: Status-rich `ls`
**Acceptance criteria:**
- [ ] Columns: task id, title, status, branch, dirty, ahead/behind upstream, integrated, age
- [ ] `--all` across every project the notebook's tasks track
- [ ] MR state through the configured `[forge.*]`/sources when available

#### WT-303: Fast copies of ignored files
**Acceptance criteria:**
- [ ] `copy_ignored` copies gitignored files from the main worktree, narrowed by
      `.worktreeinclude` (gitignore syntax), skipping VCS and tool-state directories
- [ ] Reflink (`FICLONE`) when the filesystem supports it, plain copy otherwise; measured on a
      real `node_modules` and recorded

#### WT-304: Port blocks and database names
**Acceptance criteria:**
- [ ] `{{port}}` stable per branch and project, collision-checked against other tracked worktrees
- [ ] The zantop workspace's `post_remove` `dropdb` hooks expressed with `{{branch_db}}`

### Later (not scheduled)

Carry uncommitted changes into a new worktree (git-wt); copy Claude Code session data into a new
worktree (ccmanager); `tasq worktree run <cmd>` across worktrees; `git worktree lock` while a
session runs; TUI worktree actions (create, switch, remove) through the CLI.

## 6. Delivery order and rationale

WT-101 first: the lifecycle decisions are the heart of the feature and pure, so they are pinned
down before any git code exists. WT-102/103 next so the toggle shapes every later task instead of
being retrofitted. Phase 2 follows the lifecycle in order (declare, materialize, provision,
release) and reaches parity, at which point the author uninstalls gwm (WT-208 is the real
acceptance). Phase 3 improvements are independent and can land
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
- Settled 2026-10-07: no removal of `## Worktrees` entries ever; worktrees are declared at
  creation, created at launch, released at close (4.2). Defaults confirmed: `on_create = "auto"`,
  `on_done = "release"`, `delete_branch = "safe"`.
