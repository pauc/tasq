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

# Mutation testing on changed code (configured in T-004).
mutants:
    @echo "configured in T-004"

# Mutation testing on the whole workspace (configured in T-004).
mutants-full:
    @echo "configured in T-004"
