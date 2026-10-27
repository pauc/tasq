# Plan: store sync as the `tasq-git` plugin

Task: tasq 57 ("Git sync as an opt-in feature for tasq stores"). Blocked by tasq 59 (plugin
extension points, plan in [`../plugin-events/PLAN.md`](../plugin-events/PLAN.md)). Decision
record: [ADR 0030](../../../docs/adr/0030-store-sync-as-a-plugin.md), building on
[ADR 0026](../../../docs/adr/0026-store-v2-frontmatter-tasq-directory-ulid-files.md). Terms:
[CONTEXT.md](../../../CONTEXT.md) (Store sync, Sync, Plugin, Event, Store, Id). Progress:
[PROGRESS.md](PROGRESS.md). Planned 2026-10-19.

## Problem Statement

Since task 56 a store is a directory of task files with no git: no history, no backup, and no
way to see the same tasks on a second machine. The nb-era store committed after every write and
pulled and pushed on an interval; that went with nb. The user wants it back, opt-in, without any
command or the terminal UI ever waiting for git, and with the stores committing and pushing as a
personal identity and SSH key even when a command runs inside an employer project whose
environment carries another.

Two machines that both create tasks offline allocate the same next id. Today the store detects
it (`list` warns, `doctor` fails, `get` refuses the id) but does not help the user out of it.

## Solution

`tasq-git`, a plugin built in this workspace (`crates/git`), turned on with
`[plugins] enabled = ["git"]`.

- **Setup.** `tasq git init [<url>] [--branch <b>] [--name --email --ssh-key]` makes the current
  store a repository, commits its task files, and, given a URL, pushes them to a branch named
  after the store unless `--branch` says otherwise. `tasq git clone <url> [--branch <b>]
  [--store <name>] [identity flags]` creates a store from a remote branch. Several stores can
  share one remote repository, one branch each. The remote, branch and identity are written to
  the store repository's own git config; `[plugin.git]` holds defaults for the flags.
- **Background.** After every command that wrote, a detached job commits the task files and
  pushes. When a command or the TUI opens the store and the last fetch is older than the
  interval (60 s), the job fetches and rebases. Only the rebase holds the store lock. One job
  per store; writes that land while it runs are picked up before it exits.
- **Foreground.** `tasq git sync` runs the same steps in the terminal and prints what happened;
  it can prompt for a passphrase.
- **Problems** (a conflict, a rejected push, an unreachable remote, a duplicate id pulled in)
  become lasting warnings shown on every command until fixed, and `tasq doctor` explains the
  repository's state.
- **A store that is not a repository** behaves exactly as without the plugin.

## User Stories

1. As a user, I want to turn store sync on with `[plugins] enabled = ["git"]`, so that it is one
   line in my global config.
2. As a user, I want a store that is not a git repository to behave exactly as before with the
   plugin enabled, so that enabling it globally is safe for stores I keep local.
3. As a user, I want `tasq git init` to make my existing store a repository with one commit of
   its task files, so that its history starts from what I have.
4. As a user, I want `tasq git init <url>` to push my store to a branch named after the store,
   so that several stores share one remote repository without me naming branches.
5. As a user, I want `tasq git init <url>` to refuse when that branch already has history, and
   tell me to clone or pick another `--branch`, so that two unrelated task histories never merge.
6. As a user, I want `tasq git clone <url> [--branch <b>] [--store <name>]` to create a store on
   a second machine from a remote branch, refusing if that store already has task files.
7. As a user, I want `--name`, `--email` and `--ssh-key` on init and clone, defaulting from
   `[plugin.git]`, written into the store repository's git config, so that my stores commit and
   push as me-personal whatever project my shell is in.
8. As a user, I want the store's key used even when my SSH agent holds my work key, so that the
   wrong key is never offered first.
9. As a user, I want `git` run by hand in the store directory to use the same identity, so that
   a manual fix is committed as the store's identity too.
10. As a user, I want every command that wrote to be committed in the background, so that my
    history is complete and no command waits for git.
11. As a user, I want a commit subject like `log 57: Git sync as an opt-in feature for tasq
    stores`, and one body line per write when several are batched, so that `git log` reads as
    my activity.
12. As a user, I want only task files committed, never `.tasq/` or anything else I put in the
    store directory, so that locks and plugin state stay local.
13. As a user, I want the background job to push after committing when the store has a remote,
    so that my other machines see the change soon.
14. As a user, I want a rejected push to fetch, rebase and push again, so that two machines
    writing in turn need no manual step.
15. As a user, I want any command, a read included, to fetch and rebase in the background once
    the last fetch is older than the interval, so that a machine I only read on stays current.
