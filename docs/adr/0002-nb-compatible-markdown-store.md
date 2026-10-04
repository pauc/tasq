# ADR-0002: nb-compatible markdown files as the day-one store

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

Today's tasks live in nb notebooks: one `*.todo.md` file per task, ids taken
from nb's `.index` (line number = id), status and priority encoded as `#tags`.
The author has years of tasks in this form, nb's own commands (`nb todos`,
`nb todo do`, `nb sync`) keep working on them, and the old script must keep
running on the same files during a side-by-side period (plan T-904). A rewrite
that started with a new store would need a migration before anyone could use it
and would break nb interoperability.

## Decision

The existing nb todo files are the canonical store on day one. The new tool
reads and writes the same files with the same ids, and changes nothing about
their layout except the section being edited (FR-1, FR-2).

Storage sits behind a `Store` trait (ADR-0003) implemented first by
`tasq-store-nb`. SQLite or other stores can be added later without touching the
CLI or TUI. No other store ships in v1.

### File format outline

The normative spec is `docs/file-format.md`. The shape the script writes, and
which the new tool preserves, is:

```
# [ ] Title

## Description
...
## Project
/abs/path
## Due
2026-10-10
## Related
- https://...
### Merge requests
- [title](url)
## Tags
#gitlab #A #ready
## Progress
- 2026-10-04 10:15: note
## Worktrees
- /path (`branch`)
## Sessions
- 2026-10-04 10:15: `session-id` — desc
```

### Optional additions

All optional, all ignored by nb and the old script:

- A `## Source` section (for example `gitlab: https://.../merge_requests/123`)
  recording the external origin of a task for reconciliation (ADR-0004).
- HTML comments for metadata the model needs but humans should not see:
  `<!-- tasq: {...} -->`.

### Lossless unknown sections

The format layer parses a file into a `Task` plus a `Document` that keeps every
unknown section and unknown line verbatim, in order. Writing a task back edits
only the sections that changed and re-emits everything else byte for byte.
Round-trip tests require `write(parse(x)) == x` for files produced by the script.

## Consequences

### Positive

- Zero migration: point the tool at the notebook and it works.
- nb, the old script and the new tool can run on the same notebook at once.
- Files stay human-editable and greppable; git history stays meaningful.
- The `Store` trait keeps the door open for stable-id stores later.

### Negative

- Ids are positional and can shift after deletions and `nb index reconcile`.
  `Store::describe` reports this so the CLI can warn.
- Lossless editing is more work than regenerating files, and the writer must
  match the script's insertion rules exactly (plan T-103).
- Markdown is a weak schema; the parser must never panic on arbitrary input
  (fuzz tests) and must report non-task files as `NotATask`.
- A directory of hundreds of files is slower than a database for queries, but
  well within the 50 ms target.

## Alternatives considered

- **SQLite first, import from nb** — stable ids and fast queries, but requires a
  migration, breaks nb and the old script, and duplicates the source of truth
  during the transition.
- **New markdown format with front matter** — cleaner to parse, but nb and the
  old script would not understand it; same migration problem.
- **Keep driving nb for all reads and writes** — correct by construction but
  slow (a bash startup per command) and unusable without nb. See ADR-0007 for
  the hybrid that was chosen instead.

## References

- Plan sections 4.4, 4.5, 9 and tasks T-102, T-103, T-201 to T-206, T-904.
- `docs/file-format.md`.
- ADR-0003, ADR-0007.
