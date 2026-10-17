# Plan: manual rank within a status group

Task: tasq 43 ("manual task order: move a task up/down within its status group (TUI J/K, tasq move)").
Decision record: [ADR 0025](../../../docs/adr/0025-manual-rank-within-a-status-group.md). Vocabulary:
[CONTEXT.md](../../../CONTEXT.md) (status group, rank, move). Progress: [PROGRESS.md](PROGRESS.md).
Started 2026-10-17.

## Problem Statement

Inside a status group, tasks are always ordered by priority, then due date, then id. When I want
a particular task to be the next one I work on, the only levers are its priority or its due date,
and both carry meaning of their own: bumping a `#B` task to `#A` or inventing a due date just to
float it to the top lies about the task. `tasq next` then picks whatever that order puts first,
not what I decided to do next. nb gives no help: its own ordering is by mtime or id, ids are
positions we never rewrite, and pinning is a single tier.

## Solution

A task can carry a **rank**: its manually set place among the tasks of its status group. I move a
task with `tasq move <id> --before <id>`, `--after <id>` or `--top`, or with `ctrl+k` / `ctrl+j`
in the TUI, and the list, the TUI and `tasq next` all follow that order. Ranked tasks come first,
smallest rank on top; tasks I never moved keep the familiar priority, due, id order below them.
`tasq move <id> --unrank` hands a task back to the natural order. The rank lives in the task file
as a small `## Rank` section, stays out of every display, and survives status changes, closing and
reopening, so a task keeps its place when it comes back.

## User Stories

1. As a tasq user, I want to put a task at the top of its status group, so that it is the first thing I see in that group.
2. As a tasq user, I want to place a task directly before another task of the same group, so that I can order two tasks relative to each other.
3. As a tasq user, I want to place a task directly after another task of the same group, so that I can push a task down without moving everything else.
4. As a tasq user, I want to move a `#C` task above a `#A` task, so that my manual decision wins over priority without me changing the priority.
5. As a tasq user, I want tasks I never moved to keep the priority, due date, id order, so that the feature costs nothing until I use it.
6. As a tasq user, I want ranked tasks to come before unranked ones in a group, so that the tasks I deliberately placed are on top.
7. As a tasq user, I want `tasq next` to start the top ranked task of `in-progress` (else `ready`), so that "next" means what I decided.
8. As a tasq user, I want `tasq list` (grouped, single-status, filtered) to show tasks in rank order, so that the CLI and the TUI agree.
9. As a tasq user, I want `tasq list --json` to come out in the same order, so that scripts see what I see.
10. As a tasq user, I want `tasq move <id> --unrank` to drop a task's rank, so that it falls back to the natural order.
11. As a tasq user, I want a task to keep its rank when I change its status, so that it lands in the new group where its number places it.
12. As a tasq user, I want a task to keep its rank when I close it and reopen it, so that it comes back to the place it had.
13. As a tasq user, I want the done list to keep sorting by close time, so that "what did I close recently" still works.
14. As a tasq user, I want `tasq move` to refuse an anchor from another status group with an error naming both statuses, so that a move never changes status by accident.
15. As a tasq user, I want `tasq move` to refuse a done task with "task N is done", so that I am not ranking tasks that have no group.
16. As a tasq user, I want a move to rewrite only the moved task's file in the common case, so that each move is one small commit.
17. As a tasq user, I want my first move into an unranked part of a group to fix the order of the tasks above the destination, so that the move lands exactly where I asked.
18. As a tasq user, I want the group to be renumbered only when there is no room left between two neighbours, so that multi-file writes are rare.
19. As a tasq user, I want a move that writes several files to be one commit, so that history shows one move as one change.
20. As a tasq user, I want a move that fails halfway to tell me which tasks were already written, so that I know the state of my notebook.
21. As a tasq user, I want a move that leaves the task where it already is to write nothing, so that repeated commands are idempotent.
22. As a scripting user, I want `tasq move --json` to give me the moved task, `changed: ["rank"]` or `[]`, and `ranked: [id]` for the other tasks written, so that I can tell what changed.
23. As a scripting user, I want `rank` in every task JSON, absent when unset, so that I can read the order without parsing markdown.
24. As a scripting user, I want `tasq apply` to leave the rank alone when my document has no `rank` key, so that older documents and hand-written ones never clear ranks.
25. As a scripting user, I want `"rank": null` in `tasq apply` to clear the rank, so that I can unrank through JSON.
26. As a scripting user, I want `tasq view --json | tasq apply` to stay a no-op on ranked tasks, so that the round trip contract holds.
27. As a TUI user, I want `ctrl+k` to move the selected task up one slot, so that I can reorder without leaving the list.
28. As a TUI user, I want `ctrl+j` to move the selected task down one slot, so that I can reorder in both directions.
29. As a TUI user, I want the selection to follow the task I moved, so that I can keep pressing the key to move it further.
30. As a TUI user, I want a move in a filtered view to place the task relative to its visible neighbour, so that the move does what I see.
31. As a TUI user, I want a status-bar message instead of a move at the top or bottom of a group, on a collapsed group, in the done list or across status groups in the Today view, so that I understand why nothing moved.
32. As a TUI user, I want to rebind the move keys under `[ui.keys]` (`move-up`, `move-down`), so that they fit my terminal and habits.
33. As a TUI user, I want `ctrl+j` never to be taken for `Enter`, so that reordering never launches a session.
34. As a tasq user, I want the rank number hidden from `list`, the TUI rows and the detail pane, so that meaningless numbers do not clutter the views.
35. As a tasq user, I want to see and edit `## Rank` in the raw file (`tasq view --raw`, `$EDITOR`), so that I keep full control over my data.
36. As a tasq user, I want a hand-edited, invalid `## Rank` to count as unranked and to be kept as written, so that a typo never makes a task unreadable.
37. As an nb user, I want ranked files to stay readable and editable by nb and the original script, so that switching between tools stays safe.
38. As a tasq user, I want sync to keep the ranks of the tasks it updates, so that syncing a source does not scramble my order.
39. As a tasq user with tasks in the no-status group, I want to rank them like any other group, so that the trailing group is not second-class.
40. As a maintainer, I want the rank format and semantics documented in the file-format, JSON and config references, so that other tools and future me can rely on them.

