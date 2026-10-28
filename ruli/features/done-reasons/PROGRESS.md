# Progress: one switch per reason sync marks a task done

Plan: [PLAN.md](PLAN.md). Task: tasq 61.

## Status

| Sub-task | Title | Status | Commits |
|----------|-------|--------|---------|
| SD-001 | `DoneReason` and the policy in core | done | 3801dc0 |
| SD-002 | sources report reasons | todo | |
| SD-003 | config keys and the per-kind check | todo | |
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

## Blockers

## Deviations

- SD-001 touches `crates/sources` as well: `ItemState::Done` becoming a tuple variant does not
  compile there, so the forge sources already report their correct reasons and
  `registry::policy_for` maps `cfg.close_when_done` onto all four `DoneWhen` switches. The
  existing sources tests only got the reasons they needed to compile; the reason assertions
  and the per-key registry tests stay in SD-002.
