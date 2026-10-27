# Progress: store sync as the `tasq-git` plugin

Plan: [PLAN.md](PLAN.md). Task: tasq 57. ADR: 0030. Blocked by tasq 59
([plugin-events](../plugin-events/PROGRESS.md)).

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| GS-001 | The `tasq-git` crate answers events and does nothing for a plain store | done | e71fd21. Mutants with `--no-config` on `crates/git/src`, and `--in-diff` on the whole diff: 0 missed (14 mutants: 13 caught, 1 unviable). |
| GS-002 | `tasq git init` with identity | done | 2c07c38. Mutants with `--no-config` on `crates/git/src`: 0 missed (50: 43 caught, 7 unviable); `--in-diff` on the whole diff: 0 missed (37: 31 caught, 6 unviable). |
| GS-003 | Writes are committed in the background | done | df8198e. Mutants with `--no-config` on `crates/git/src`: 0 missed (121: 103 caught, 15 unviable, 3 timeouts); `--in-diff` on the whole diff: 0 missed (77: 66 caught, 8 unviable, 3 timeouts). The timeouts are mutants that make the job loop forever (`pending` never cleared or always there). |
| GS-004 | Push, fetch and rebase in the background | done | b61d272. Mutants with `--no-config` on `crates/git/src`: 0 missed (158: 139 caught, 16 unviable, 3 timeouts, the endless-loop mutants of GS-003); `--in-diff` on the whole diff: 0 missed (52: 50 caught, 2 unviable). A first run left 2 missed (the "still behind" guard before the push); the late-write test now has a commit of its own on B. |
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

- 2026-10-27 (GS-001): `CARGO_BIN_EXE_<name>` exists only for the binaries of the test's own
  package, and a dev-dependency builds a library, never a binary. The CLI test that runs
  `tasq-git` (`crates/cli/tests/tasq_git.rs`) finds it next to `tasq` in the target directory,
  where `cargo test --workspace` puts it. `cargo test -p tasq` alone, and a cargo-mutants
  baseline of the CLI (it tests only the mutated package, in a fresh scratch build), have no
  `tasq-git`, so those tests skip with a note, like the bash-less skip of `events.rs`. A stale
  `target/debug/tasq-git` is not rebuilt by `-p tasq` either.
- 2026-10-27 (GS-001): the plugin never needs a `print!` allowance: `main` writes through
  `std::io::Write` on stdout and stderr, which the `print_stdout`/`print_stderr` lints do not
  cover, and ignores a write error (tasq stopped reading).
