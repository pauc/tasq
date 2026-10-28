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
| GS-005 | Conflicts are aborted and reported | done | d6671fc. Mutants with `--no-config` on `crates/git/src`: 0 missed (174: 154 caught, 17 unviable, 3 timeouts, the endless-loop mutants of GS-003); `--in-diff` on the ticket's diff: 0 missed (27: 26 caught, 1 unviable). |
| GS-006 | Duplicate ids are explained | done | f3689d5. Mutants with `--no-config` on `crates/git/src`: 0 missed (191: 170 caught, 18 unviable, 3 timeouts, the endless-loop mutants of GS-003); `--in-diff` on the ticket's diff (store and plugin): 0 missed (36: 29 caught, 7 unviable). |
| GS-007 | `tasq git sync`, `tasq git clone` and the doctor checks | done | a4d298d. Mutants with `--no-config` on `crates/git/src`: 0 missed (241: 209 caught, 29 unviable, 3 timeouts, the endless-loop mutants of GS-003); `--in-diff` on the ticket's diff: 0 missed (87: 71 caught, 15 unviable, 1 timeout, the same). |
| GS-008 | The docs describe store sync | done | cd2324b. Docs only, no mutation run: the diff has no Rust, so 0 mutants and 0 missed. fmt, clippy and the full suite (1412 tests) pass. The doc's steps were run with the built `tasq` and `tasq-git` against a bare `file://` remote and two isolated `HOME`s (Learnings). |
| GS-009 | Fix the branch review findings | done | adcf56d. All ten findings verified, a failing test for each real one first (1-7); 8 and 9 fixed as conventions, 10 by restoring `main`'s `ambiguous`; none left as a false positive (Deviations). Mutants with `--no-config` on `crates/git/src`: 0 missed (235: 194 caught, 38 unviable, 3 timeouts, the endless-loop mutants of GS-003); `--in-diff` on the ticket's diff: 0 missed (58: 42 caught, 16 unviable); `--in-diff` on the branch's store diff: 0 missed (15: 10 caught, 5 unviable). fmt, clippy and the full suite (1420 tests) pass. |
| GS-010 | Keep `.tasq/` out of `git status` | done | 6ab426d (tasq 62). Mutants with `--no-config` on `crates/git/src`: 0 missed (241: 199 caught, 39 unviable, 3 timeouts, the endless-loop mutants of GS-003). A first `--in-diff` run left one missed in `git_init` (the `NotFound` guard on reading `info/exclude`: any other read error fails the write after it with the same message); the read is now `read(..).unwrap_or_default()`, bytes, so a non-UTF-8 template exclude is no error either. The same run's two missed in `crates/cli/src` (`cli.rs` `early_globals`, `worktree_hooks.rs` `run`) are in files this ticket does not touch. fmt, clippy and the full suite (1447 tests, rebased on `main` 7688d53) pass. |

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
- 2026-10-27 (GS-005): the abort must happen inside `tasq-git rebase`, under the store lock,
  and the conflicting files must be read before it: `git diff --name-only --diff-filter=U`
  lists them only while the rebase is stopped. Their ids are then read from the working tree,
  which the abort has put back to the local version (no markers in the frontmatter).
- 2026-10-27 (GS-005): "the job does not push while that warning stands" needs no check of
  the warnings file: after an abort the branch is still behind its upstream, and the job never
  pushes while behind. Each later round rebases again and either conflicts again (the warning
  is set anew) or, once resolved, is not behind and pushes (the warning is cleared).
- 2026-10-27 (GS-005): the user's own `git pull --rebase` leaves a rebase in progress with
  conflict markers in the working tree. A job round then would `git add -A` and commit the
  markers, and `tasq-git rebase` would abort the user's rebase on its failed `git rebase`. Both
  now check `rebase-merge`/`rebase-apply` (`git rev-parse --git-path`) first.
- 2026-10-27 (GS-005): `git rebase --continue` after a conflict opens the editor for the
  message; the test passes `-c core.editor=true`.
- 2026-10-27 (GS-006): `tasq doctor` exits 1 whenever a check fails, so the plugin reads
  `doctor --json` from stdout whatever the exit status; a duplicate id is exactly the case it
  asks about. The job's `tasq doctor` fires no plugin event (it inherits `TASQ_EVENT`), so it
  cannot recurse into the plugin's own `doctor` answer.
- 2026-10-27 (GS-006): ULID names sort in creation order, so "the newer file" is the name
  order for files tasq created; only a hand-named file (no creation time) needs the explicit
  ordering, and it counts as newer than any dated file.
- 2026-10-27 (GS-006): the fixture store's task 3 is a 2026-09 ULID; a test file standing
  for "another machine's later task" needs a later ULID or doctor marks the fixture's file
  as the newer.
