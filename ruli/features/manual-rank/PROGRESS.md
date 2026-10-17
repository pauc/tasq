# Progress: manual rank within a status group

Plan: [PLAN.md](PLAN.md). Task: tasq 43.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RK-001 | `Store::update_many` (prefactor) | done | 2026-10-17; `cargo mutants --in-diff`: 11 mutants, 10 caught, 1 unviable, 0 missed; commit: 364c9d5 |
| RK-002 | Rank in the file orders lists and `next` | done | 2026-10-17; `cargo mutants --in-diff`: 20 mutants, 16 caught, 4 unviable, 0 missed; commits: da947b4, 36816c2 (review fix: done list ignores a kept rank) |
| RK-003 | `tasq move --top` / `--unrank` | done | 2026-10-17; `cargo mutants --no-config -f crates/core/src/edit.rs`: 82 mutants, 63 caught, 19 unviable, 0 missed; `--no-config -f crates/cli/src/commands/rank.rs -f crates/cli/src/commands/mod.rs`: 10 mutants, 9 caught, 1 unviable, 0 missed (the config excludes `crates/cli/**`, so the CLI bar needs this one-off run); commits: 4c70461, 8faef11 (review fixes) |
| RK-004 | `tasq move --before` / `--after` | done | 2026-10-17; `cargo mutants --no-config -f crates/core/src/edit.rs`: 115 mutants, 91 caught, 24 unviable, 0 missed; `--no-config -f crates/cli/src/commands/rank.rs`: 11 mutants, 10 caught, 1 unviable, 0 missed; commits: 1c48375, c421e59 (review fixes) |
| RK-005 | Renumber a group when the gap runs out | done | 2026-10-17; `cargo mutants --no-config -f crates/core/src/edit.rs`: 125 mutants, 99 caught, 26 unviable, 0 missed; commits: 863d03c, 7d9e9a2 (review fixes) |
| RK-006 | `ctrl+k` / `ctrl+j` in the TUI | done | 2026-10-17; `cargo mutants --no-config --in-diff <(git diff main -- crates/tui)`: 27 mutants, 20 caught, 7 unviable, 0 missed (the config excludes `crates/tui/**`); after the review, `--no-config -f crates/core/src/edit.rs`: 128 mutants, 102 caught, 26 unviable, 0 missed and `-f crates/cli/src/commands/rank.rs`: 6 mutants, 5 caught, 1 unviable, 0 missed; commits: d558307, 03d1193 (review fixes) |

## Learnings

- 2026-10-17: design settled by grilling; ADR 0025 and `CONTEXT.md` committed (3665873).
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

