# ADR-0032: Done reasons and source keys checked per kind

- **Status:** Accepted
- **Date:** 2026-10-28

## Context

`tasq sync` closes a matched open task when its source reports the item as done. Until now that
was one state, `ItemState::Done`, and one switch, `[[source]] close_when_done` (default `true`),
for five different events: a merge request merged, closed, or approved by me, and an issue
closed or no longer assigned to me. The reason only survived as the note text (`"merged"`,
`"no longer assigned to you"`).

At Acme an issue moving to `Core::Review` is reassigned to the merge request's reviewer.
`WorkItems::check` reports an open issue no longer assigned to me as done, so sync closed the
task while its work was still in review (task 216 in the `acme` store). Turning `close_when_done`
off to keep it open would also have stopped tasks from closing when their issue closes.

Separately, `SourceConfig` is one flat struct for every kind. `labels` on a review-requests
source or `drafts` on a work-items source were accepted and silently ignored, so a key in the
wrong block looked like it worked.

## Decision

A source reports **why** an item is done: `ItemState::Done(DoneReason)`, with `DoneReason` one
of `Gone` (404), `Closed`, `Merged`, `Approved`, `Unassigned` and `Reported` (the LLM bridge's
`done`). The source's `Policy` holds one switch per reason a user may turn off,
`DoneWhen { closed, merged, approved, unassigned }`, all on by default, and
`Policy::closes(reason)` decides; `Gone` and `Reported` always close. A done item whose reason
does not close is reconciled like an open one: no `Close`, no note, and `flag` still applies.
The note stays the source's text; the reason decides, the note explains. The LLM bridge JSON is
unchanged (`"state": "open" | "done" | "needs-attention"`): `done` reads as `Done(Reported)`.

`close_when_done` is replaced by four `[[source]]` keys, `done_when_closed`,
`done_when_merged`, `done_when_approved` and `done_when_unassigned` (default `true`). Each kind
declares the keys it reads (`SourceKind::keys`, on top of `SourceKind::COMMON_KEYS`). Every
config file's `[[source]]` tables are checked before the file is deserialised: a
`close_when_done` key is an error naming its replacements, whatever else the source holds (no
`name` or `kind` needed), and in a source whose `name` and `kind` parse, a key that the kind
does not read is an error at the key, naming the source and the kind:

```
config.toml:12:1: source "gitlab-work-items": `done_when_approved` does not apply to kind gitlab-work-items
```

| Key | Review requests | Work items | LLM bridge |
|-----|-----------------|------------|------------|
| `done_when_closed` | MR closed without merge | issue closed | no |
| `done_when_merged` | MR merged | no | no |
| `done_when_approved` | I approved the MR | no | no |
| `done_when_unassigned` | no | open, no longer assigned to me | no |

## Consequences

### Positive

- `done_when_unassigned = false` on a work-items source keeps a reassigned issue's task open
  and still closes it when the issue closes.
- Every way a task gets closed by sync has a name in the code, and `Policy` never needs to know
  the source's kind: a key the kind does not read keeps its default.
- A key in the wrong `[[source]]` block fails at load with its file, line and column instead of
  doing nothing.

### Negative

- Breaking for configs that set `close_when_done` or a key of another kind. There is no
  migration command: the error names the new keys and points at `docs/sources.md`, and
  `docs/migration.md` lists the moves.
- A new `SourceConfig` field must be added to the key table; a test checks that every field is
  common or read by at least one kind.
- The kind check needs a `name` and a `kind` that parse; a source without them gets the
  existing schema errors first.

## Alternatives considered

- **A `done_when = ["closed", "merged"]` list** — one key instead of four, but the values
  differ per kind and a typo in a string is harder to point at than a key.
- **Separate config structs per kind** (a tagged enum on `kind`) — the per-kind check for free,
  but an internally tagged enum buffers the table before it picks the variant, which loses
  the key spans the error messages rely on, and every consumer of `SourceConfig` would match
  on the kind.
- **Detecting the reviewer handoff** (by label or by the new assignee) — specific to one
  team's workflow; turning the reason off covers it without the source knowing.

## References

- Plan: `ruli/features/done-reasons/PLAN.md` (task 61).
- ADR 0004 (each source adapter decides its sync strategy).
- Code: `tasq_core::source` (`DoneReason`, `DoneWhen`, `Policy::closes`),
  `tasq_core::config::SourceKind::keys`, `check_source_keys` in `crates/core/src/config/load.rs`.
- Docs: `docs/sources.md` ("When a task is marked done"), `docs/config.md`.
