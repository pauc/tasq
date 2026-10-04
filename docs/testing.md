# Testing

Quality gates run locally with `just check` (or the cargo commands it wraps) and in CI
(`.github/workflows/ci.yml`): `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
`cargo deny`, an MSRV build, line coverage with `cargo-llvm-cov`, and mutation testing.

## Conventions

- Logic lives in pure functions with injected `Clock`, filesystem views and fake processes, so it
  is cheap to run thousands of times. That is what makes mutation testing affordable.
- Fixtures for the markdown format live under `crates/core/tests/fixtures/`.
- Snapshot tests use `insta`; property tests use `proptest`; HTTP adapters use `wiremock`; the CLI
  uses `assert_cmd`. Tests never touch the network.

## Claude Code plugin

`crates/cli/tests/plugin.rs` checks the layout of `plugins/claude` (manifest name `tasq`, one
`SKILL.md` per skill with matching `name` and a `description`, no direct notebook edits) and that
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
drawn.) The TUI crate is excluded from the mutants config like the CLI, but its pure modules are
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
- `just mutants-full` tests every mutant in `tasq-core`, `tasq-store-nb`, `tasq-sources` and
  `tasq-launch`. CI runs it nightly on `main` and keeps the issue "Surviving mutants (nightly)"
  up to date.
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
with a hard memory ceiling (16G by default, `GUARD_MEM=8G just test` to lower it). If a build or
test run exceeds it, only the processes inside the scope are killed. `.cargo/config.toml` also caps
parallel `rustc` jobs at eight, and dev profiles carry reduced debuginfo.

Why: on a 32-core machine a cold build with the default job count, run three times concurrently
alongside cargo-mutants, exhausted 62 GB of RAM and killed the desktop session. On Linux, cargo also runs every test binary through `scripts/test-runner`, which caps the
process address space at 4 GiB (`TASQ_TEST_AS_KB` overrides). A test, or a mutant, that
allocates without bound then aborts on its own and is counted as a failure, instead of
outrunning cargo-mutants' timeout. Rules of thumb:

- Run cargo through the guard (`scripts/guard cargo ...`) whenever you are not using `just`.
- Never run more than one cargo-mutants at a time, and keep `--jobs 2` (each job builds a full copy of the tree).
- Do not start several agents or shells that build the same workspace concurrently.