- 2026-10-17 (RK-004): the planner works on positions, not ranks: it sorts the group with
  `query::sort`, takes the moved task out, finds the slot next to the anchor and reads the two
  neighbours. "Already there" is `slot == current position`, so an unranked task that already
  follows its unranked anchor is a no-op (US21) even though no rank could express that. The
  neighbour pair decides everything: `(none, _)` is `--top`, `(ranked, ranked)` is the midpoint
  (`i64::midpoint`, refused when it lands on a neighbour), `(ranked, unranked|none)` is one step
  below, `(unranked, _)` ranks every unranked task above the slot first, continuing from the
  highest rank above it. Since ranked tasks always sort first, `--before` the first unranked
  task needs no prefix: one step below the last ranked one is enough. One reachable corner:
  the moved task's own rank can stay the same while the prefix is written (ranked 2048 above
  unranked tasks, then `--after` the first of them: it gets 1024, the task keeps 2048), so
  `changed` follows the task's own rank and `ranked` the others, and the printed line counts
  the move as done when either is non-empty. The batch writes the prefix first and the moved
  task last: a failure midway leaves tasks ranked in the order they already had, never the
  moved task above a still-unranked prefix. `update_many` is called once per move (test
  double counting batches); the one-commit property is RK-001's store-nb test. A done anchor
  is refused as "task N is done but task M is <status>" through the same `OtherGroup` error,
  with `done` as the group label. Review follow-ups: the planner is pure (`checked_anchor` does the
  one store read, for the error label), a slot's two neighbours are a `Neighbour` enum so the
  four arms read without comments, and a store-nb test proves a prefix-ranking move is one
  commit (the acceptance line rested on RK-001's `update_many` test alone).

- 2026-10-17 (RK-005): the renumber is one more arm of the same planner: when `i64::midpoint`
  lands on a neighbour, `renumber` walks the group's current order (the moved task taken out)
  and hands the ranked tasks 1024, 2048, ... with the moved task taking the number of its slot.
  Since ranked tasks always sort first, the slot between two ranked neighbours is inside the
  ranked prefix, so the unranked tail is never visited by the rank sequence and stays untouched.
  A task whose number does not change (typically the first one, already at 1024) is left out of
  the batch, so `ranked` names only the files written and a renumber of a group whose top is
  already 1024 costs one file less. The `NoGap` error and its CLI test are gone; `beside` no
  longer returns a `Result`. The write order stays "others in group order, moved task last". Unlike a
  prefix ranking, a renumber does not keep the group's order in every intermediate state of a
  failed batch: a task above that gets its new, higher number before the tasks below it sinks
  under them until they are written (ranks -7, -6, 500 renumbered to 1024, 3072, 4096: after the
  first write alone the top task sits last), and a renumber that lowers ranks fails the other
  way round, so no single write order protects both. ADR 0025 accepts this ("no rollback, every
  intermediate state is a valid order"); the next move overwrites it.
  Review follow-ups: the renumber test keeps an unranked task at the bottom of the group and
  checks it byte for byte, a renumber where the moved task keeps its number is covered
  (`changed: []`, `ranked: [3]`), the CLI test asserts the `--json` document of a renumbering
  move, and `rank_prefix` / `renumber_from` share `with_rank`.
  Ten bisections of one gap before the renumber is exactly what the 1024 step promises: the
  core test walks 1536, 1280, ..., 1025 and the eleventh move renumbers.

- 2026-10-17 (RK-006): the TUI adds nothing to the planning: `Msg::MoveUp` / `MoveDown` become
  `Cmd::Move(id, Target::Before(above))` / `After(below)` with the neighbour read from the
  model's own `ListGroup` (`Model::group_of`, now public), so a filtered view and the Today
  view place the task next to what is shown, and `dispatch` calls `edit::move_task` like the
  CLI and words the status line like `tasq move` (`[3] moved before task 12`, `already
  before`, ` (also ranked: ...)`). The selection follows for free: the reload keeps
  `Cursor::Task(id)` whenever the id is still visible. The Today view still groups by
  status (`query::list`), so "a neighbour in another status group" is the group edge: the
  refusal is the same `nothing above [id] in its group` message. The `Ctrl+J` / `Enter`
  question is settled in crossterm, not here: outside raw mode it reports the byte `\n` as
  `Enter` (issue #371), in raw mode as `Ctrl+J`, and the kitty protocol names the key, so the
  map only ever sees `ctrl+j`; the test pins that `Chord::from_event` keeps the two apart and
  that `enter` on `move-down` is a configuration conflict. One row more in the help overlay
  no longer fits a 28-row terminal with the status bar (the popup is clamped and the `quit`
  row is cut), so the help snapshot is taken at 30 rows; the overlay was already cut at 24.
  The `.cargo/mutants.toml` excludes `crates/tui/**`, so the acceptance run is
  `--no-config --in-diff <(git diff main -- crates/tui)`.
  Review follow-ups: the `tasq move` line is `Moved::describe(&Target)` in core now, used by
  the CLI and the TUI alike (the TUI had copied the CLI's eight arms, dead ones included);
  the neighbour lookup is `Model::neighbours` (group, task above, task below, as shown),
  so `group_of` stays private and the update step only decides; the folded-group refusal
  names the `toggle-group` key as bound (ADR 0013) instead of a hardcoded `z`; and the
  `Ctrl+J` test no longer pretends to cover keyboard enhancement with a second identical
  press: it pins the map (`ctrl+j` is not `enter` in any mode, a kitty `Repeat` moves
  again) and says the byte parsing is crossterm's.

## Blockers

## Deviations from the plan

- 2026-10-17 (RK-004): the plan and ADR 0025 say a move into the unranked part ranks "every
  task from the top through the anchor". `move_task` ranks every unranked task *above the slot*
  instead: for `--before <unranked anchor>` the anchor itself stays unranked, and for
  `--before` the first unranked task (or `--after` the last ranked one) nothing but the moved
  task is written, one step below the last ranked task. The resulting order is the one the
  plan asks for in every case; the difference is one file fewer (US16). The anchor is only
  written when it is above the slot, that is for `--after`.
