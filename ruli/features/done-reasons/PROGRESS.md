# Progress: one switch per reason sync marks a task done

Plan: [PLAN.md](PLAN.md). Task: tasq 61.

## Status

| Sub-task | Title | Status | Commits |
|----------|-------|--------|---------|
| SD-001 | `DoneReason` and the policy in core | done | 3801dc0 |
| SD-002 | sources report reasons | done | 1ea7da5 |
| SD-003 | config keys and the per-kind check | done | 59d536f |
| SD-004 | docs, ADR, changelog | done | 73e49a7 |
| SD-005 | the user's `sf` profile | todo | |

## Learnings

- SD-001 (3801dc0): mutants `crates/core/src/source.rs` 37 tested, 21 caught, 16 unviable,
  0 missed; `crates/sources/src/{registry,work_items,review_requests,llm_bridge}.rs` 83 tested,
  64 caught, 19 unviable, 0 missed.
- The `-E 'impl Default'` exclude in the mutants recipe skips `DoneWhen::default()`, so the
  all-`true` default is pinned by an exact `assert_eq!` against a literal `DoneWhen` instead.
- `clippy::struct_excessive_bools` fires on `DoneWhen` (four bools); it carries the same
  `#[allow(..., reason = ...)]` form as `SourceConfig`.
- `changes_for_known` no longer matches on the state: a `Done(reason)` that `Policy::closes`
  returns early with the `Close`; everything else (open, needs attention, a done reason that is
  off) falls through to the flag and note rules. That is what makes "an off reason is treated
  as open" one code path instead of a special case.
- SD-003 (59d536f): mutants `crates/core/src/config/{mod,load}.rs` 181 tested, 160 caught,
  21 unviable, 0 missed; `crates/sources/src/registry.rs` 8 tested, 4 caught, 4 unviable,
  0 missed.
- The `SourceKeys` pass sees each `[[source]]` as `BTreeMap<Spanned<String>, Value>`, so a
  source with two offending keys reports the first one in file order (sorted by span start),
  not the alphabetically first. The wrong-kind check only fires on keys `SourceKind::is_key`
  knows, so a misspelt key still gets `FileSchema`'s "unknown field ..., expected one of" list.
- The `close_when_done` error does not need a `kind` or `name`: it fires on the key alone, so a
  half-written source still gets the migration message instead of "unknown field".
- Clippy `result_large_err` fires on a test closure returning `Result<Loaded, ConfigError>`;
  the closure returns the error text instead.

- SD-002 (1ea7da5): mutants `crates/sources/src/{registry,work_items,review_requests,llm_bridge}.rs`
  83 tested, 64 caught, 19 unviable, 0 missed.
- cargo-mutants does not mutate struct field initialisers, so `policy_for` wiring two
  `done_when_*` keys crossed survived every earlier test and every mutant run. The per-key
  registry test sets one key `false` on a kind that uses it and asserts the whole `DoneWhen`
  against a literal with only that switch off; crossing `merged` and `approved` by hand fails it.
- Precedence is now pinned: a closed issue no longer assigned to me is `Closed`, not
  `Unassigned`; a merged or closed MR I approved is `Merged` or `Closed`, not `Approved`. An
  issue with no assignees is `Unassigned`. Each was checked by breaking the match by hand.
- The registry test config needs a `[forge.f]` with a `kind` matching the source kind, or load
  fails with `BadForgeReference` / `ForgeKindRequired` before `policy_for` is reached.

- SD-004 (73e49a7): docs only, no code touched, so no mutants run: 0 missed.
- Every snippet written was loaded with `tasq --config-dir <dir> config show --json`: the
  `docs/sources.md` work-items example (its `done_when_unassigned = false` reaches the config),
  and the `docs/config.md` example block uncommented once as review requests and once as work
  items, each without the other kinds' keys. The three error texts quoted in the docs are the
  ones the binary prints. An exported `TASQ_PROFILE` makes `--config-dir` on a scratch dir fail
  with "profile ... is not defined"; run the check with `env -u TASQ_PROFILE`.
- The review of the draft caught claims the new keys made conditional: `exclude_authors`'
  "still closes when its MR merges" (now: with `done_when_merged`), step 5's "gone" as a third
  state (now a done reason), and the example block, whose mixed keys were harmless before and
  are errors now on one kind (it now says so in its first line).

