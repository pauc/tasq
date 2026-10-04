# Contributing

## Building

A stable Rust toolchain (`rust-toolchain.toml`, MSRV 1.90 in `Cargo.toml`) and
[`just`](https://github.com/casey/just). The `justfile` documents every command; without
`just`, run the cargo command of the recipe by hand.

Two rules that are not optional:

1. **Every cargo invocation goes through `scripts/guard`.** `scripts/guard cargo test
   --workspace`, never `cargo test --workspace`. The guard runs the command in a transient
   systemd user scope with a hard memory ceiling (12G by default, `GUARD_MEM` or `--mem` to
   change it, no swap). When a build or a test exceeds it, only the processes inside the scope
   are killed. Where `systemd-run` is unavailable (CI containers, macOS) the guard prints a
   notice and runs the command directly.
2. **Only one thing compiles at a time.** Do not run several shells, editors or agents that
   build this workspace concurrently, and never more than one `cargo mutants`.

Why: this workspace killed the desktop session twice on a 32-core, 62 GB machine. The first
time three concurrent builds with cargo's default job count exhausted memory; the second time a
mutant turned a loop infinite and allocated tens of gigabytes faster than any timeout. The
fixes are structural (`.cargo/config.toml` caps `build.jobs = 8` and routes every test binary
through `scripts/test-runner`, which caps its address space at 4 GiB; dev profiles carry
reduced debuginfo) but they only hold when the two rules above are followed. Details under
"Blockers" in `ruli/features/rust-rewrite/PROGRESS.md`.

```sh
scripts/guard cargo fmt --all --check
scripts/guard cargo clippy --workspace --all-targets -- -D warnings
scripts/guard cargo test --workspace
scripts/guard cargo doc --workspace --no-deps
scripts/guard cargo test -p tasq-core --test format_ops set_status      # one test
TASQ_REQUIRE_NB=1 scripts/guard cargo test -p tasq-store-nb              # nb-gated tests must run
```

## Quality gates

`just check` (format, clippy, tests) must pass before a commit. CI (`.github/workflows/ci.yml`)
runs the same plus `cargo deny check`, an MSRV build with `cargo +1.90`, line coverage with
`cargo-llvm-cov`, and `cargo mutants --in-diff` against the PR base; a mutant that survives in
changed code fails the PR.

| Gate | Rule |
|---|---|
| `cargo fmt --all --check` | No diff |
| `cargo clippy --workspace --all-targets -- -D warnings` | `all` and `pedantic` enabled in `Cargo.toml`; the few allowed lints are listed there with a reason. Clippy 1.99: write `assert_eq!(v, Vec::new())`, not `assert!(v.is_empty())` |
| `cargo test --workspace` | Offline. nb-gated tests skip silently without `nb` on `PATH`; set `TASQ_REQUIRE_NB=1` to make a skip a failure (CI does) |
| `cargo doc --workspace --no-deps` | No warnings; `tasq-core` and `tasq-store-nb` have `#![warn(missing_docs)]` |
| `cargo deny check` | Every dependency GPL-3.0-compatible (`deny.toml`) |
| Mutation testing | Zero missed mutants in the module touched (below) |

## Mutation testing

[cargo-mutants](https://mutants.rs/) (`cargo install cargo-mutants --locked`) is how test
quality is measured, not line coverage. The acceptance bar for a task is **zero missed mutants
in the module touched**.

```sh
just mutants                                  # code changed since main (what a PR is judged on)
just mutants-in crates/core/src/format        # one module
scripts/guard cargo mutants --no-config --jobs 2 --timeout-multiplier 3 --minimum-test-timeout 20 \
    -f 'crates/core/src/format/*.rs' -f 'crates/core/src/format' \
    -E 'impl Debug' -E 'impl Display' -E 'impl Default'           # what mutants-in runs
```

- Always `--jobs 2` at most, and one run at a time: each job builds a full copy of the tree.
- `-f` does not narrow the scope while `.cargo/mutants.toml` is loaded (cargo-mutants 27.1
  unions it with `examine_globs`). Use `--no-config` plus the `-E` excludes, as the recipe does.
  Check with `cargo mutants --list ...` before trusting a scope.
- Survivors land in `mutants.out/missed.txt`. For each one, add a test with an exact assertion
  or change the code shape so the mutant is no longer equivalent. Do not argue a mutant away in
  a report; two cases in the format module were fixed by removing an unreachable guard and a
  redundant branch.
- `#[mutants::skip]` is allowed only on functions that purely wrap I/O or `exec` (spawn a
  process, replace the process, read a file, touch the terminal). Plain form, never
  `#[cfg_attr(test, mutants::skip)]`, with a one-line `Reason:` comment above it. The
  `mutants` crate is a regular dependency because the attribute sits on non-test code.
- The CLI and TUI crates are excluded from mutation testing and covered by `insta` snapshots
  instead; their pure modules are still checked per task with `--no-config -f`.

## Tests

Conventions, with examples, in [`docs/testing.md`](docs/testing.md). The short version:

- Logic is pure with injected `Clock`, filesystem views, environment and fake processes, so a
  test runs it against a temp dir in microseconds. Keep new code in that shape; it is what makes
  mutation testing affordable.
- Markdown fixtures live in `crates/core/tests/fixtures/*.md`; round trips must be
  byte-identical.
- The nb store tests build an `NbEnv` (`crates/store-nb/tests/support/mod.rs`): a temp copy of
  `tests/fixtures/nb/home` with `NB_DIR`, `NBRC_PATH`, `HOME` (holding a `.gitconfig`) and
  `NB_AUTO_SYNC=0`, so the real `nb` only ever sees that copy. Fixture ids are the constants in
  `support::id`. Tests that need the real `nb` call `nb_or_skip`.
- The CLI tests use `TestEnv` (`crates/cli/tests/support/mod.rs`): the same fixture notebook,
  an isolated `HOME` and a `PATH` holding only `git`, so output is identical with or without nb
  installed. `TASQ_NOW="YYYY-MM-DD HH:MM"` pins timestamps.
- Snapshots are `insta`; accept changes with `INSTA_UPDATE=always scripts/guard cargo test -p
  <crate>` (no `cargo insta` needed), then review the diff.
- No test touches the network. HTTP goes through an injected `Transport`.
- Probed nb behaviours are recorded under "nb facts" in `PROGRESS.md`; read it before assuming
  how nb behaves.

## Commits

Conventional commits scoped by crate or area:

| Prefix | Use |
|---|---|
| `feat(core):`, `feat(store-nb):`, `feat(sources):`, `feat(launch):`, `feat(cli):`, `feat(tui):` | New behaviour in that crate |
| `fix(<crate>):` | Bug fix |
| `test(<crate>):` | Tests only |
| `docs(plan):`, `docs(adr):`, `docs:` | Plan and progress files, ADRs, other docs |
| `build:`, `ci:` | Workspace, cargo, guard and workflow changes |

One logical change per commit. `CHANGELOG.md` is derived from these messages.

## Architecture decisions

Significant decisions are ADRs in `docs/adr/`, written when the decision is taken. An accepted
ADR is never edited to say something else: write a new one that supersedes it and update the
index in [`docs/adr/README.md`](docs/adr/README.md). Template and process in that file.

## Where things are documented

| Topic | File |
|---|---|
| Plan, tasks and acceptance criteria | `ruli/features/rust-rewrite/PLAN.md` |
| Status per task, learnings, blockers, deviations | `ruli/features/rust-rewrite/PROGRESS.md` (update it when you finish a task) |
| Crates, their roles and dependencies (with diagrams) | `docs/architecture.md` |
| Markdown task format (normative) | `docs/file-format.md` |
| Configuration keys and defaults | `docs/config.md` |
| Sources and `tasq sync` | `docs/sources.md` |
| `--json` payloads and `tasq apply` | `docs/json.md` |
| Plugins and hooks | `docs/plugins.md` |
| Testing conventions and mutation testing | `docs/testing.md` |
| Release process | `docs/release.md` |
| Migration from the `tasks` script | `docs/migration.md` |
| Claude Code plugin | `plugins/claude/README.md` |
| Example configurations and plugins | `examples/README.md` |

Public items in `tasq-core` carry doc comments; `cargo doc` must stay warning-free.

## What never changes

- `original/tasks`, the bash script this project rewrites. Read-only reference.
- The user's real nb notebooks. Tests only ever touch temp copies of the fixture notebook, and
  nb is only ever run with `NB_DIR` pointing inside one.
- The existing `~/.claude` skills (`/wrapup`, `/update-tasks`, `/update-support-tasks`,
  `/time-logs`). The new tool ships its own plugin under `plugins/claude`.