- 2026-10-27 (GS-002): a task file is what tasq's store reads (`is_task_file_name`): `.md` in
  any case, no leading dot, top level. The pathspecs are `:(glob,icase)*.md` and
  `:(exclude,glob).*` (`git::TASK_FILES`, for GS-003's job too): `:(glob)` keeps `*` from
  crossing `/`, so `sub/` and `.tasq/` stay out, and the exclude drops editor locks (`.#x.md`).
  `git add` with a pathspec that matches nothing fails, so an empty store skips it.
- 2026-10-27 (GS-002): `git init --initial-branch '<bad>'` creates `.git` before it rejects the
  name, so the undo must cover a failed `git init`; and when git cannot run at all there is no
  `.git`, where an unconditional `remove_dir_all` replaced git's error with `NotFound`.
- 2026-10-27 (GS-002): a global `core.hooksPath` with a failing `pre-commit` or `pre-push` is
  how the tests make a commit or a push fail on demand, with no fake git.
- 2026-10-27 (GS-003): the job and the hooks agree without a race by ordering, not by a
  check-then-act on `job.lock`: a hook appends to `writes` and touches `pending` first, and
  only then spawns a job unless one is live; the job clears `pending` before each commit,
  releases `job.lock` after it, and only then looks at `pending` again (and retakes the lock
  if it is there). A hook that saw the lock held is therefore always seen by that job, and one
  that came after the release starts its own. A spawned job that finds the lock taken exits.
- 2026-10-27 (GS-003): `writes` is appended by hooks while the job drops the lines it
  committed, so both take `File::lock` (std since 1.89) on it, and the job truncates the file in
  place (a rename would move the lock's inode away from a hook that already opened it).
- 2026-10-27 (GS-003): the first `lock_job` removed `job.lock` whenever it was not live,
  absent included, so a job starting at the same moment could delete the lock another had just
  made; and a stale holder's `Drop` deleted its successor's lock. The lock now carries a token
  (pid and time) that `Drop` checks, and only a lock seen stale is removed (self-review).
- 2026-10-27 (GS-003): `NotFound`/`AlreadyExists` match guards written inline in each `fs`
  call leave mutants no binary test can reach (they need a non-missing error). One helper per
  guard (`open_existing`, `modified`, `remove_if_present`) with a unit test below a file
  (`ENOTDIR`), and a read-only state directory for `create_new`, killed them. `lock_job` tries
  the exclusive create first, so a live lock goes through the `AlreadyExists` arm the binary
  tests cover.
- 2026-10-27 (GS-003): for GS-004: the job takes `job.lock` afresh each round, so its mtime is
  the round's start; a single round over two minutes (a slow fetch or push) is taken for dead
  and a second job starts. Refresh the lock during long git steps.
- 2026-10-27 (GS-004): `process_group(0)` detaches the job from tasq's group but not from the
  terminal: it keeps the session's controlling tty. With `SSH_ASKPASS_REQUIRE=never` (the plan,
  and GS-003's environment) OpenSSH reads a passphrase or host-key answer from `/dev/tty`, and a
  background group reading the tty is stopped by SIGTTIN, so the job would hang, not fail.
  `SSH_ASKPASS_REQUIRE=force` with `SSH_ASKPASS=false` makes every ssh prompt fail at once; git
  also asks `SSH_ASKPASS` for credentials before `GIT_TERMINAL_PROMPT=0` refuses.
- 2026-10-27 (GS-004): `git rebase --autostash` whose stash pop conflicts exits 0 and leaves
  conflict markers in the working file (git 2.47). The rebase therefore never autostashes; it
  is skipped when a task file has uncommitted changes (a write that landed after the job's
  commit and before the store lock), and the round after commits and rebases.
- 2026-10-27 (GS-004): `tasq store lock` hands its command tasq's environment, `GIT_*`
  included, so `tasq store lock -- git rebase` would let a direnv `GIT_DIR` through. The
  command is `tasq-git rebase --store <dir>` (hidden), which runs git with the background
  environment like the job.
- 2026-10-27 (GS-004): test seams with real git only: `remote.origin.uploadpack` set to a
  wrapper that logs and execs `git-upload-pack` counts fetches exactly; a global `pre-push`
  hook that pushes from the other machine on its first run stages a rejected push; a
  `pre-rebase` hook stages a failed rebase without leaving one in progress; `touch -r` and
  `find -newer` (POSIX, so macOS CI too) age the job lock mid-round and check it was refreshed.
- 2026-10-27 (GS-004): for GS-005: a failed rebase is now only the `rebase failed: ...`
  warning; a conflict leaves the rebase in progress, which `@{upstream}` and the commit round
  do not expect. The skip on uncommitted changes is not a failure and must not be aborted. For
  GS-007: `last-fetch` is the last look at the remote, failed or not (Deviations).

## Blockers

None. tasq 59 is merged.

## Deviations

- GS-001: "a warning naming it" is tasq's own rule for a failed event: `hook` exits 1 with
  `unsupported event payload schema <n> (tasq-git reads schema 1)` on stderr, which tasq
  shows as `plugin git: <event> event failed: exit status 1: ...`. For `doctor` the same exit
  is a `FAIL` check `git/doctor` (docs/plugins.md), not a warning. A payload with no schema
  (`the event payload has no schema`), one that is not JSON, or a schema 1 payload missing
  `event` or `store` fail the same way. An event the plugin does not know is exit 0, silent.
- GS-001: the `Ok` check is named `sync` (listed as `git/sync`), message `store sync off:
  <store> is not a git repository`, no fix. An absent store directory is "off" too and is not
  created. A store with `.git` (a directory, or a `gitdir:` file) answers `doctor` with no
  checks until GS-007 adds them; its other events do nothing until GS-003.
- GS-001: CI needed no workflow change: `cargo clippy --workspace` and `cargo test
  --workspace` pick up the new member on Linux and macOS. `.cargo/mutants.toml` gained
  `crates/git/**` in `examine_globs`, so the PR mutants job covers it. Beyond install and CI,
  the release builds, smoke-tests (`tasq-git --version`), packs and publishes `tasq-git` (last,
  after `tasq`), the Homebrew formula installs and tests it, and `docs/release.md` says so.
- GS-001: dependencies are clap, serde, serde_json and thiserror (as `tasq-worktree` uses it
  for errors). chrono, in the plan's list, waits for the first ticket that needs a time
  (GS-003's `last-fetch`).
- GS-001: the library is `payload` (`parse`, schema checked before the fields so a later
  schema is named, not reported as a missing field) and `hook` (`run`, `answer`,
  `is_repository`). The tests drive the binary only (`crates/git/tests/hook.rs`), plus the
  two CLI tests through a real `tasq doctor`, `list` and `log`.
- GS-002: the plugin learns the store from `$TASQ_BIN store info --json` (`store.location`,
  and `store_name.value` for the branch, not `store.name`, which is `tasq`) and the defaults
  from `$TASQ_BIN config show --json` (`config.plugin.git`); `tasq` on `PATH` when `TASQ_BIN`
  is unset. Both run with the plugin's own environment and working directory, so the profile,
  `TASQ_SET` and `.tasq.toml` resolve as for the command. git runs with that environment minus
  every `GIT_*` variable (`process::without_git_vars`), stdin `/dev/null`.
- GS-002: beyond the plan: a store that is already a repository is refused (`<dir> is already a
  git repository`), and the remote is asked before anything is written, so every refusal
  (branch with history, unreachable remote) leaves the store as it was. A failure after `git
  init` (commit, push, ...) removes the new `.git`, so init can be run again; an absent store
  directory is created and stays (empty, it is an empty store). An empty store gets an empty
  first commit (`--allow-empty`), so the branch exists and can be pushed.
- GS-002: "an empty one is pushed" reads as an empty remote (no refs): a branch always has a
  commit, so any existing branch is "with history" and refused. The refusal is `branch <b> of
  <url> already has history; tasq git init never merges two task histories` and `fix: tasq git
  clone <url> --branch <b> into an empty store, or tasq git init <url> --branch <another> to
  push to a new branch`.
- GS-002: output is `initialized <dir> as a git repository on branch <b>: <n> task file(s)
  committed`, plus `pushed branch <b> to <url>`; errors are `tasq-git: <message>`, exit 1. The
  first commit's subject is `tasq git init`. `--branch` without a URL names the local branch.
  The key path in `core.sshCommand` is single-quoted for git's shell (`'` as `'\''`); a
  leading `~` is left to ssh. A `[plugin.git]` default that is not a string is refused.
- GS-002: library modules `process` (`run`, the only `#[mutants::skip]`), `git` (`Git`,
  `TASK_FILES`, `remote_branch`), `tasq` (the two questions) and `init`. Tests drive the binary
  (`crates/git/tests/init.rs`, a fake `tasq` answering the two documents, real git, a bare
  `file://` remote) plus one CLI test through the real `tasq git init` on the fixture store.
- GS-002: added a CHANGELOG entry (PE-007's learning).
- GS-003: the hook spawns the job unless a live job holds `job.lock`, and touches `pending`
  either way (the plan touches it only when the lock is held); see Learnings for why. The job
  takes and releases `job.lock` once per commit round rather than holding it across the loop.
- GS-003: the `writes` line is `<command> <ids>`, ids separated by spaces (`move 4 3`). The
  commit subject is the first line plus `: <title>` of its first id, from `tasq view <id>
  --json`; with several lines the body lists them all, the first included. A change no line
  explains (a hand edit) is committed as `update task files`. A round with nothing to commit
  makes no commit and forgets its lines (the change was committed already).
- GS-003: beyond the plan: a failed commit (or `git status`/`git add`) is the lasting warning
  `commit failed: <git's message, lines joined by "; ">`, replacing the previous one and
  cleared by the next good round; other plugin warning lines are kept, and the file is
  removed when it has none left. The job keeps `writes` then, exits 1 with
  `tasq-git: commit failed: ...` (seen only when run by hand), and the next `post-write`
  retries. GS-004 reuses `State::set_warning` for push and fetch.
- GS-003: the job is `tasq-git job --store <dir>` (hidden from `--help`), spawned from
  `current_exe` in its own process group (`process_group(0)`, no `setsid`: `unsafe_code`
  forbids `pre_exec`) with stdio on `/dev/null`, keeping the hook's environment (so
  `TASQ_EVENT`: its `tasq view` calls fire no event) and working directory. git runs with
  `process::background_env`: `PATH`, `HOME`, `SSH_AUTH_SOCK`, `GIT_TERMINAL_PROMPT=0`,
  `SSH_ASKPASS_REQUIRE=never`, `LANG=C`, stdin `/dev/null`. `store-open` still does nothing
  (GS-004).
- GS-003: the commit is of the index, not limited by pathspec, so the commit of the job
  following a write sees the tasks' staged changes only; a file a user stages by hand in the
  store goes into that commit too (limiting `git commit` to the task-file pathspecs takes the
  working tree, which would also pull in a write that lands after `git add` without its
  message line). Writes racing the job the same way may be committed under the previous
  write's message; the content is always committed.
- GS-003: payload `command` and `writes` are optional for every event; a `post-write` to a
  repository store without either is an error (`the post-write event payload has no command`).
  Tests: `crates/git/tests/job.rs` (the job in the foreground, the detach with a fake `tasq`
  that holds the job in its title lookup until the test says go, a write made by that fake
  while the job runs), unit tests for the message, the background environment and the lock's
  clock, and one CLI test (`a_write_is_committed_in_the_background`: `tasq git init`, `tasq
  log 3`, the commit `log 3: Answer the support ticket` appears).
- GS-004: the background environment sets `SSH_ASKPASS=false` and `SSH_ASKPASS_REQUIRE=force`
  in place of the plan's `SSH_ASKPASS_REQUIRE=never` (Learnings: `never` hangs a job that still
  has the terminal).
- GS-004: the rebase is `tasq store lock -- tasq-git rebase --store <dir>`, a hidden subcommand
  (Learnings), onto `@{upstream}`; it does nothing when a task file has uncommitted changes.
  The remote and branch are the branch's upstream: no upstream, and the job only commits. The
  push is `git -c push.default=upstream push`, so a global `push.default` cannot change where
  it goes.
- GS-004: `last-fetch` records every look at the remote, failed ones and a store without an
  upstream included, so an offline machine or a local-only repository is not asked again on
  every command; a round that committed always syncs. A future `last-fetch` (the clock went
  back) is due.
- GS-004: any failed push is fetched, rebased and pushed once more, not only a rejection: no
  parsing of git's message, and an offline remote fails at the second fetch.
- GS-004: `store-open` touches `pending` too, so a running job goes round once more and fetches
  if due. The hook passes the interval to the job (`job --interval <secs>`, default 60); an
  `interval` that is not a whole number of seconds is a hook error (`[plugin.git] interval
  must be a whole number of seconds, not <json>`), for `post-write` too.
- GS-004: warnings: the failed step's kind (`fetch failed: `, `rebase failed: `, `push failed:
  `) is set, the steps before it cleared, those after it left as they were. Not behind clears
  the rebase warning, not ahead the push warning. A failed sync step is only a warning: the job
  still exits 0 (a failed commit stays exit 1). The job lock is refreshed before the fetch, the
  rebase and the push.
- GS-004: GS-003's `store_open_starts_nothing` test is gone (a repository store now fetches on
  `store-open` when due); `tests/sync.rs` covers `store-open`. Tests: `crates/git/tests/sync.rs`
  (two machines on a bare `file://` remote, the job in the foreground and through the hook),
  state unit tests (`fetch_due`, lock refresh), and one CLI test
  (`another_machines_write_is_pulled_when_the_store_opens`: `tasq git init <url>`, a push from
  a clone, `tasq list`, the commit arrives through the real `tasq store lock`).
- GS-004: a failed rebase is not aborted; that and the conflict warning are GS-005's.
