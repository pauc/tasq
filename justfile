# Common development commands for tasq. Run `just` to list them.

default:
    @just --list

# Run every quality gate: formatting, clippy (warnings are errors) and tests.
check: fmt-check clippy test

# Format the whole workspace in place.
fmt:
    cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    cargo fmt --all --check

# Lint every target with warnings denied.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run the test suite.
test:
    cargo test --workspace

# Build API docs for the workspace crates.
doc:
    cargo doc --workspace --no-deps

# Mutation testing on code changed since `main` (what a PR would be judged on).
# `--jobs` is left to cargo-mutants' auto-detection. The diff goes through a
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
    cargo mutants --in-diff "$diff_file"

# Mutation testing on the whole workspace (nightly / phase review).
mutants-full:
    cargo mutants --workspace
