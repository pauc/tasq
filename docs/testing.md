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

## Mutation testing

We use [cargo-mutants](https://mutants.rs/) to measure test quality rather than line coverage. It
rewrites each function (returns a default, flips a condition, drops a statement) and expects at
least one test to fail. Install it with `cargo install cargo-mutants --locked`.

- `just mutants` tests only code changed since `main`. Run it before opening a PR. CI fails if a
  mutant in the diff survives.
- `just mutants-full` tests every mutant in `tasq-core`, `tasq-store-nb`, `tasq-sources` and
  `tasq-launch`. CI runs it nightly on `main` and keeps the issue "Surviving mutants (nightly)"
  up to date.
- `cargo mutants -f crates/core/src/format` checks a single module, which is how the per-task
  acceptance criteria in the plan are verified.

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