16. As a user, I want the interval set as `[plugin.git] interval` (seconds, default 60).
17. As a user, I want the TUI to show pulled changes without a keypress, which the TUI reload of
    task 59 gives.
18. As a user, I want a background job that would prompt (passphrase, host key, credentials) to
    fail with a lasting warning instead of hanging.
19. As a user, I want my shell's `GIT_*` variables ignored by the background job, so that a
    direnv environment cannot redirect it to another repository or identity.
20. As a user, I want `tasq git sync` to commit, fetch, rebase and push in the foreground and
    print each step's outcome, prompting if it must, so that I can sync on demand and see why it
    fails.
21. As a user, I want a rebase conflict to be aborted, my local commits kept, and a lasting
    warning naming the conflicting task ids and the git commands to resolve it, so that nothing is
    lost and no task file ever contains conflict markers.
22. As a user, I want a failed push or fetch (offline, remote gone) to be a lasting warning that
    clears itself on the next success, so that I am told once per command until it works again.
23. As a user, I want `tasq doctor` to show, per store: store sync off (not a repository), the
    identity it commits as, the remote and branch, commits not yet pushed, the last fetch time,
    and a rebase left in progress, so that a stuck sync explains itself.
24. As a user, I want `doctor`'s duplicate-id failure to name the newer file by creation time,
    both titles, and the next free id to write into the newer file's `id:`, so that I can fix it
    in one edit.
25. As a user, I want a pull that brings in a duplicate id to leave a lasting warning, so that I
    learn of it on the next command rather than when `get` refuses the id.
26. As a user, I want duplicate ids never renumbered automatically, so that worktree branches,
    merge request links and notes that carry the id keep pointing at the right task.
27. As a user, I want writes that land while a job runs to be committed by that job before it
    exits, so that the last write is never left uncommitted.
28. As a user, I want a job that died to stop blocking after two minutes, as the store lock does.
29. As a maintainer, I want `tasq-git` to depend on no tasq crate, only on the event payloads,
    `tasq ... --json` and `tasq store lock`, so that it can move to its own repository unchanged.
30. As a maintainer, I want the job's git steps to run against a temp repository and a bare
    remote in tests, with injected `PATH` and `HOME`, so that the behaviour is pinned without a
    network or my own config.
31. As a maintainer, I want `scripts/install`, CI and the release workflow to build and test
    `tasq-git`, so that it ships with `tasq`.
32. As a reader of the docs, I want a `docs/git-sync.md` covering setup on the first and second
    machine, identity, the interval, conflicts and duplicate ids.

## Implementation Decisions

- **Crate.** `crates/git`, package `tasq-git`, one binary `tasq-git` and a library holding the
  logic. Dependencies: serde, serde_json, toml-free (config arrives as JSON), chrono, clap; no
  tasq crate. Subcommands: `hook` (events), `init`, `clone`, `sync`, and the internal `job
  --store <dir>` the hook detaches. Every `git` call goes through one `Git` runner with an
  injected environment, as `tasq-worktree` does.
- **State.** `<store>/.tasq/plugins/git/`: `job.lock` (one job per store; created exclusively,
  stale after 120 s), `pending` (touched by a hook that finds the job running; the job loops
  while it exists), `writes` (one line per `post-write` not yet committed: command and ids, for
  the commit message), `last-fetch` (Unix seconds), `warnings` (the lasting warnings of task 59).
- **Hook.** `store-open` and `post-write` read the payload, return at once if the store is not a
  repository (`<store>/.git` absent), append the writes to `writes` for `post-write`, and spawn
  `tasq-git job` detached (`setsid`, stdio to `/dev/null`) unless a job holds `job.lock`, in
  which case they touch `pending`. `store-open` spawns only when `last-fetch` is older than the
  interval. The hook returns in milliseconds. `doctor` answers the checks of story 23.
- **Job.** Under `job.lock`, repeat until `pending` is gone: stage `git add -A --
  ':(glob)*.md'`; commit if anything is staged, with the message from `writes`, then empty
  `writes`; if a remote exists and (it committed or the fetch is due): `git fetch`, then
  `tasq store lock -- git rebase origin/<branch>` when behind, then `git push` when ahead;
  record `last-fetch`. A rejected push repeats fetch, rebase and push once. Each failure replaces
  the lasting warning of its kind; each success clears it.
- **Commit message.** Subject `<command> <ids>: <title of the first id>` (title from `tasq view
  <id> --json` at commit time, `<command> <ids>` alone if that fails); when several writes are
  batched, the subject is the first and the body lists every `<command> <ids>` line.
