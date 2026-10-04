# Common development commands for tasq. Run `just` to list them.
#
# Every cargo invocation goes through `scripts/guard`, which runs it in a
# memory-limited systemd scope (16G by default, GUARD_MEM to override). A
# runaway build then fails on its own instead of exhausting the machine.
# `.cargo/config.toml` additionally caps parallel rustc jobs.

guard := "scripts/guard"

default:
    @just --list

# Run every quality gate: formatting, clippy (warnings are errors) and tests.
check: fmt-check clippy test

# Format the whole workspace in place.
fmt:
    {{guard}} cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    {{guard}} cargo fmt --all --check

# Lint every target with warnings denied.
clippy:
    {{guard}} cargo clippy --workspace --all-targets -- -D warnings

# Run the test suite.
test:
    {{guard}} cargo test --workspace

# Build API docs for the workspace crates.
doc:
    {{guard}} cargo doc --workspace --no-deps

# Mutation testing on code changed since `main` (what a PR would be judged on).
# `--jobs 2` bounds concurrent mutant builds (each is a full build of a copy). The diff goes through a
# temp file because `just` runs recipes with `sh`, which lacks `<(...)`.
mutants:
    #!/usr/bin/env sh
    set -eu
    diff_file="$(mktemp)"
    trap 'rm -f "$diff_file"' EXIT
    git diff main...HEAD > "$diff_file"
    if [ ! -s "$diff_file" ]; then
        echo "no changes relative to main; run 'just mutants-full' for everything"
        exit 0
    fi
    scripts/guard cargo mutants --jobs 2 --in-diff "$diff_file"

# Mutation testing on the whole workspace (nightly / phase review).
mutants-full:
    {{guard}} cargo mutants --jobs 2 --workspace