## Blockers

## Deviations

- SD-004: `cargo doc --workspace --no-deps` is not warning-free, but all 9 warnings are on
  `main` already, in files this branch does not touch (`crates/cli/src/cli.rs`,
  `crates/core/src/edit.rs`, `crates/store/src/lock.rs`, `crates/tui/src/view.rs`); SD-004 adds
  none. Left alone to keep the sub-task docs-only.
- SD-004 also adds a "From an earlier `tasq`: done reasons" table to `docs/migration.md`, not in
  the plan: the CHANGELOG points upgraders there for the ADR 0031 moves.
- SD-004: the `LlmBridge::check` doc comment ("nothing is ever closed by it") was left as is. It
  describes `check`, which is accurate; a fetched item whose `state` is `done` closes through
  `reconcile`, which `docs/sources.md` now says.

- SD-001 touches `crates/sources` as well: `ItemState::Done` becoming a tuple variant does not
  compile there, so the forge sources already report their correct reasons and
  `registry::policy_for` maps `cfg.close_when_done` onto all four `DoneWhen` switches. The
  existing sources tests only got the reasons they needed to compile; the reason assertions
  and the per-key registry tests stay in SD-002.
- SD-003: no committed fixture, example or test config set `close_when_done` or a key of
  another kind, so none needed updating; `examples/` and `plan-4.6.toml` load unchanged. The
  doc comments cite ADR 0032, which SD-004 writes.
- SD-003: the wrong-kind check also skips a source without a `name` that is a string, not
  only one without a parseable `kind` as the plan says: the message names the source
  (`source "mrs": ...`), so without a name there is nothing to print. `FileSchema` reports
  "missing field `name`" first and the wrong-kind error on the next load. Stated in ADR 0032
  and the `check_source_keys` doc comment, tested in `config_load.rs`.
- SD-002: no code change. SD-001 already made the sources report the right reasons and SD-003
  already mapped the four keys in `policy_for`, so every new test passed on arrival instead of
  going red first. Each was proven by a hand-made breakage instead (see Learnings).

## Review

Two-axis review of `main...HEAD` (2026-10-28): Standards against `CLAUDE.md` and
`docs/testing.md`, Spec against PLAN.md SD-001..SD-004. No documented-standard violation and no
missing, extra or wrong requirement; six findings, two fixed.

| # | Axis | Finding | Outcome |
|---|------|---------|---------|
| 1 | Standards | Duplicated code (test-only): `state_wire_format_carries_no_reason` re-listed the six `DoneReason` variants although `REASONS` sits in the same test module | fixed (275321b) |
| 2 | Standards | Duplicated code: both forge sources pair `Done(Gone)` with `"no longer exists"` and `Done(Closed)` with `"closed"`; a `DoneReason::note()` would collapse the pair | skipped: the plan says "the note stays as is, no mapping from reason to text" |
| 3 | Standards | Feature envy (light): `Policy::closes` reads only `self.done_when` | skipped: ADR 0032 names `Policy::closes` as the decision point |
| 4 | Standards | Shotgun surgery: a new `SourceConfig` field touches the struct, `SourceKind::keys`, the guard test and `docs/sources.md` | skipped: the guard test is the agreed mitigation; the table has to live somewhere |
| 5 | Standards | `3801dc0 feat(core)` also rewrites four `crates/sources` files | skipped: already recorded under Deviations (SD-001) |
| 6 | Spec | The wrong-kind check also skips a source without a string `name`; the plan only lists a missing or unparseable `kind`, and PROGRESS.md did not record it | fixed (93c4c3a) |

Gate after the fixes: fmt, clippy `-D warnings`, workspace tests green; `cargo mutants --in-diff`
38 tested, 27 caught, 11 unviable, 0 missed.

Seen on the way, out of scope: `tasq_cli::commands::ui::tests::sync_passes_the_picked_sources_as_source_flags`
failed once in a full `cargo test --workspace` run and passed alone and on the rerun. It writes a
`#!/bin/sh` script to a temp dir and executes it at once while sibling tests fork, so an
`ETXTBSY` race is the likely cause. `crates/cli` is untouched by this branch.