## Implementation Decisions

- **Model.** `Task` gains `rank: Option<i64>`. Smaller is higher. Serialized as a number, omitted
  when `None`.
- **Sort key.** The core sort key becomes `(rank if set, priority, due, id)` with ranked before
  unranked. Every consumer of the shared comparison (grouping, `list`, `next`) follows it without
  changes of its own. Done tasks keep their own close-time order. The doc comments of the query
  module and of `next` describe the new order.
- **Format.** A tasq-only `## Rank` section holding one integer, read from the first non-empty
  line like `## Due`. Non-integer content gives `rank = None` and is preserved byte for byte.
  A new format op sets, replaces or removes the section; insertion follows the scalar chain
  (after `## Closed`, else `## Source`, `## Due`, `## Project`, `## Description`, else after the
  title line). `Document::from_task` writes it in the same place.
- **Store diff.** The nb store's diff learns `rank`: a changed rank applies the op, so `update`
  round-trips it and sync (which writes whole tasks) keeps it.
- **`Store::update_many(&[Task])`.** A new trait method. The default implementation calls
  `update` in order. The nb store overrides it to take the lock once, write every file, register
  nothing new, and checkpoint once. On failure it stops and reports the ids already written
  alongside the error. Every implementation keeps the per-task `Conflict` check.
- **`edit::move_task(store, id, target)`.** The one domain entry point, used by the CLI and the
  TUI. `target` is `Top`, `Before(id)`, `After(id)` or `Unrank`. It reads the task and its status
  group, refuses a done task and an anchor in another group (errors carry the task id and both
  statuses), plans the new ranks and writes them through `update_many`. It returns the moved task
  as stored, whether its rank changed, and the ids of the other tasks written.
- **Planning rules.** Step 1024. `Top`: lowest rank in the group minus 1024, or 1024 when nothing
  is ranked. `Before`/`After`: the midpoint between the two neighbours in the group's current
  order. If the anchor or the slot sits in the unranked part of the group, every task from the
  top through the anchor is ranked first, in the order currently shown. When no integer fits
  between the neighbours, the group's ranked tasks are renumbered 1024, 2048, ... in their current
  order. A move that leaves the task where it is writes nothing.
- **CLI `tasq move <id> (--before <id> | --after <id> | --top | --unrank)`.** The options are
  mutually exclusive and one is required. It renders the core result: a one-line confirmation, or
  with `--json` the `task` / `changed` / `ranked` document. It warns that ids are positional the
  way other commands do. Exit codes and error rendering follow the existing CLI conventions.
- **`tasq apply`.** A document without a `rank` key keeps the stored rank (same mechanism as
  `worktrees`); `"rank": null` clears it.
- **TUI.** New actions `move-up` (`ctrl+k`) and `move-down` (`ctrl+j`) in the list keymap,
  rebindable under `[ui.keys]`. The update step maps them to `move_task` with `Before`/`After` on
  the visible neighbour when it is in the same status group, then reloads and keeps the moved task
  selected. Otherwise it is a no-op with a status-bar message (group edge, collapsed group, done
  list, cross-group neighbour in the Today view). The help overlay lists both keys. The rank is
  never rendered.
