# Testing

Quality gates run locally with `just check` (or the cargo commands it wraps) and in CI
(`.github/workflows/ci.yml`): `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
`cargo deny`, an MSRV build, line coverage with `cargo-llvm-cov`, and mutation testing.

## Conventions

- Logic lives in pure functions with injected `Clock`, filesystem views, environment and fake
  processes, so it is cheap to run thousands of times. That is what makes mutation testing
  affordable.
- Fixtures for the markdown format live under `crates/core/tests/fixtures/v2/*.md` (format v2,
  `format_v2.rs`, `format_v2_write.rs`) and `crates/core/tests/fixtures/*.md` (format v1, read
  by `tasq migrate` only, `format_fixtures.rs`); round trips must be byte-identical. Property
  tests use `proptest` (`format_props.rs`, one arm per format).
- Snapshot tests use `insta`. Accept changes with `INSTA_UPDATE=always scripts/guard cargo test
  -p <crate>` (no `cargo insta` needed), then review the snapshot diff.
- The CLI is tested end to end with `assert_cmd` (`crates/cli/tests/cli.rs`) against the
  harness below.
- Tests never touch the network. HTTP in `tasq-sources` goes through the
  `tasq_sources::http::Transport` trait: `UreqTransport` is the real one (the only
  `#[mutants::skip]` in that module), unit tests use `ScriptedTransport`, a queue of canned
  responses that records every request's URL and headers (`crates/sources/src/http.rs`), and
  the CLI end-to-end sync test uses `FakeHttp`, a loopback `TcpListener` server with a route
  table (`crates/cli/tests/support/mod.rs`) reached through `forge.<name>.url`. No `wiremock`
  and no tokio in the dependency tree.

## The v2 fixture store

`crates/store/tests/fixtures/stores/default` is the nb fixture notebook converted by
`tasq_store::migrate`: one `<ULID>.md` per task (ids 1, 3, 4, 6, 7, the nb ids), laid out as
`<data_dir>/<store.name>` with the default name, for the CLI tests to copy. Each file name is a
ULID of the nb file's second read as UTC (what `migrate` writes with `TZ=UTC`) with the random
part counting up from 1, so the committed names do not depend on the zone of the machine that
made them; `created_at` read back is that instant in local time.
`the_committed_fixture_store_is_the_migrated_nb_fixture` (`crates/store/tests/migrate.rs`) fails when the fixture and the
migration of the notebook disagree; after a deliberate change to either, regenerate it with
`TASQ_UPDATE_FIXTURES=1 scripts/guard cargo test -p tasq-store --test migrate` and review the
diff. Hand edits to the fixture are overwritten by that run.

## Testing the store

The tasq store (`crates/store`) is tested through the `Store` trait over a temporary directory,
with no environment to build: `crates/store/tests/support/mod.rs` gives a `TempStore`, a temp
root whose store directory is absent (`TempStore::absent()`, an empty store that a write must
create) or empty (`TempStore::empty()`). `open()` opens a `TasqStore` on it with the default
workflow and a fixed clock; `open_with(clock, random)` also scripts the random part of minted
ULIDs, so a created file's name is an exact-string assertion and a name collision can be forced
by returning the same value twice. `write(name, text)` places a task file by hand, `entries(dir)`
lists a directory. Nothing in these tests runs a process: the store never runs `nb` or `git`.
Creation times are compared through the same local-time conversion the store uses
(`local_millis`), so the tests hold in any `TZ`. The lock tests set the lock file's mtime
relative to the fixed clock, which is what makes stale and live locks deterministic.

## The nb crate (migrate only)

`crates/store-nb` and the v1 reader stay only until the follow-up task that deletes them;
nothing in the CLI opens an nb notebook, and `tasq migrate` reads the notebook through
`tasq-core` alone (`tasq_store::migrate`). Its tests still build an `NbEnv`
(`crates/store-nb/tests/support/mod.rs`): a temporary copy of
`crates/store-nb/tests/fixtures/nb/home` with `NB_DIR`, `NBRC_PATH`, an isolated `HOME` and
`NB_AUTO_SYNC=0`, so the real `~/.nb` is never read or written. `crates/store-nb/tests/migrate.rs`
checks that a migrated notebook reads back through the tasq store equal, field by field, to the
nb store's tasks. The tests that run the real `nb` start with `nb_or_skip("<test name>")`: they
pass vacuously when `nb` is not on `PATH`, unless `TASQ_REQUIRE_NB=1` is set (CI sets it).

The CLI harness (`crates/cli/tests/support/mod.rs`, `TestEnv`) copies the v2 fixture store
(above) to `<home>/.local/share/tasq/stores/default`, the default store under an isolated
`HOME`, sets `PATH` to a directory holding only a `git` symlink and `TZ=UTC` (minted file
names are ULIDs of the clock's time), so the binary never sees the real store, the real config
or the machine's zone. `TestEnv::empty()` has no store directory at all, which is an empty
store. Tests place a task by hand with `write_task(ulid_name(when, n), v2(id, front, body))`
and read one back by id (`read_task(id)`, `file_of(id)`: the store names files, not the test).
`fake_tool(name, body)` installs a script on that `PATH` to stand in for `claude`, `glow`,
`tmux` or `herdr`; fakes must use absolute paths for anything that is not a shell builtin.
Snapshots normalise the temp root to `[ROOT]`.

## Claude Code plugin

`crates/cli/tests/plugin.rs` checks the layout of `plugins/claude` (manifest name `tasq`, one
`SKILL.md` per skill with matching `name` and a `description`, no mention of task files,
`.index` or a notebook: every write goes through the CLI) and that
the launch prompt template only names skills the plugin ships. The authoritative check needs
Claude Code and is not in CI: `claude plugin validate --strict plugins/claude` (and the same for
`.claude-plugin/marketplace.json` and `plugins/claude/skills`).

## Terminal UI

`tasq-tui` is Elm-shaped so that almost all of it is testable without a terminal: `update()`
is a pure function tested with message sequences (`crates/tui/src/update.rs`), key bindings are
a pure table (`keys.rs`), and `dispatch()` runs commands against `tasq_core::store::MemoryStore`
and a `RecordingHost` (`runtime.rs`). Rendering is pinned by `crates/tui/tests/render.rs`,
which draws models onto `ratatui::backend::TestBackend` at both layouts (120 and 80 columns)
and in every mode, and snapshots the screen with `insta` (`INSTA_UPDATE=always cargo test -p
tasq-tui` accepts changes); one test asserts cell styles for the theme and for `NO_COLOR`. The
only untested code is the event loop and the raw-mode/alternate-screen switching, marked
`#[mutants::skip]`. To check the terminal path by hand without a real session:

```sh
printf 'j?qq' | script -qec "stty cols 120 rows 30; target/debug/tasq ui" /dev/null
```

(`script` gives the program a pseudo-terminal; without `stty` its size is 0x0 and nothing is
drawn.)

The README gif is a [VHS](https://github.com/charmbracelet/vhs) tape, `docs/demo/demo.tape`,
recorded against a throwaway store that `docs/demo/setup.sh` builds with `tasq create`
under a fixed `TASQ_NOW`, so re-rendering gives the same frames. `scripts/demo-gif` (also
`just demo`) builds the debug binary and runs the tape with the pinned VHS docker image,
mounting the repository and the binary; nothing else needs to be installed. The container has
no `less` or `claude`, so the setup exports `TASQ_PAGER=cat` and `TASQ_SUMMARIZER=raw`.
Re-render whenever the list or TUI output changes visibly and commit the new
`docs/demo/tasq.gif`.

The TUI crate is excluded from the mutants config like the CLI, but its pure modules are
checked per task with `cargo mutants --no-config -f crates/tui/src/update.rs ...`.

## Deterministic time

`TASQ_NOW="YYYY-MM-DD HH:MM"` in the environment makes the binary use a fixed clock for every
timestamp it writes (progress notes, sessions, new file names). The CLI integration tests set it
so file contents can be compared exactly; it is also handy for reproducible demos.

## Mutation testing

We use [cargo-mutants](https://mutants.rs/) to measure test quality rather than line coverage. It
rewrites each function (returns a default, flips a condition, drops a statement) and expects at
least one test to fail. Install it with `cargo install cargo-mutants --locked`.

- `just mutants` tests only code changed since `main`. Run it before opening a PR. CI fails if a
  mutant in the diff survives.
- `just mutants-full` tests every mutant in `tasq-core`, `tasq-store`, `tasq-store-nb`,
  `tasq-sources`, `tasq-launch` and `tasq-worktree`. CI runs it nightly on `main` and keeps the
  issue "Surviving mutants (nightly)" up to date.
- `just mutants-in crates/core/src/format` checks a single module, which is how the per-task
  acceptance criteria in the plan are verified. It passes `--no-config` because cargo-mutants
  27.1 unions `-f` with the config's `examine_globs`, so with the config active `-f` never narrows
  the scope. Check with `cargo mutants --list ...` before trusting any scope.

The TUI and CLI crates are excluded in `.cargo/mutants.toml`; they are covered by snapshot tests
instead. `impl Debug`, `impl Display` and `impl Default` are excluded as noise.

Results go to `mutants.out/` (git-ignored). `mutants.out/missed.txt` lists survivors. At the end
of every phase, review it and either add a test or, for a function that only wraps I/O or `exec`,
mark it `#[mutants::skip]` with a one-line reason above the attribute. Never use the skip to
silence a logic function.

The `mutants` crate that provides the attribute is a regular dependency of `tasq-core`, because
the attribute sits on non-test code. Do not wrap it in `cfg_attr(test, ...)`: cargo-mutants reads
the attribute from source text and never evaluates the condition.

Targets: zero missed mutants in `tasq-core::{format, query, reconcile, config, dates}`, at most 5%
missed across `tasq-core`, full run under 20 minutes in CI.

## Memory safety when building

Every recipe in the `justfile` runs cargo through `scripts/guard`, a transient systemd user scope
with a hard memory ceiling and no swap (12G by default; `GUARD_MEM=8G just test` or
`scripts/guard --mem 8G cargo test` to lower it). If a build or test run exceeds it, only the
processes inside the scope are killed. `.cargo/config.toml` also caps parallel `rustc` jobs at
eight, and dev profiles carry reduced debuginfo.

Why: on a 32-core machine a cold build with the default job count, run three times concurrently
alongside cargo-mutants, exhausted 62 GB of RAM and killed the desktop session. On Linux, cargo
also runs every test binary through `scripts/test-runner`, which caps the process address space
at 4 GiB (`TASQ_TEST_AS_KB` overrides). A test, or a mutant, that allocates without bound then
aborts on its own and is counted as a failure, instead of outrunning cargo-mutants' timeout.
The runner also sets `MALLOC_ARENA_MAX=2`: the cap is on address space, and glibc reserves a
64 MiB malloc arena per thread that allocates (up to eight per core), so a test binary with one
thread per core, each spawning a child with two reader threads, reserved several GiB of arenas
and then failed to create the next thread (`failed to spawn thread`, `failed to allocate an
alternative stack: Cannot allocate memory`). That was the cause of random failures across the
CLI integration tests on a 32-core machine, two or three tests per full run.
Rules of thumb:

- Run cargo through the guard (`scripts/guard cargo ...`) whenever you are not using `just`.
- Never run more than one cargo-mutants at a time, and keep `--jobs 2` (each job builds a full copy of the tree).
- Do not start several agents or shells that build the same workspace concurrently.