- 2026-10-27 (GS-007): every `tasq` the foreground sync runs (`store info`, `view` for the
  commit title, `store lock`, `doctor`) gets `TASQ_EVENT=sync`. Without it the `store info`
  call fires `store-open`, whose hook (fetch due) spawns a background job beside the sync, and
  each title lookup during the commit touches `pending`, so the sync went round twice.
- 2026-10-27 (GS-007): `git var GIT_COMMITTER_IDENT` is git's own identity resolution, local
  config, global config and the user/host fallback, without committing; with
  `user.useConfigOnly` and no email it fails, and its last line is the reason (the lines above
  are git's `--global` advice).
- 2026-10-27 (GS-007): a clone into a store directory that already holds `.tasq/` cannot be a
  `git clone` (it wants an empty directory). `git init`, `git remote add -t <b>` (the
  single-branch refspec), `git fetch` and `git checkout --track origin/<b>` on the unborn
  branch do the same and keep the other files; a tracked file colliding with an untracked one
  fails the checkout, which the undo of init then covers.
- 2026-10-27 (GS-007): while a rebase is stopped `HEAD` is detached, so `@{upstream}` and
  `branch --show-current` say nothing useful; doctor then reports the identity and the failed
  rebase check only.

- 2026-10-27 (GS-008): the doc was checked by running it: two isolated `HOME`s with the doc's
  `[plugin.git]` table verbatim, a bare `file://` remote, `tasq git init <url>` on A, `tasq git
  clone <url>` on B, a `tasq log` on B pulled by `tasq git sync` on A, a conflict while the
  remote was moved away (the warning, `git pull --rebase`, resolve, `git rebase --continue`,
  `tasq git sync` clears it), a duplicate id created offline on both (the warning, the `id:`
  edit, cleared on both), and `git remote add` plus `git push -u` on a local-only store (the
  job then pushes). The example outputs in the doc are those runs' texts with the paths
  shortened. No SSH was exercised.
- 2026-10-27 (GS-008): `tasq list` shows both files of a duplicate id (with a warning); only
  `get` refuses the id. A first draft said otherwise.

- 2026-10-27 (GS-009): `GIT_CONFIG_COUNT` with `GIT_CONFIG_KEY_<n>`/`GIT_CONFIG_VALUE_<n>` is
  config at command-line scope: it beats the global and the repository config, reaches every
  git child (the rebase under `tasq store lock` included), and git drops it (`local_repo_env`)
  for the `receive-pack`/`upload-pack` of a local transport, so a bare remote's own hooks still
  run. `core.hooksPath=/dev/null` turns every hook off.
- 2026-10-27 (GS-009): with hooks off, a hook is no longer a failure seam. The tests use a `git`
  first on `PATH` that runs a snippet for a matching `case " $* "` line and execs the real git
  (std's `Command` looks the program up in the child's `PATH` when one is set), and, for a push
  that `tasq git init` must see refused, a `pre-receive` hook in the bare remote.
- 2026-10-27 (GS-009): an OS file lock (`File::try_lock`, std since 1.89) is what the job lock
  needed: the OS drops it when the job exits however it exits, so there is no stale rule to get
  wrong (a hung fetch let a second job in) and no break-then-create race (two jobs breaking a
  stale lock). std opens files `O_CLOEXEC`, so git children never inherit it. The hook's probe
  (`try_lock_shared`) can make a job starting at that instant exit; harmless, as the hook then
  starts its own.
- 2026-10-27 (GS-009): a git killed while it waits on ssh leaves the ssh child holding git's
  stdout and stderr; `process::run_within` does not join its reader threads after a kill, or a
  timed-out fetch would wait for ssh after all.
- 2026-10-27 (GS-009): `std::env::current_dir` fails (`ENOENT`) in a directory that was
  removed, and so does `Command::current_dir` with that path; the job then ran nothing at all.

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
- GS-005: the warning is `rebase conflict on task <id>: local commits kept, nothing pushed
  until it is resolved: cd <store> && git pull --rebase, fix the files, git add them, git
  rebase --continue` (`tasks 3, 7` for several; a file with no `id:` is named by its file
  name). The store path is not shell-quoted. `rebase conflict` and `rebase failed` share the
  warning kind `rebase `, so either replaces the other. `tasq-git rebase` prints the conflict
  text on stderr; the job tells it from a plain failure by its `conflict on ` start.
- GS-005: beyond the plan: while a rebase is in progress in the store (the user resolving
  one), the job does nothing at all: no commit (the writes wait in `writes`), no sync, no
  `last-fetch`, warnings as they were. `tasq-git rebase` refuses one in progress with `a rebase
  is in progress: finish it with git rebase --continue or --abort` instead of touching it. A
  stopped rebase with no unmerged file is aborted too and reported as `rebase failed: <git's
  message>`.
- GS-005: a round whose rebase was left for the next round (an uncommitted write, GS-004) now
  clears only the fetch warning; before, it cleared the rebase and push warnings too, which
  would have dropped a standing conflict warning until the next round set it again.
- GS-005: tests in `crates/git/tests/sync.rs` (`mod conflict`: the abort, a later write, a
  deferred rebase, the manual resolution end to end) and unit tests for `task_id` and the
  warning text. No CLI test: the CLI path is GS-004's, unchanged. CHANGELOG entry added.
- GS-006: the store's `ids` failure is `id <n> is claimed by more than one file: <file>
  (created <YYYY-MM-DD HH:MM>, "<title>"), <file> (created ..., "<title>", newer)`, files
  oldest first (creation time, then name; a file without a ULID name is `creation time
  unknown` and sorts after the dated ones). The fix is `give the newer file the next free id:
  write 'id: <n>' in <file> (or remove it if it is a copy)`, or the plural form listing each
  newer file; free ids are handed out from the highest plus one across every duplicate, so
  two failures never offer the same id. `list`'s warning and `get`'s `AmbiguousId` are
  unchanged.
- GS-006: "after a rebase that changed task files" is "after a sync that moved `HEAD`": only
  task files are committed, so a rebase that moves the branch brought task files; no extra
  `git diff`. The job also asks doctor in any round while a `duplicate id: ` warning stands,
  which is what clears it after a hand fix on either machine. A hand fix with no tasq write
  is picked up by the next job: the next write, or the next `store-open` once the fetch is due.
- GS-006: the warning is `duplicate id: <doctor's detail>; fix: <doctor's fix>`, one line per
  `ids` failure (`State::set_warnings`, of which `set_warning` is now the one-line case). A
  `tasq doctor` that gives no JSON leaves the warnings as they were and is not an error.
- GS-006: tests: `crates/store/tests/read.rs` (the doctor text: two dated files, three files
  with a hand-named one, consecutive free ids), `crates/git/tests/sync.rs` (`mod duplicate`,
  with a fake `doctor --json` answering from the store in `FAKE_STORE` and logging each call,
  so the test also pins when doctor is not asked), and one CLI test
  (`an_id_pulled_in_twice_is_a_lasting_warning_until_fixed_by_hand`: real `tasq`, a pushed
  duplicate from a clone, the warning, the hand fix, a `tasq log` whose job clears it).
  CHANGELOG entry added.
- GS-007: `tasq git sync` runs the job's rounds (`job::foreground`) with interval zero, so it
  always fetches, and prints one line per step on stdout: `committed: <subject>` or `nothing
  to commit`, `fetched <upstream>`, `pulled <n> commit(s) from <upstream>`, `pushed <n>
  commit(s) to <upstream>`, `up to date with <upstream>`, `push failed, fetching again: ...`,
  `no upstream: nothing to fetch or push`; a failed step prints the lasting warning it leaves
  (`fetch failed: ...`, `rebase conflict on ...`, `duplicate id: ...`) and the exit is 1. It
  updates the warnings and `last-fetch` like the job. git runs with the user's environment
  minus `GIT_*` (it may prompt); the rebase still runs as `tasq-git rebase` under `tasq store
  lock`. A failed commit is `tasq-git: commit failed: ...` on stderr, exit 1.
- GS-007: beyond the plan: `tasq git sync` waits for a running background job (polling
  `job.lock`, which goes stale after two minutes), and goes round again for writes that land
  meanwhile, as the job does. A store that is not a repository, or one with a rebase in
  progress, is refused (`tasq-git: <store> is not a git repository: ...`, `tasq-git: a rebase is
  in progress in <store>: ...`).
- GS-007: `tasq git clone <url> [--branch] [--store <name>] [--name --email --ssh-key]` is not
  a `git clone` but init, the identity, `remote add -t <b>`, fetch and `checkout --track`
  (Learnings). `--store` is passed to tasq as `--set store.name=<name>`; the branch defaults
  to the resolved store name. Refusals, all before the store is touched: an existing
  repository (init's text), task files (`<store> already has task files; tasq git clone only
  fills an empty store` with the fix `tasq git clone <url> --store <another> into a new store,
  or tasq git init <url> --branch <another> to push this one`), a missing branch (`branch <b> of
  <url> does not exist`, fix `tasq git init <url> --branch <b> pushes a store to it`), an
  unreachable remote. Output `cloned branch <b> of <url> into <store>: <n> task file(s)`.
  `init.rs` now shares `create`, `or_undo` and `nearest_existing` with `clone.rs`; the undo's
  error says `setup failed and could not remove`.
- GS-007: doctor checks for a repository store, in order: `identity` (`commits as <name>
  <email>`, plus `, pushes with <core.sshCommand>`; `fail` with git's reason and the fix `git -C
  <store> config user.name '<name>' && git -C <store> config user.email '<email>'`),
  `remote` (`branch <b> tracks <upstream> at <url>`, or `ok` `branch <b> has no upstream:
  commits stay local`), `push` (`every commit is pushed`, or `warn` `<n> commit(s) not
  pushed`, fix `tasq git sync`; counted against the upstream as of the last fetch), `fetch`
  (`last fetch <n> s/min/h/d ago`, or `warn` `never fetched`, fix `tasq git sync`), `rebase`
  (`no rebase in progress`, or `fail` `a rebase is in progress in <store>` with the commands).
  `push` and `fetch` only with an upstream; while rebasing only `identity` and `rebase`. No
  check asks the remote. The hook's `Check` gained an optional `fix`; `hook::run` and `answer`
  take the `Git` (background environment) and `now`.
- GS-007: tests: `crates/git/tests/sync.rs` (`mod foreground`: the three snapshots plus counts,
  retry, no upstream, a duplicate, waiting, refusals, a failed commit; `mod doctor`: JSON
  snapshots), `crates/git/tests/init.rs` (`mod clone`), unit tests for `ago` and
  `has_task_files`, and two CLI tests (`git_sync_prints_each_step_and_doctor_explains_the_repository`,
  `git_clone_fills_another_store`) through the real `tasq`. The hook tests for a `.git` store
  now expect the repository checks. CHANGELOG entry added.
- GS-008: beyond the ticket's list: `docs/architecture.md` gains `crates/git` in the crate
  table and the diagram (outside the dependency graph), `docs/config.md` links the
  `[plugin.git]` keys, and the `[plugin.git]` examples of `docs/config.md` and
  `docs/plugins.md` drop `remote = "origin"` (no such key; the remote is the repository's
  upstream) for `email`. The doctor example of `docs/plugins.md` fixes `tasq git push`, which
  does not exist, to `tasq git sync`; its other illustrative lines are kept. CHANGELOG entry
  added.
- GS-008: adding a remote to a store initialized without one has no command; the doc gives
  `git -C <store> remote add origin <url> && git -C <store> push -u origin <branch>`, which
  the job then follows (it syncs whatever upstream the branch has).
- GS-009: findings 1 (a fetch or push hanging past the two-minute stale rule let a second job
  run) and 6 (two jobs breaking one stale lock could both run) are fixed together: `job.lock`
  is an OS file lock (`State::lock_job`, `wait_job`, `job_running`), so story 28's "after two
  minutes" is now "at once". The lock file stays after the job. On top, every git of the
  background job is killed after two minutes (`Git::timeout`, `process::run_within`, hidden `job
  --timeout <secs>` for the tests); the foreground (`tasq git sync`, init, clone, the hook's
  doctor, the rebase) has no timeout.
- GS-009: finding 2: the job's `tasq` (`Tasq::for_store`) runs in the job's working directory
  or, when that is gone, in the store, and always with `TASQ_DATA_DIR` and `TASQ_STORE_NAME`
  pinned to the store it was given, so a `.tasq.toml` of either directory cannot send its
  title lookups, doctor or `store lock` to another store.
- GS-009: finding 3: a bad `[plugin.git] interval` is still the event's error, but after the
  work, with the default interval: `post-write` records and starts the job, `store-open` fetches
  when due.
- GS-009: finding 4: the background allowlist grows (`XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`,
  `DBUS_SESSION_BUS_ADDRESS`, the proxy variables git reads) rather than becoming "everything
  but `GIT_*`", which would let a terminal-only variable decide whether the job prompts.
  `GNUPGHOME` stays out: nothing signs (finding 5).
- GS-009: finding 5: commit signing and hooks are turned off by `GIT_CONFIG_COUNT` variables in
  both environments (`process::background_env`, `foreground_env`), so every git the plugin
  runs (job, rebase, `tasq git sync`, init, clone, doctor) is unsigned and unhooked, existing
  store repositories included. Not written into the store's config: a commit made by hand in the
  store stays the user's (signed, hooked). The tests that made commits, pushes and rebases fail
  through global or repository hooks use the `git` wrapper or a remote `pre-receive` instead
  (Learnings); their messages changed where git's own lines no longer follow (`push failed:
  nope`).
- GS-009: finding 7: the stopped rebase is aborted before the error of listing its conflicting
  files is returned; that error becomes `rebase failed: <message>`.
- GS-009: findings 8 and 9, conventions rather than bugs, are fixed because the fix is small:
  `spawn_detached` takes its environment (main passes its own, unchanged behaviour), and the
  job takes its clock as `Settings::now`. The job lock no longer reads the time at all.
- GS-009: finding 10: `Scan::ambiguous` is back to its `main` form (one `single` per id) rather
  than built from `duplicates()`: mapping `duplicates()` straight to `AmbiguousId` would list a
  hand-named file's paths in creation order instead of name order.
- GS-009: no finding was left as a false positive.
