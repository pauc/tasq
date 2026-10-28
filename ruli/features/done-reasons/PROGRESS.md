# Progress: one switch per reason sync marks a task done

Plan: [PLAN.md](PLAN.md). Task: tasq 61.

## Status

| Sub-task | Title | Status | Commits |
|----------|-------|--------|---------|
| SD-001 | `DoneReason` and the policy in core | done | 3801dc0 |
| SD-002 | sources report reasons | done | 1ea7da5 |
| SD-003 | config keys and the per-kind check | done | 59d536f |
| SD-004 | docs, ADR, changelog | todo | |
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

## Blockers

## Deviations

- SD-001 touches `crates/sources` as well: `ItemState::Done` becoming a tuple variant does not
  compile there, so the forge sources already report their correct reasons and
  `registry::policy_for` maps `cfg.close_when_done` onto all four `DoneWhen` switches. The
  existing sources tests only got the reasons they needed to compile; the reason assertions
  and the per-key registry tests stay in SD-002.
- SD-003: no committed fixture, example or test config set `close_when_done` or a key of
  another kind, so none needed updating; `examples/` and `plan-4.6.toml` load unchanged. The
  doc comments cite ADR 0032, which SD-004 writes.
- SD-002: no code change. SD-001 already made the sources report the right reasons and SD-003
  already mapped the four keys in `policy_for`, so every new test passed on arrival instead of
  going red first. Each was proven by a hand-made breakage instead (see Learnings).