- **Environment.** The background job runs `git` with `env_clear()` plus `PATH`, `HOME`,
  `SSH_AUTH_SOCK`, `GIT_TERMINAL_PROMPT=0`, `SSH_ASKPASS_REQUIRE=never`, `LANG=C`, and stdin
  `/dev/null`; no `GIT_SSH_COMMAND`, so the repository's `core.sshCommand` applies. `tasq git
  sync` keeps the user's terminal and drops only `GIT_*` from the environment.
- **Identity.** init and clone write `user.name`, `user.email` and, with `--ssh-key`,
  `core.sshCommand = ssh -i <key> -o IdentitiesOnly=yes` into the store repository's config.
  Defaults come from `[plugin.git] name`, `email`, `ssh_key`. Without any, git's own resolution
  applies and `doctor` shows what it resolved to.
- **Remote and branch** are `origin` and the branch's upstream in the repository config; the
  default branch name is the store name. `init` with a URL runs `git ls-remote --heads`: no such
  branch or an empty one is pushed; a branch with history is refused. `clone` runs a
  single-branch clone into `<data_dir>/<store>` (resolved with `tasq store info --json` under the
  chosen `--store`), refusing if the directory has task files.
- **Conflicts.** A failed rebase is `git rebase --abort`ed. The warning names the conflicting
  files' ids (read from their `id:` line), says local commits are kept, and gives the commands:
  `cd <store> && git pull --rebase`, resolve, `git rebase --continue`. The job does not push
  while that warning stands.
- **Duplicate ids.** In the store crate (not the plugin): `doctor`'s `ids` failure names each
  file's creation time and title, marks the newer one, and its fix gives the next free id
  (`highest + 1`). In the plugin: after a rebase that changed task files, the job runs `tasq
  doctor --json` and turns `ids` failures into a lasting warning, cleared when they are gone.
- **Packaging.** Workspace member, `scripts/install` installs both binaries, CI runs its tests
  (git is on the runner), the release publish list gains it after `tasq-cli`.

## Testing Decisions

- A good test drives `tasq-git` the way tasq does (a payload on stdin, or a subcommand) against
  a temp store repository with a bare remote (`file://` path), and asserts on the repositories
  (commits, messages, branches, files, config) and on the warnings file and output. It never
  asserts which internal function ran.
- **Job in the foreground.** Tests run `tasq-git job --store <dir>` directly and wait for it; one
  test proves `hook` returns before the job finishes and that the job then runs (poll for the
  commit with a deadline).
- **Two machines.** Two clones of one bare remote: write on A, sync A, sync B, B has the file;
  both write different tasks, both sync, both have both; both edit one task, the second sync
  aborts with the conflict warning naming the id and the store files are unchanged; both create
  offline with the same id, sync, the duplicate warning appears.
- **Identity.** A test with `GIT_AUTHOR_NAME`/`GIT_SSH_COMMAND` exported in the parent
  environment shows the commit carries the repository's identity; `core.sshCommand` is the
  configured one (asserted from config, no real SSH).
- **No prompts.** A remote URL that would prompt (an `ssh://` to a fake `ssh` on `PATH` that
  reads stdin) fails fast with a warning instead of hanging.
- **Setup.** init on an empty remote, on a missing branch, on a branch with history (refused);
  clone into an empty store, into a store with files (refused); identity flags and defaults.
- **Store crate.** The duplicate-id doctor text, snapshot-tested in `crates/store/tests`.
- **End to end.** One CLI test with `tasq` and `tasq-git` built, `[plugins] enabled = ["git"]`,
  a `tasq log` producing a commit with the expected subject.
- **Mutation bar.** Zero missed in `crates/git/src` and the store diff. Only the functions that
  spawn the detached job or exec `git` get `#[mutants::skip]`.
- Every cargo command goes through `scripts/guard`; one build at a time.

## Out of Scope

- The extension points (task 59).
- A task-aware three-way merge of task files; conflicts are aborted and reported.
- Renumbering duplicate ids, automatically or through a command.
- Interactive merge-or-orphan menus as in nb; setup refuses instead.
- Any git inside `tasq-store` or the CLI.
- Moving `tasq-git` to its own repository.
- Syncing anything but task files (config, `.tasq/`).

## Further Notes

- nb (7.25.4) was studied for this plan: one local repository per notebook, orphan branches only
  on the remote, background checkpoint commits, sync as fetch + rebase + push throttled at
  command start but blocking, no locking, conflict markers committed. tasq keeps the repository
  per store and the branch per store on a shared remote, and drops the rest.