- **Docs.** `docs/file-format.md` (the `## Rank` section), `docs/json.md` (`rank`, `tasq move`,
  the `apply` rule), `docs/config.md` (the two actions in the key table), the README command list,
  and the Claude plugin skill if it enumerates commands.

## Testing Decisions

- Tests exercise public behaviour only: what a command prints and exits with, what a store reads
  back, what order a query returns. The move planner is private and is tested through
  `move_task`. Assertions are exact (exact rank values, exact id order, exact strings), because
  the acceptance bar is zero missed mutants in each touched module.
- **Core `edit::move_task` over `MemoryStore`** is the main seam: top, before, after, unrank,
  midpoints, ranking a prefix, renumbering, idempotent moves, refusals, the returned
  `changed`/`ranked`. Prior art: the existing `edit` tests (`set`, `done`, `revise`) over
  `MemoryStore`.
- **Core query and format** for the sort key (ranked before unranked, ties, `next`) and the
  `## Rank` section (parse, invalid kept, op insert/replace/remove at each point of the scalar
  chain). New fixtures round-trip byte for byte. Prior art: the query unit tests, `format_ops`,
  `format_parse`, `format_fixtures`.
- **Store-nb** for `update_many` (one checkpoint, ids written on a mid-batch `Conflict`) and the
  rank diff through `update`. Prior art: `write.rs` with `NbEnv` and `wait_for_bookkeeping()`.
- **CLI** insta snapshots in `cli.rs` for `tasq move` text and `--json`, the refusals, the
  options' exclusivity, and the `apply` missing-key rule. They do not re-test planning.
- **TUI** unit tests for the key mapping (including `ctrl+j` vs `Enter`), the update step
  (selection follows, no-op messages) and a render snapshot after a move. Prior art: the
  `keys.rs` tests and `render.rs` `TestBackend` snapshots.
- Mutation runs: `format`, `query` and `edit` in core with the scoped `--no-config` recipe, then
  `--in-diff` for the store-nb, CLI and TUI changes. One run at a time, `--jobs 2`.

## Out of Scope

- `tasq list --sort` and other alternative orders (task 32).
- A `--bottom` target and a TUI key for unranking.
- Moving a task across status groups, or changing status as part of a move.
- Ranking done tasks or changing the done list's order.
- Showing the rank number anywhere outside JSON and the raw file.
- Batching sync and apply writes through `update_many` (task 24 may reuse it).
- Format v2 frontmatter (ADR 0024), where `rank` would become a frontmatter key.

## Further Notes

- The kept rank after a status change is deliberate (ADR 0025): a task can land at an arbitrary
  height of its new group's ranked block, and the user moves it again if needed.
- The 1024 step gives ten midpoints into the same gap before a renumber.
- `pick` takes an explicit id, so the rank does not apply to it.

## Sub-tasks

Tracer-bullet slices, blockers first. Work the frontier: any sub-task whose blockers are done.
Each one closes with a mutation run on the modules it touched (zero missed) and a row update in
PROGRESS.md.

### RK-001: `Store::update_many`, one lock and one checkpoint (prefactor)

**What to build:** a batch write on the `Store` trait. The default implementation writes each task
with `update`, in order. The nb store takes the lock once, writes every file and checkpoints once,
so a batch is one commit; on a failure it stops and reports the ids already written alongside the
error. Nothing calls it yet; it makes RK-004 and RK-005 easy, and task 24 may reuse it.

**Blocked by:** None (can start immediately).

**Status:** done (2026-10-17)

- [x] The default implementation writes every task and stops at the first error, reporting the ids written before it.
- [x] On the nb store, a batch of several tasks produces exactly one bookkeeping commit.
- [x] A `Conflict` on a task in the middle of a batch leaves the earlier files written, the later ones untouched, and reports the written ids.
- [x] An empty batch writes nothing and starts no job.
- [x] Zero missed mutants in the touched code.

### RK-002: a rank in the file orders lists and `next`

**What to build:** a task carries a rank read from a `## Rank` section, and every ordering of a
status group follows it: ranked tasks first, smallest on top, then the natural priority, due, id
order. A rank written by hand into a task file is enough to see `tasq list`, `tasq list --json`,
the TUI and `tasq next` follow it. The rank appears in task JSON and survives `apply` and sync.

**Blocked by:** None (can start immediately).

**Status:** done (2026-10-17)

