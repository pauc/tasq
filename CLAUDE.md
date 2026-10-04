# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`tasq` is a Rust rewrite of `original/tasks`, a bash script that manages todos stored as
[nb](https://github.com/xwmx/nb) markdown files. Work is driven by a phased plan:

- `ruli/features/rust-rewrite/PLAN.md`: tasks (`T-xxx`) with acceptance criteria, architecture, decisions log.
- `ruli/features/rust-rewrite/PROGRESS.md`: status per task, learnings, blockers, deviations. **Update it
  when you finish a task** (status table, learnings, commit shas). Read both before starting any task.
- `docs/adr/`: decision records. Add a new ADR rather than editing an accepted one; update the index in `docs/adr/README.md`.
- `docs/file-format.md`, `docs/config.md`, `docs/testing.md`: normative references for the markdown format, config layers and test conventions.

`original/tasks`, the user's real nb notebooks and the existing `~/.claude` skills are **never modified** by this project. The new tool ships under a new binary name and a new Claude Code plugin.

## Commands

`just` is **not installed** on this machine; the `justfile` documents the exact commands. Every cargo invocation must go through `scripts/guard` (a memory-capped systemd scope). Two OOM incidents killed the desktop session; see Blockers in PROGRESS.md.

```sh
scripts/guard cargo fmt --all --check
scripts/guard cargo clippy --workspace --all-targets -- -D warnings
scripts/guard cargo test --workspace
scripts/guard cargo doc --workspace --no-deps        # missing_docs is a warning in core and store-nb

# One test, one crate, one integration test file
scripts/guard cargo test -p tasq-core --test format_ops set_status
scripts/guard cargo test -p tasq-store-nb --test create -- --nocapture

# nb-gated tests: they skip silently when `nb` is not on PATH; force them to fail instead (CI does this)
TASQ_REQUIRE_NB=1 scripts/guard cargo test -p tasq-store-nb

# Mutation testing (never more than one run at a time, always --jobs 2)
scripts/guard cargo mutants --no-config --jobs 2 --timeout-multiplier 3 --minimum-test-timeout 20 \
    -f 'crates/core/src/format/*.rs' -f 'crates/core/src/format' -E 'impl Debug' -E 'impl Display' -E 'impl Default'
scripts/guard cargo mutants --jobs 2 --in-diff <(git diff main...HEAD)   # PR-sized
```

Rules that are easy to get wrong:

- **Only one thing compiles at a time.** Do not run parallel agents or shells that build this workspace. `.cargo/config.toml` caps `build.jobs = 8` and routes every test binary through `scripts/test-runner` (4 GiB address-space cap).
- `cargo mutants -f` does **not** narrow scope while `.cargo/mutants.toml` is loaded; use `--no-config` plus the `-E` excludes above (the `just mutants-in` recipe). Survivors land in `mutants.out/missed.txt`.
- The per-task acceptance bar is **zero missed mutants** in the module touched. Prefer fixing the code shape or adding an exact-string assertion over arguing a mutant is equivalent. Only functions that purely wrap I/O or `exec` get `#[mutants::skip]` (plain form, never `cfg_attr`) with a one-line reason comment above it.
- Clippy runs `all` + `pedantic` with warnings denied. Clippy 1.99: write `assert_eq!(v, Vec::new())`, not `assert!(v.is_empty())`.
- Commit messages are conventional commits scoped by crate: `feat(store-nb): ...`, `test(core): ...`, `docs(plan): ...`, `build: ...`, `ci: ...`.

## Architecture

Workspace of six crates; dependencies flow one way, toward `tasq-core`.

| Crate | Role |
|---|---|
| `crates/core` (`tasq-core`) | Domain model, markdown format, queries, config, clock, and the `Store`/`Source`/`Launcher` traits. **No I/O, no process spawning, no terminal or network deps.** |
| `crates/store-nb` | `Store` impl over an nb notebook plus the `Bookkeeper` strategy and `doctor` checks. The only crate that runs `nb` or `git`. |
| `crates/sources`, `crates/launch` | Stubs. Phase 5 (GitLab/GitHub/LLM-bridge sources) and Phase 4 (shell/tmux/claude/herdr launchers). |
| `crates/cli` (binary `tasq`) | Stub. Phase 3 (current). Must stay thin: every command is a core function plus rendering, and every command gets `--json`. |
| `crates/tui` | Stub. Phase 8. Depends on core only. |

### The model vs the file

The nb file encodes status and priority as `#tags` (`#gitlab #A #ready`). In the model they are **fields** of `Task` (`status: Option<Status>`, `priority: Priority`), and `Task::tags` holds topic tags only. The format layer does that mapping both ways; nothing else should ask "is this tag a status?". Statuses are configured through a `Workflow`, not hard-coded.

### Lossless format: operations, not diffs

`tasq_core::format` has two layers. `Document` keeps the file byte for byte (line endings, unknown sections, blank lines); `Task` is a read-only typed projection of it. Edits are expressed as `format::ops::*` functions on the `Document`, one per awk pass of the original script, because the script's insertion rules are **not uniform** (new sections go before `## Progress`, `## Project` goes after `## Description`, a new `## Tags` goes at end of file, `### Merge requests` nests inside `## Related`). The store applies ops, re-parses, and returns `StoreError::Unsupported` if the requested `Task` still differs, rather than silently dropping a change.

### Store semantics

`Store::update` takes the whole task; the nb store diffs it against disk (`store-nb/src/diff.rs`), applies ops, and writes atomically with `NamedTempFile::persist`. A `Revision` (mtime + length + hash) captured on read makes a later `update` fail with `Conflict` if the file changed. Task ids are `.index` line numbers (positional; `describe()` reports this so the CLI can warn).

Reads never spawn a process. After a write, a `Bookkeeper` handles `.index` registration, `nb git checkpoint --wait`, verify and sync: `NbCliBookkeeper` when `nb` is on PATH, `NativeBookkeeper` (append to `.index`, run `git`) otherwise, `NoopBookkeeper` in tests. Bookkeeping failure after a successful write is a warning with the manual fix (`nb index reconcile`), never reported as data loss. nb's `.index` is only ever appended to; never rebuild or reorder it.

### Injected everything

Time comes from a `Clock` trait, config loading from `LoadOptions` (cwd, home, env), the `nb` executable from the injected PATH. This is what makes mutation testing affordable: logic is cheap to run thousands of times against temp dirs and fake processes. Keep new code in that shape.

### Config

Layered TOML (`tasq_core::config`): defaults, global `~/.config/tasq/config.toml`, nearest `.tasq.toml` walking up to home, selected `[profile.<name>]`, `TASQ_*` env, `--set`. Layers are deep-merged as TOML tables and deserialised once; each file is also checked alone with `deny_unknown_fields` so errors carry file:line:column. `Loaded::file_for("store.notebook")` gives the provenance used in error messages. Full reference in `docs/config.md`.

## Testing nb without touching the real notebook

`crates/store-nb/tests/support/mod.rs` builds an `NbEnv`: a temp copy of `tests/fixtures/nb/home` with `NB_DIR`, `NBRC_PATH`, `HOME` (with a `.gitconfig`, nb refuses to work without a git identity) and `NB_AUTO_SYNC=0`. Every `nb` invocation in tests gets only that environment. Fixture ids are named constants (`id::FULL`, `id::DONE`, `id::MISSING`, ...). Tests that need the real `nb` call `nb_or_skip`. Probed nb behaviours (async checkpoints, exit codes of `nb git dirty`, index renumbering on reconcile) are recorded under "nb facts" in PROGRESS.md; check there before assuming how nb behaves.

Markdown format fixtures live in `crates/core/tests/fixtures/*.md`; round trips must be byte-identical.
