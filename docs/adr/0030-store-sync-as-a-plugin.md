# ADR-0030: Store sync is a plugin, built on generic extension points

- **Status:** Accepted
- **Date:** 2026-10-19

## Context

ADR-0026 took git out of the store: a store is a directory of task files with a
`.tasq/lock`, and nothing is committed, pulled or pushed. The user's stores
(Tasq, Silverfin) therefore have no history and no backup, and a second machine
cannot see them. Task 57 brings git back as an opt-in feature.

Forces:

1. **Nothing may block.** The user wants commits, pulls and pushes in the
   background of the CLI and the terminal UI, never in front of them.
2. **A separate identity.** The user works on employer projects with one git
   identity and SSH key and wants the stores to use another, whatever directory
   or direnv environment a command runs in.
3. **Several stores, one remote.** nb shares one remote repository between
   notebooks by pushing each notebook's unrelated history to its own branch
   ("orphan branches"); each notebook stays its own local repository.
4. **Conflicts.** nb commits conflict markers into text files and continues. A
   tasq task file with markers fails to parse and drops out of `list`.
5. **The plugin mechanism (ADR-0006) cannot carry this today.** Hooks fire only
   on `post-create`, `post-done` and `pre-launch`; most writes (`set`, `log`,
   `apply`, `sync`, every TUI edit) fire nothing; no hook is told the store
   directory; nothing runs when the store opens; a plugin cannot take the store
   lock, show a warning on the next command, add a `doctor` check or make the
   TUI notice changed files.
6. **Background work makes the lock matter.** A rebase rewrites task files in
   place, so it must not run under a write. Commits do not need the lock: every
   write is a temp file renamed into place, so `git add` sees the old file or
   the new one, never half of one.

## Decision

Store sync ships as a plugin, `tasq-git`, on extension points that are not
specific to git. Both are built in this workspace; the plugin depends on no
tasq crate, only on the documented contract, so it can move to its own
repository later without publishing crates.

**Extension points (tasq 59).** `[plugins] enabled = ["<name>"]` turns a
plugin on; tasq passes its `[plugin.<name>]` table through to it unvalidated.
tasq runs `tasq-<name> hook` for every event with a `"schema": 1` JSON payload
on stdin: `store-open` (once per command and at TUI start, with the store
directory), `post-write` (once per command after its last write, with the
command name, the ids and the file paths; once per writing action in the TUI)
and `doctor` (the plugin answers with checks on stdout). Hooks stay
synchronous with a timeout; a plugin detaches its own background work. A
plugin's lasting warnings live in `<store>/.tasq/plugins/<name>/warnings` and
are shown by every command, the TUI status bar and `doctor` until the plugin
clears them. `tasq store lock -- <cmd>` runs a command holding the store lock.
The TUI polls the store directory and reloads when it changed. An enabled
plugin that is not installed is a warning, never a failed command.

**The git plugin (tasq 57).** Each store is its own repository; its remote
branch defaults to the store name, so several stores can share one remote
repository. The remote, branch and identity (`user.*`, `core.sshCommand` with
`IdentitiesOnly`) live in the store repository's own git config, written by
`tasq git init` and `tasq git clone` from flags or `[plugin.git]` defaults.
Setup never merges unrelated histories: `init` refuses a remote branch with
history, `clone` refuses a store with task files. On `post-write` a detached
job (one per store, with a pending flag) commits the task files and pushes; on
`store-open` it fetches and rebases once the interval has passed (60 seconds by
default). Only the rebase holds the store lock. The job drops every `GIT_*`
variable and cannot prompt. A conflicted rebase is aborted, local commits stay,
and a lasting warning names the task ids. Duplicate ids are never renumbered;
`doctor` explains them and a pull that brings one in leaves a warning. A store
that is not a repository behaves exactly as without the plugin.

## Consequences

### Positive

- No command and no TUI action waits for git; only the millisecond rebase takes
  the store lock, and the lock needs no hand-over to a job.
- The extension points serve any plugin: a write hook that sees every write, a
  store-open event, lasting warnings, `doctor` checks, the store lock and a TUI
  that notices outside edits (another terminal, a hand edit, a pull).
- The plugin proves the contract: anything it needs that the contract lacks is
  found in this workspace, not after a repository split.
- The store identity holds for `git` run by hand in the store too, because it
  is the repository's own config.
- The store crate stays free of git; a user who never enables the plugin runs
  exactly what ADR-0026 describes.

### Negative

- Two tasks instead of one: the extension points come first and block store
  sync. Mitigated: they are designed and tested with a fake plugin, and each
  task gets its own plan and mutation bar.
- Every event spawns the plugin, about a millisecond for a Rust binary, also
  for events it ignores.
- The hook payloads, the warnings file and `tasq store lock` become a public
  contract with a schema version to keep.
- A conflict needs the user to resolve it with git in the store directory;
  there is no task-aware merge. Mitigated: the warning names the tasks and the
  commands, and nothing is lost (the rebase is aborted).
- The TUI polls the directory every couple of seconds while it runs.

## Alternatives considered

- **Git built into the store or the CLI** — fastest to ship, but every need it
  has (write events, background lifecycle, warnings, lock) would be private
  code, and no plugin could do the same.
- **The plugin in its own repository from the start** — no crate is published
  and no release exists, so it would be a shell script or depend on a git
  revision, with its own CI and install story, before the contract is stable.
- **Commit synchronously in the write** — fast enough locally, but the user
  wants git off the critical path entirely.
- **One repository for all stores (`<data_dir>/stores/`)** — one commit stream
  and one remote, but one store's conflict blocks the others and a per-store
  lock does not cover a repository-wide rebase.
- **nb's interactive merge-or-orphan menus** — merging unrelated task histories
  is exactly where duplicate ids come from; refusing is simpler and safer.
- **Committing conflict markers, as nb does** — the task file stops parsing and
  disappears from `list`.
- **Renumbering duplicate ids automatically** — breaks worktree branches, merge
  request links and notes that carry the id.
- **Identity from the environment** — a command run in an employer project's
  direnv would commit and push with the employer identity and key.

## References

- Builds on ADR-0006 (plugin mechanism) and ADR-0026 (store v2, `.tasq/lock`).
- Replaces the nb-era design of ADR-0023 (superseded by ADR-0027): commits in a
  background job behind `.git/tasq/lock`, `store.sync_interval`.
- Plans: `ruli/features/plugin-events/` (tasq 59), `ruli/features/git-sync/`
  (tasq 57). Terms: `CONTEXT.md` (Sync, Store sync, Plugin, Event, Hook).
