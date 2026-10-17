# Progress: manual rank within a status group

Plan: [PLAN.md](PLAN.md). Task: tasq 43.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RK-001 | `Store::update_many` (prefactor) | done | 2026-10-17; `cargo mutants --in-diff`: 11 mutants, 10 caught, 1 unviable, 0 missed; commit: ec39107 |
| RK-002 | Rank in the file orders lists and `next` | done | 2026-10-17; `cargo mutants --in-diff`: 20 mutants, 16 caught, 4 unviable, 0 missed; commits: b0573c9, 7155e25 (review fix: done list ignores a kept rank) |
| RK-003 | `tasq move --top` / `--unrank` | done | 2026-10-17; `cargo mutants --no-config -f crates/core/src/edit.rs`: 82 mutants, 63 caught, 19 unviable, 0 missed; `--no-config -f crates/cli/src/commands/rank.rs -f crates/cli/src/commands/mod.rs`: 10 mutants, 9 caught, 1 unviable, 0 missed (the config excludes `crates/cli/**`, so the CLI bar needs this one-off run); commits: 3710eac, 1fba137 (review fixes) |
| RK-004 | `tasq move --before` / `--after` | ready | blocked by: none (RK-001, RK-003 done) |
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

- 2026-10-17 (RK-003): `move_task` decides "already there" by sorting the group with
  `query::sort` and checking whether the task comes first, not by comparing ranks: a task that
  is first by the natural order (nothing ranked) and a task that is first by rank are both
  no-ops, and the written rank is computed from the *other* tasks' lowest rank, so a ranked task
  that is not first moves above the current top instead of below itself. The fixture notebook has
  one task per status group, so every CLI move test adds a second in-progress task first
  (`store reconcile` gives it id 8). `tasq move` has no `--note`: a rank is a sort hint, not
  something worth a progress line. `BatchError` became an `EditError` variant now rather than
  in RK-004, since `move_task` already writes through `update_many`. Review follow-up: the
  clap errors (no target, two targets) are insta snapshots, and a `--top` on a hand-written,
  invalid `## Rank` is covered: the move replaces the section's body (RK-002's `set_rank`), so
  "the next move overwrites them" (ADR 0025) holds for `--top`. `--unrank` on such a task is a
  no-op that keeps the bytes: the model reads it as unranked (`None`), the diff sees no change,
  and removing the section would need the store to know a section exists for a value the model
  does not have. That is US36's "kept as written"; a fix belongs with the format, not here.

## Blockers

## Deviations from the plan
