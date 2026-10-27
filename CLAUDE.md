# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`tasq` is a Rust rewrite of `original/tasks`, a bash script that managed todos stored as
[nb](https://github.com/xwmx/nb) markdown files. tasq owns its storage: a store directory of
`<ULID>.md` task files with a frontmatter block (ADR-0026). Work is driven by a phased plan:

- `ruli/features/rust-rewrite/PLAN.md`: tasks (`T-xxx`) with acceptance criteria, architecture, decisions log.
- `ruli/features/rust-rewrite/PROGRESS.md`: status per task, learnings, blockers, deviations. **Update it
  when you finish a task** (status table, learnings, commit shas). Read both before starting any task.
- `docs/adr/`: decision records. Add a new ADR rather than editing an accepted one; update the index in `docs/adr/README.md`.
- `docs/file-format.md`, `docs/config.md`, `docs/testing.md`: normative references for the markdown format, config layers and test conventions.

`original/tasks`, the nb notebooks it manages and the existing `~/.claude` skills are **never modified** by this project. The new tool ships under a new binary name and a new Claude Code plugin.

## Commands

`just` is **not installed** on this machine; the `justfile` documents the exact commands. Every cargo invocation must go through `scripts/guard` (a memory-capped systemd scope). Two OOM incidents killed the desktop session; see Blockers in PROGRESS.md.

```sh
scripts/guard cargo fmt --all --check
scripts/guard cargo clippy --workspace --all-targets -- -D warnings
scripts/guard cargo test --workspace
scripts/guard cargo doc --workspace --no-deps        # missing_docs is a warning in every crate

# One test, one crate, one integration test file
scripts/guard cargo test -p tasq-core --test format_ops set_status
scripts/guard cargo test -p tasq-store --test create -- --nocapture

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
- Restriction lints on top (`[workspace.lints.clippy]`): no `unwrap()` outside tests (use `expect("<invariant>")`), no `print!`/`eprint!` outside `tasq` (the CLI) and the launchers, no `dbg!`/`todo!`/`unimplemented!`, and every `#[allow]` carries `reason = "..."`. Integration test crates allow `unwrap_used` at their root.
- `crates/core/clippy.toml` bans filesystem, env, clock and process APIs in `tasq-core` (`disallowed-methods`/`disallowed-types`). The sanctioned exceptions (`SystemClock`, config file loading in `config/load.rs`) carry `#[expect(clippy::disallowed_methods, reason = "...")]`; do not add more without a reason. Clippy reads only the nearest `clippy.toml`, so keys shared with the root `clippy.toml` are duplicated there; keep them in sync. Clippy caches results: after changing a `clippy.toml`, `touch` the crate roots or the change looks like it had no effect.
- Commit messages are conventional commits scoped by crate: `feat(store): ...`, `test(core): ...`, `docs(plan): ...`, `build: ...`, `ci: ...`.

## Architecture

Workspace of eight crates; dependencies flow one way, toward `tasq-core`, except `tasq-git`, which depends on no tasq crate.

| Crate | Role |
|---|---|
| `crates/core` (`tasq-core`) | Domain model, markdown format, queries, config, clock, and the `Store`/`Source`/`Launcher` traits. **No I/O, no process spawning, no terminal or network deps.** |
| `crates/store` (`tasq-store`) | The store the CLI opens (ADR-0026, built under `ruli/features/store-v2/`). `TasqStore` reads a directory of `<ULID>.md` task files (the id map is the files' frontmatter, scanned on every call; `ulid` decodes the creation time from the name) and creates tasks under the store lock (`lock`: `.tasq/lock`, stale after two minutes; highest id plus one; a no-clobber write), and updates under it too (`Revision` compared against the one remembered on read, `diff::apply`, an atomic write over the file). Holds the `Revision` check (mtime, length, hash), `diff::apply` (whole task in, `format::ops` calls out), `draft` (the draft check and default note) and `fingerprint` (what the TUI polls to reload on outside changes). |
| `crates/sources` (`tasq-sources`) | `Source` implementations: GitLab and GitHub review requests and work items over a shared forge client, and the LLM bridge that turns a command's JSON into items. |
| `crates/launch` (`tasq-launch`) | `Launcher` implementations (shell, tmux, Claude Code, herdr) and the command `Summarizer` behind `tasq summary`. |
| `crates/worktree` (`tasq-worktree`) | Runs `git` for declared worktrees (ADR 0022): branch state, `git worktree add`, creating a task's worktree (`create::materialize`: provisioning, hooks, the trust store), releasing it at close (`release::release`: removal, branch deletion, remove hooks), the rows of `worktree ls` (`listing::rows`: scope, changes). The decisions live in `tasq_core::worktree`; the CLI passes in the terminal question (`hooks::Approver`). |
| `crates/cli` (binary `tasq`) | Argument parsing, config loading, opening the store, rendering, hooks and plugins: dispatch to `tasq-<name>`, and the events told to `[plugins] enabled` (`plugins::fire`; `store-open` in `App::open_store`, `post-write` from the `writes::Recording` store decorator after the command, `doctor`), lasting warnings, `tasq store lock` (ADR 0030, `docs/plugins.md`). Must stay thin: every command is a core function plus rendering, and every command gets `--json`. |
| `crates/git` (binary `tasq-git`) | The store sync plugin (ADR 0030, `docs/git-sync.md`). Depends on no tasq crate: it knows tasq only through the event payloads, `tasq ... --json` and `tasq store lock`. `hook` answers events (`store-open` and `post-write` detach `job`, the `doctor` event gets the repository checks of `doctor::checks`); `job` commits the task files and fetches, rebases (as `tasq-git rebase` under `tasq store lock`) and pushes, foreground for `tasq git sync`; `init` and `clone` set up a store with its identity; `state` is `.tasq/plugins/git/` (job lock, `pending`, `writes`, `last-fetch`, lasting warnings). Every `git` runs through `git::Git` with an injected environment (`process::background_env`: no `GIT_*`, no prompts). |
| `crates/tui` (`tasq-tui`) | Full-screen terminal UI. Depends on core only; the outside world comes through a `Host` the CLI provides. |

### The model vs the file

A task file holds status, priority and topic tags as frontmatter keys (`status: ready`, `priority: A`, `tags: [gitlab]`; done is `status: done`). In the model they are **fields** of `Task` (`status: Option<Status>`, `priority: Priority`), and `Task::tags` holds topic tags only. The format layer does that mapping both ways; nothing else should ask "is this tag a status?". Statuses are configured through a `Workflow`, not hard-coded.

### Lossless format: operations, not diffs

`tasq_core::format` has two layers. `Document` keeps the file byte for byte (the frontmatter block with its unknown keys and key order, line endings, unknown sections, blank lines); `Task` is a read-only typed projection of it. Edits are expressed as `format::ops::*` functions on the `Document`: a scalar op sets, adds (in `key::ORDER` among the keys present) or clears one frontmatter line, and the list-section ops are one per awk pass of the original script, because its insertion rules are **not uniform** (new sections go before `## Progress`, `### Merge requests` nests inside `## Related`). The store applies ops, re-parses, and returns `StoreError::Unsupported` if the requested `Task` still differs, rather than silently dropping a change.

### Store semantics

A store is `<data_dir>/<store.name>` (default `$XDG_DATA_HOME/tasq/stores/default`, else `~/.local/share/tasq/stores/default`); an absent directory is an empty store and the first write creates it. There is no index, no cache and no git: `TasqStore` reads every `.md` file's frontmatter on each call to map ids to files. A task's id is the `id:` key, written once (highest plus one on create) and never changed. Two files with one id make it ambiguous (`StoreError::AmbiguousId`, naming both).

Reads never lock and never spawn a process. A write (`create`, `update`, `set_done`) takes `.tasq/lock` (exclusive create, stale after two minutes against the injected clock, polled) for its whole duration. `create` mints a ULID from the clock and an injected random source and writes no-clobber, retrying on a taken name. `Store::update` takes the whole task, diffs it against disk (`tasq_store::diff`), applies ops and writes atomically (`NamedTempFile::persist`). A `Revision` (mtime + length + hash) remembered on read makes a later `update` fail with `Conflict` if the file changed. `tasq store lock -- <cmd>` runs a command holding the same lock (refreshed every 30 s), for plugins that rewrite task files. Commits and sync are the `tasq-git` plugin (`crates/git`, ADR 0030, `docs/git-sync.md`): enabled, it commits each command's writes in a detached job and syncs the store with its remote, taking the store lock only for the rebase.

### Injected everything

Time comes from a `Clock` trait, config loading from `LoadOptions` (cwd, home, env), and every child process (`git`, worktree hooks, launchers, plugin events) runs with `env_clear()` plus an injected environment (`PATH`, `HOME`, ...), so tests pick the binaries and git identity. This is what makes mutation testing affordable: logic is cheap to run thousands of times against temp dirs and fake processes. Keep new code in that shape.

### Config

Layered TOML (`tasq_core::config`): defaults, global `~/.config/tasq/config.toml`, nearest `.tasq.toml` walking up to home, selected `[profile.<name>]`, `TASQ_*` env, `--set`. Layers are deep-merged as TOML tables and deserialised once; each file is also checked alone with `deny_unknown_fields` so errors carry file:line:column. `Loaded::file_for("store.name")` gives the provenance used in error messages and `tasq store info`. Full reference in `docs/config.md`.

## Testing the store

`crates/store/tests/support/mod.rs` gives a `TempStore`: a temp root whose store directory is absent or empty, opened with a fixed clock and, when a test needs exact file names, a scripted random source. Nothing runs `git`. The CLI harness (`crates/cli/tests/support/mod.rs`) copies the committed fixture store `crates/store/tests/fixtures/stores/default` (ids 1, 3, 4, 6, 7) under an isolated `HOME` with `TZ=UTC`; tests place tasks with `write_task(ulid_name(..), task_text(id, front, body))` and read them back by id. The fixture store is maintained by hand, like any other fixture.

Markdown format fixtures live in `crates/core/tests/fixtures/*.md`, the op before/after pairs in `crates/core/tests/fixtures/ops/`; round trips must be byte-identical. See `docs/testing.md`.

## Agent skills

### Issue tracker

Issues are tasq tasks in the configured tasq store, managed with the `tasq` CLI; larger ones get a plan under `ruli/features/<slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five roles, applied as tasq topic tags (`#needs-triage`, ...). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` at the root (not written yet) plus `docs/adr/`. See `docs/agents/domain.md`.