- [x] `Task::rank` is read from the first non-empty line of `## Rank`; a non-integer value gives no rank and the bytes are preserved.
- [x] Setting, replacing and removing the rank through the store writes or removes `## Rank` at the right place in the scalar chain; round trips of new fixtures are byte-identical.
- [x] Within a status group, ranked tasks come before unranked ones, ascending, with ties broken by priority, due, id; the done list order is unchanged.
- [x] `tasq next` picks the top ranked task of the first non-empty next status.
- [x] Task JSON has `rank` as a number, absent when unset; `tasq apply` without a `rank` key keeps the stored rank, `"rank": null` clears it; `view --json | apply` is a no-op on a ranked task.
- [x] A status change and a close/reopen keep the rank.
- [x] `docs/file-format.md` and `docs/json.md` document the section and the field.
- [x] Zero missed mutants in `format`, `query` and the store diff.

### RK-003: `tasq move --top` and `--unrank`

**What to build:** `tasq move <id> --top` puts a task first in its status group and
`tasq move <id> --unrank` hands it back to the natural order, each rewriting only that task's file.
Both go through `edit::move_task`, the one entry point the TUI will share.

**Blocked by:** RK-002.

**Status:** done (2026-10-17)

- [x] `--top` writes the group's lowest rank minus 1024, or 1024 when nothing in the group is ranked; `next` then picks it when the group is a next status.
- [x] `--unrank` removes the rank; on an unranked task it writes nothing.
- [x] A move that leaves the task where it is writes nothing and reports `changed: []`.
- [x] A done task is refused with "task N is done".
- [x] The target options are mutually exclusive and one is required (clap error otherwise).
- [x] Text output is a one-line confirmation; `--json` prints `task`, `changed` and `ranked` (empty here), with insta snapshots.
- [x] README command list and the Claude plugin skill (if it enumerates commands) mention `tasq move` (the skills do not enumerate commands; `docs/agents/issue-tracker.md` does and was updated).
- [x] Zero missed mutants in `edit` and the new CLI command.

### RK-004: `tasq move --before` and `--after`

**What to build:** `tasq move <id> --before <id>` and `--after <id>` place a task next to another
task of the same status group. The common move writes one file at the midpoint between the
neighbours; the first move into an unranked part of a group also ranks every task from the top
through the anchor, all in one commit through `update_many`.

**Blocked by:** RK-001, RK-003.

**Status:** ready-for-agent

- [ ] Between two ranked neighbours, the task gets their midpoint and only its file is written.
- [ ] Before the first ranked task, the rank is that task's rank minus 1024; after the last ranked task with nothing unranked below the slot, plus 1024.
- [ ] An anchor in the unranked part ranks every task from the top through the anchor in the current order (step 1024), the moved task lands at the requested side, and `ranked` lists the other ids written.
- [ ] A multi-file move produces one bookkeeping commit; a mid-batch failure reports the ids written.
- [ ] An anchor in another status group is refused with an error naming both statuses; an anchor equal to the task is refused.
- [ ] When no integer fits between the neighbours, the move is refused with a clear error (renumbering comes in RK-005).
- [ ] Zero missed mutants in `edit` and the CLI changes.

### RK-005: renumber a status group when the gap runs out

**What to build:** a move into a slot with no free integer renumbers the group's ranked tasks to
1024, 2048, ... in their current order, then places the task, in one commit. The refusal from
RK-004 goes away.

**Blocked by:** RK-004.

**Status:** ready-for-agent

- [ ] Ten repeated moves into the same gap succeed; the eleventh renumbers and still lands the task in the requested slot.
- [ ] Renumbering keeps the relative order of every ranked task and leaves unranked tasks untouched.
- [ ] `ranked` lists every other task whose rank was rewritten; one bookkeeping commit.
- [ ] Zero missed mutants in `edit`.

### RK-006: `ctrl+k` / `ctrl+j` in the TUI

**What to build:** in `tasq ui`, `ctrl+k` moves the selected task up one visible slot and `ctrl+j`
down one, through `edit::move_task`, and the selection follows the task. Where a move makes no
sense the status bar says why instead.

**Blocked by:** RK-004.

**Status:** ready-for-agent

- [ ] New actions `move-up` (`ctrl+k`) and `move-down` (`ctrl+j`), rebindable under `[ui.keys]`, listed in the help overlay and in `docs/config.md`.
- [ ] A move places the task before/after its visible neighbour (in a filtered view too) and the moved task stays selected after the reload.
- [ ] No-op with a status-bar message at a group edge, on a collapsed group, in the done list and when the neighbour is in another status group (Today view).
- [ ] `ctrl+j` is never interpreted as `Enter`, with and without keyboard enhancement (test on the key mapping).
- [ ] The rank number is never rendered (render snapshot after a move).
- [ ] Zero missed mutants in the touched TUI modules.
