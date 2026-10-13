Implement tasq task 31 on this branch (feat/field-commands, from main).

Spec: `tasq view 31 --no-pager`. The description has the commands and examples; the progress
log has the decisions, and a later entry overrides an earlier one (22:49 replaces the
trailing-note part of 22:43). Read CLAUDE.md, docs/file-format.md and docs/testing.md first.

Scope: CLI commands plus small tasq_core::edit functions. The format ops (set_title, set_due,
set_description, set_tags) and the store diff (crates/store-nb/src/diff.rs) already handle
these fields; do not change the format layer or the store unless a test proves a gap. Each
command is: parse args -> tasq_core::edit function -> store.update -> print, following
`tasq set` in crates/cli/src/commands/edit.rs.

Slices, one commit each, in this order:
1. tasq rename ID TITLE [--note TEXT]
2. tasq due ID <date|none> [--note TEXT]: same date parser as create --due (ADR 0019)
3. tasq tag ID TAG... [--note TEXT]: +a adds, -b removes, a bare name adds; status and
   priority words are rejected with a hint to use tasq set; only the named tags change
4. tasq desc ID [TEXT|-] [--note TEXT]: TEXT sets, - reads stdin to EOF and drops one
   trailing newline, no TEXT prints the description (nothing when none), "" clears
5. tasq desc ID --edit: $VISUAL then $EDITOR on a temp file holding the description; zero
   exit saves (one trailing newline dropped, empty clears), non-zero exit or unchanged file
   writes nothing; neither variable set is a user error naming both
6. --note on set, done and reopen as an alias of their positional note; passing both is a
   usage error (exit 2). The positional stays (tasks-script parity, wrapup skill).

Output: "[31] renamed", "[31] due 2026-10-17" / "[31] due cleared",
"[31] tags: +needs-info -needs-triage", "[31] description updated" / "[31] description cleared".
Every command takes --json and prints {"schema": 1, "task": ...} as stored after the change.
A changed task on disk between read and write must surface the store's Conflict, not retry.

Docs in the same slice as the code: the clap help text with one example per command, the
command table in docs/json.md, and docs/migration.md (new commands; --note on set/done/reopen
in slice 6). Add the new commands to the `tasq --help` examples block.

Rules (CLAUDE.md):
- Every cargo call through scripts/guard. One build at a time: no parallel agents or shells
  that compile.
- Test-first with /tdd at the tasq_core::edit seam and at the CLI integration tests
  (crates/cli/tests); never touch the real notebook, use the test support temp notebooks.
- Per slice: fmt --check, clippy --workspace --all-targets -D warnings, the crate's tests;
  full `scripts/guard cargo test --workspace` at the end.
- Zero missed mutants in the modules you touch, using the --no-config mutants-in recipe from
  the justfile with --jobs 2. Fix survivors with exact-string assertions or a better code
  shape; only the function that spawns the editor may get #[mutants::skip], with a one-line
  reason above it.
- Conventional commits scoped by crate: feat(core): ..., feat(cli): ..., docs(cli): ...
- Never `cargo install` from this branch: the installed tasq comes from feat/worktrees and
  the user's config needs it. Run the built target/debug/tasq against a temp notebook only.

After each commit: `tasq log 31 "<slice> done (<sha>)"`. At the end, run /code-review against
main, fix what it finds, then log a summary on task 31 (commits, test count, mutants result).
Do not open the PR or mark the task done; leave that to me.
