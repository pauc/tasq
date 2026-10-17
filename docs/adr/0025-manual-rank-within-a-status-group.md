# ADR-0025: Manual rank within a status group (`## Rank`, `tasq move`)

- **Status:** Accepted
- **Date:** 2026-10-17

## Context

A status group is sorted by `(priority, due, id)` (`query::sort_key`), so the only way to
move a task up or down is to change its priority or due date. Users want to say "this one
next" without touching either (nb todo 43).

nb cannot hold the order for us: `nb list` sorts by mtime or by id, ids are `.index` line
positions that tasq never rewrites (reordering would renumber every id), and `nb pin` only
floats items into a single pinned tier. The order has to be a tasq field in the task file.

Constraints: the file stays readable by nb and the original script (ADR 0002), unknown and
unparsable lines are preserved byte for byte, a write should touch as few files as possible
(every write is a commit, ADR 0023), and `docs/json.md` is a contract that `tasq apply` must
keep reading.

## Decision

`Task` gains `rank: Option<i64>`. The file keeps it in a new optional section written only
by tasq, read like `## Due` (first non-empty line):

```
## Rank

1024
```

- **Ordering.** Ascending: the smallest rank comes first. Within a status group the sort key
  becomes `(rank if set, priority, due, id)`: ranked tasks come before unranked ones, equal
  ranks fall back to `(priority, due, id)`, unranked tasks keep the natural order. The done
  list keeps sorting by close time. `next` follows the rank because it shares the sort key.
- **Reading.** An integer line gives the rank. Anything else gives `rank = None`; the bytes
  are preserved and the next move overwrites them. A sort hint never makes a task unreadable.
- **Placement.** `## Rank` is a scalar section and follows the scalar chain: after
  `## Closed`, else `## Source`, `## Due`, `## Project`, `## Description`, else after the
  title line.
- **Kept across status changes.** A status change, closing and reopening keep the rank.
  After a status change the task slots into the new group's ranked block by its number.
- **Moving.** `tasq move <id> --before <id> | --after <id> | --top | --unrank`. The step is
  1024: `--top` writes the group's lowest rank minus 1024 (1024 when nothing is ranked),
  `--before`/`--after` write the midpoint between the neighbours, and when no integer fits
  the group's ranked tasks are renumbered to 1024, 2048, ... The first move into an
  unranked part of a group ranks every task from the top through the anchor, in the order
  currently shown. `--unrank` removes the section.
- **Refusals.** A move never changes status: an anchor in another status group is refused
  with an error naming both statuses, and a done task is refused with "task N is done".
- **Several files.** Ranking a prefix and renumbering write several files under one lock
  and one checkpoint (one commit). A failure midway stops and reports the ids written; there
  is no rollback, every intermediate state is a valid order.
- **JSON.** `rank` is a number, absent when unset. `tasq move --json` prints `"task"`,
  `"changed": ["rank"]` (or `[]`) and `"ranked": [id]` for the other tasks written. In
  `tasq apply` a missing `rank` means unchanged and `"rank": null` clears it, like
  `worktrees` (ADR 0022).
- **TUI.** `ctrl+k` / `ctrl+j` (actions `move-up` / `move-down`, rebindable, ADR 0013) move
  the selected task one visible slot, relative to its visible neighbour in a filtered view;
  a no-op with a status-bar message at a group edge, on a collapsed group, in the done list
  and across status groups (Today view). The rank number is never displayed.

## Consequences

### Positive

- Manual order without touching priority or due; `next` picks the task you put on top.
- The common move rewrites one file, so one small commit.
- Older files and older JSON keep working: no rank is the old behaviour.

### Negative

- The first move into an unranked group and a renumber write several files. Mitigated by
  the 1024 step (ten midpoints into the same gap before a renumber) and one checkpoint.
- A kept rank can put a task at an arbitrary height of its new group after a status change;
  the user moves it again. Clearing it was rejected because the user wants the place kept.
- One more scalar insertion rule in the format layer, which ADR 0024 counts as a cost of
  the section format.

## Alternatives considered

- **Rank inside priority, `(priority, rank, due, id)`** — every move is one file, but a task
  can never pass a higher priority without changing it, which defeats the purpose.
- **Rank the whole group on the first move** — simplest, but rewrites every file of the
  group for a one-step move.
- **Descending rank (higher first)** — rejected for "rank 1 = first", the reading most
  people bring to the word.
- **Clear the rank on status change or done** — avoids accidental placement in the new
  group, rejected to keep a task's place when it comes back.
- **A hidden `<!-- rank: N -->` comment or a `#rank-N` tag** — the comment format is still
  TBD in `docs/file-format.md`, and a tag would pollute the tag line and nb's tag index.
- **Reorder `.index` or use `nb pin`** — renumbers every id / only one pinned tier.

## References

- nb todo 43 (manual task order), `CONTEXT.md` (Rank, Move, Status group).
- `crates/core/src/query.rs` (`sort_key`, `next_from`), `docs/file-format.md`,
  `docs/json.md`, `docs/config.md`.
- ADR-0002, ADR-0013, ADR-0020 (`## Closed`, the scalar-section precedent), ADR-0022,
  ADR-0023, ADR-0024 (where `rank` would become a frontmatter key).