- The nb-era job (`crates/store-nb/src/job.rs` and `bookkeeper.rs`, last at `d4c4c3d^`) is a
  reference for the job script, the stale lock and the failed-file pattern.

## Sub-tasks

Blocked by task 59 as a whole. Blockers first; work the frontier, one build at a time. Each
sub-task passes fmt, clippy and the full suite and closes with a row in PROGRESS.md.

### GS-001: the `tasq-git` crate answers events and does nothing for a plain store

**What to build:** the crate, its binary and packaging (workspace, install, CI, release); `hook`
reads and checks the payload (schema 1), and for a store that is not a repository returns at
once on `store-open`/`post-write` and answers `doctor` with one `Ok` "store sync off".

**Blocked by:** tasq 59.

**Status:** done

- [x] An unknown schema is a warning naming it; a missing `.git` is a no-op.
- [x] `tasq doctor` with the plugin enabled on the fixture store shows the `Ok` line.
- [x] `scripts/install` and CI build and test it.
- [x] `mutants-in crates/git/src`: zero missed.

### GS-002: `tasq git init` with identity

**What to build:** init without a URL (repository, first commit of the task files, identity in
the repository config) and with a URL (remote, branch named after the store, push; refusal on a
branch with history).

**Blocked by:** GS-001.

**Status:** done

- [x] The first commit holds exactly the task files.
- [x] Identity flags and `[plugin.git]` defaults land in the repository config.
- [x] Empty remote and missing branch are pushed; a branch with history is refused with the fix text.
- [x] `mutants-in`: zero missed.

### GS-003: writes are committed in the background

**What to build:** `post-write` records the writes and detaches the job; the job commits task
files with the agreed message under its own lock, loops on `pending`, drops `GIT_*` and cannot
prompt.

**Blocked by:** GS-002.

**Status:** done

- [x] One commit per job run with the subject and batched body lines.
- [x] `.tasq/` and non-`.md` files are never committed.
- [x] A write during a running job is committed before it exits.
- [x] Exported `GIT_*` variables do not change the commit identity.
- [x] `mutants-in`: zero missed.

### GS-004: push, fetch and rebase in the background

**What to build:** the job pushes after a commit, fetches and rebases (under `tasq store lock`)
when `store-open` finds the interval passed, retries a rejected push once, and reports failures
as lasting warnings that clear on success.

**Blocked by:** GS-003.

**Status:** done

- [x] Two-machine tests: writes on A reach B; interleaved writes on different tasks converge.
- [x] `store-open` before the interval spawns nothing; after it, one fetch.
- [x] Offline remote: warning, then cleared after a successful run.
- [x] A remote that would prompt fails fast.
- [x] `mutants-in`: zero missed.

### GS-005: conflicts are aborted and reported

**What to build:** a failed rebase is aborted with local commits kept, a lasting warning names
the conflicting task ids and the commands, and pushing stops until it is resolved.

**Blocked by:** GS-004.

**Status:** blocked

- [ ] Both machines edit one task: the second sync leaves the store files as before the rebase and the warning names the id.
- [ ] After a manual resolution the next job clears the warning and pushes.
- [ ] `mutants-in`: zero missed.

### GS-006: duplicate ids are explained

**What to build:** the store's `doctor` names both files' creation times and titles, marks the
newer and gives the next free id; the job turns `ids` failures after a pull into a lasting
warning.

**Blocked by:** GS-004.

**Status:** blocked

- [ ] Store doctor snapshot with a duplicate id.
- [ ] Two machines creating offline with one id: warning after the pull; gone after a hand fix.
- [ ] `mutants-in` the store diff and `crates/git/src`: zero missed.

### GS-007: `tasq git sync`, `tasq git clone` and the doctor checks

**What to build:** the foreground sync printing each step; clone of a remote branch into a new
store with identity; the plugin's doctor checks (identity, remote and branch, unpushed commits,
last fetch, rebase in progress).

**Blocked by:** GS-004.

**Status:** blocked

- [ ] `git sync` output snapshot for a clean sync, an offline remote and a conflict.
- [ ] Clone into an empty store works; into a store with task files is refused.
- [ ] `doctor` JSON snapshot with each check.
- [ ] `mutants-in`: zero missed.

### GS-008: the docs describe store sync

**What to build:** `docs/git-sync.md` (first and second machine, identity, interval, conflicts,
duplicate ids), links from README and `docs/plugins.md`, CLAUDE.md's architecture table and
"Commits and sync are task 57" line.

**Blocked by:** GS-005, GS-006, GS-007.

**Status:** blocked

- [ ] Setting up a second machine works from the doc alone.
- [ ] Docs only; no mutation run.
