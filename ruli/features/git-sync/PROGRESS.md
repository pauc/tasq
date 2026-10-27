# Progress: store sync as the `tasq-git` plugin

Plan: [PLAN.md](PLAN.md). Task: tasq 57. ADR: 0030. Blocked by tasq 59
([plugin-events](../plugin-events/PROGRESS.md)).

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| GS-001 | The `tasq-git` crate answers events and does nothing for a plain store | done | e71fd21. Mutants with `--no-config` on `crates/git/src`, and `--in-diff` on the whole diff: 0 missed (14 mutants: 13 caught, 1 unviable). |
| GS-002 | `tasq git init` with identity | blocked | |
| GS-003 | Writes are committed in the background | blocked | |
| GS-004 | Push, fetch and rebase in the background | blocked | |
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
