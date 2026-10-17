# ADR-0024: Format v2: frontmatter for scalar fields and a stable id, after nb compatibility ends

- **Status:** Proposed (deferred until the side-by-side period of T-904 is over and the old script is retired)
- **Date:** 2026-10-17

## Context

ADR-0002 rejected "a new markdown format with front matter" because nb and the
old `tasks` script would not understand it. The constraint was verified again
on 2026-10-17 against nb and `original/tasks`:

- The old script reads line 1 and only treats a file as a task when it starts
  with `# [ ] ` or `# [x] `. A file that starts with `---` vanishes from every
  listing. `tasq` has the same line-1 rule (`NotATask` in `docs/file-format.md`).
- nb survives a leading `---` block: `_get_content` skips front matter and takes
  the first `# ` heading after it, and `nb todo do` flips `# [ ]` with a
  file-wide `sed`. The one trap is a `title:` key inside the block, which nb
  takes as the title and so loses the `[x]` done detection it reads from the
  listed title.
- nb's `search --tags` and the hashtag links in `nb browse` match `#word`
  tokens in the body. Tags moved into a YAML list are invisible to them.

While those tools share the notebook, the current format stands. The question
this record settles is what happens once they no longer do. Two forces remain
after that point:

1. **Ids are positional.** A task's id is the line number of its file in nb's
   `.index` (ADR-0002, plan section 9). `tasq store reconcile` keeps lines in
   place, but the scheme exists for nb's sake, `describe()` reports it as
   `positional`, and the plan leaves stable ids to "future stores". A stable id
   has to live somewhere in the file, and none of the existing sections is a
   sensible home.
2. **Scalar fields have non-uniform placement.** `## Project` goes after
   `## Description`, `## Due` after `## Project`, `## Source` after `## Due`,
   `## Closed` after `## Source`, a new `## Tags` at end of file, and status and
   priority share a `#tag` line with topic tags. Most of `format::ops` exists to
   reproduce these rules. Each new scalar adds one more rule and one more ADR
   about where its heading goes.

Priority, due date and tags already parse and round-trip correctly as sections.
Moving them alone would not justify touching every file. Moving them together
with the id, in the one migration that the id requires anyway, would.

## Decision

Deferred. When the old script is retired and nb's commands are no longer run
on the notebook, `tasq` adds a **format v2** and a `tasq migrate` command. Until
then nothing changes: the v1 format stays normative and no v2 code is written.

Format v2, as proposed:

- **A frontmatter block** (`---` lines) at the top of the file holds the scalar
  fields: `id`, `status`, `priority`, `due`, `closed`, `project`, `source`, and
  `tags` as a one-line list. Keys are flat; values are scalars or one-line
  lists. No nesting.
- **The body keeps the sections that are lists or prose**: `# Title` as the
  first heading, `## Description`, `## Related` with `### Merge requests`,
  `## Progress`, `## Worktrees`, `## Sessions`. They are append-only, read by
  humans and diff well in git; a session entry (time, id, description) is
  worse as YAML than as a bullet line.
- **Done is a status value**, so the `[ ]`/`[x]` marker leaves the title line,
  the "status tag absent on done tasks" rule and the stale `## Closed` edge case
  (ADR-0020) go with it.
- **No `title:` key.** The body heading is the only title.
- **Stable ids.** `id` is written on migrate and on create, never derived from
  `.index` position; the v2 store reports `stable` through `describe()`.
- **Lossless, line-based.** The frontmatter is parsed line by line in
  `tasq-core` like the sections are today, over a documented flat subset of
  YAML. Unknown keys, their order, unknown sections and blank lines are
  preserved verbatim; `write(parse(x)) == x` holds for v2 files as it does for
  v1. No YAML library, so the crate stays dependency-free, no-I/O and cheap to
  mutation test.
- **Read both, write what the file has.** The v1 reader stays. A file is v2
  when it starts with `---`. A v1 file is never rewritten as v2 by an ordinary
  edit; only `tasq migrate` converts a notebook, explicitly and in one commit.
- **Sequence.** Not before Phases 3 to 5 are done. The CLI, sources and
  launchers talk to the `Store` trait (ADR-0003) and need no change for v2.
  Accepting this record supersedes the "new markdown format with front matter"
  bullet of ADR-0002, not ADR-0002 itself.

## Consequences

### Positive

- Stable ids: deletions and reconciles stop renumbering tasks, and the
  positional-id warning in `tasq doctor` goes away for v2 notebooks.
- Scalars become one block with one insertion rule. New fields are a key, not a
  heading placement decision; `format::ops` shrinks to the list sections.
- One status model: done is a status like the others, closed time sits next to
  it.
- Editors and note tools that understand frontmatter (Obsidian properties and
  the like) show the fields as fields.

### Negative

- A per-file migration, mitigated by making it explicit (`tasq migrate`), one
  commit, reversible from git history, and never triggered by an edit.
- Two readers for the transition, mitigated by the `Document` model already
  separating the parsed task from the preserved bytes; v2 adds a block kind,
  not a second model.
- nb's `todos`, `todo do`, `search --tags` and browse hashtags stop being
  useful on a v2 notebook. This is the premise of the record, not a side
  effect: it is accepted only when those commands are no longer used.
- `#tags` in the body were greppable; a `tags:` list is still greppable but by
  a different pattern. Scripts outside `tasq` that grep the notebook need a
  one-line change.

## Alternatives considered

- **Keep v1 forever** — zero migration, but ids stay positional and every new
  scalar needs a heading rule. Acceptable as long as nb commands are in use,
  which is exactly the condition this record waits on.
- **Frontmatter now, without stable ids** — breaks the old script during the
  side-by-side period for fields that already parse fine. Rejected in ADR-0002
  and rejected again here.
- **Stable id in a `<!-- tasq: {...} -->` comment** — keeps line 1 and nb
  compatibility, but hides the one field a human most needs to see and leaves
  the heading rules untouched. Reasonable as a stopgap if a stable id is
  needed before the transition ends; it would not change this decision.
- **Everything in frontmatter, including progress and sessions** — a single
  parser, but YAML lists of objects diff badly, are hard to append by hand and
  would need a real YAML library to round-trip, giving up the line-based,
  dependency-free format layer.
- **SQLite store with markdown export** — stable ids and fast queries, but a
  second source of truth and no human-editable files; the plan keeps it as a
  possible later store behind the same trait, not as the v2 format.

## References

- ADR-0002 (day-one store; "front matter" under Alternatives considered).
- ADR-0003 (the `Store` trait the v2 store implements), ADR-0020 (`## Closed`),
  ADR-0023 (no nb at runtime).
- `docs/file-format.md` (v1, normative).
- Plan sections 4.4 and 9, tasks T-204 (`describe()`), T-904 (side-by-side
  period and switch-over).
- nb: `_get_content` (front matter and title detection), `_todo` `do`/`undo`
  (`sed s/# \[ \]/# \[x\]/`), `--tags` search pattern, checked 2026-10-17.
