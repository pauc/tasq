# Progress: manual rank within a status group

Plan: [PLAN.md](PLAN.md). Task: tasq 43.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RK-001 | `Store::update_many` (prefactor) | done | 2026-10-17; `cargo mutants --in-diff`: 11 mutants, 10 caught, 1 unviable, 0 missed; commit: ec39107 |
| RK-002 | Rank in the file orders lists and `next` | done | 2026-10-17; `cargo mutants --in-diff`: 19 mutants, 15 caught, 4 unviable, 0 missed; commit: b0573c9 |
| RK-003 | `tasq move --top` / `--unrank` | ready | blocked by: none (RK-002 done) |
| RK-004 | `tasq move --before` / `--after` | blocked | blocked by: RK-003 (RK-001 done) |
| RK-005 | Renumber a group when the gap runs out | blocked | blocked by: RK-004 |
| RK-006 | `ctrl+k` / `ctrl+j` in the TUI | blocked | blocked by: RK-004 |

## Learnings

- 2026-10-17: design settled by grilling; ADR 0025 and `CONTEXT.md` committed (d621eae).
- 2026-10-17 (RK-001): the batch error is its own type, `BatchError { written, error }`, not a new
  `StoreError` variant: wrapping would have forced every `match` on `Conflict` in the CLI to look
  inside, and `update_many` callers are the only ones who care about partial progress. The nb
  override still checkpoints the files it wrote before a mid-batch failure, so they never sit
  uncommitted until the next unrelated write. The five other `Store` impls (memory, two core test
  doubles, two TUI test doubles) take the default body and needed no change.

- 2026-10-17 (RK-002): the rank is one more scalar section in the existing shape (`set_rank` /
  `clear_rank` mirror `set_closed` / `clear_closed`, the diff's `apply_scalars` groups the four
  one-value sections to stay under clippy's 100-line limit) and one more component at the front
  of `query::SortKey` (`RankKey::At(n) < RankKey::Unranked`). Nothing in `list`, the TUI or
  `next` changed: they all go through `sort_key`. The first store-diff test failed with `Ok` and
  an unchanged file, not with an error: until `differing_fields` knows a field, `update` drops
  a change to it silently, so a new `Task` field is not done until it is in that table.
  `tasq apply` now reports which optional keys the document carries (`Keys { worktrees, rank }`)
  instead of one bool, since both follow the "missing means unchanged" rule.

## Blockers

## Deviations from the plan
