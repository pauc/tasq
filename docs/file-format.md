# Task file format

**Status:** first normative draft (2026-10-04). Derived from plan section 4.4
and from what `original/tasks` reads and writes. Points marked **TBD** are not
yet decided; do not infer behaviour for them from this document.

A task is one markdown file in an nb notebook. nb, the old `tasks` script and
`tasq` all read and write the same files. `tasq` must preserve anything it does
not understand (see "Lossless editing").

## File and identity

- Filename: `<stamp>.todo.md`, where `<stamp>` is nb's timestamp
  (`YYYYMMDDHHMMSS`). **TBD:** collision rule when two files are created in the
  same second; T-203 verifies against nb's `_add` implementation.
- Id: the 1-based line number of the filename in the notebook's `.index` file.
  Ids are positional and can change after deletions or `nb index reconcile`.
- Index lines that are not `*.todo.md`, or whose file no longer exists, are
  skipped when listing.
- Encoding UTF-8, LF line endings when writing. Files with CRLF must parse;
  **TBD** whether they are rewritten with LF or preserved.
- Related ADRs: [0002](adr/0002-nb-compatible-markdown-store.md),
  [0007](adr/0007-nb-under-the-hood.md).

## Title line

Line 1 is the title and done marker:

```
# [ ] Title        open
# [x] Title        done
```

Exactly one space inside the brackets and one after the closing bracket. The
old script lists only files whose first line starts with `# [ ] `; files whose
first line is anything else are not tasks (`NotATask`). Marking done flips
`[ ]` to `[x]` on this line only (what `nb todo do` does) and removes the status
tag (see Tags).

## Sections

Sections are level-2 headings `## Name`, matched exactly (`## ` plus the name,
no trailing text). A section runs until the next line starting with `## `.
Each heading is followed by one blank line, then its content, then one blank
line before the next heading. A file created by the script has this order:

```
# [ ] Title

## Description

Free text, any number of lines.

## Project

/absolute/path

## Due

2026-10-10

## Related

- https://example.invalid/issue/1

### Merge requests

- [MR title](https://example.invalid/-/merge_requests/123)

## Tags

#gitlab #A #ready

## Progress

- 2026-10-04 10:15: created via tasks create
- 2026-10-04 11:40: note

## Worktrees

- /abs/path/to/worktree (`branch-name`)

## Sessions

- 2026-10-04 10:15: `session-id` — short description
```

Only `## Tags` and `## Progress` are always written on create; the others
appear when they have content. `## Worktrees`, `## Sessions` and
`### Merge requests` are added later by the corresponding commands.

### Description

Free markdown. `tasq` treats it as opaque text.

### Project

One absolute directory path. Reading takes the first non-empty line of the
section. Writing replaces the whole section body with a blank line and the
path.

### Due

Reading takes the first non-empty line. The plan requires `tasq` to write ISO
dates `YYYY-MM-DD` (and accept `today`, `tomorrow` on input). The old script
stored whatever string it was given, so existing files may contain other
shapes; **TBD** how non-ISO values are reported.

### Related

A list of `- <url>` lines, one link per line. **TBD** whether `- [label](url)`
is accepted in the top-level list; the script only writes bare URLs there.

#### `### Merge requests`

A level-3 subsection inside `## Related`. Each entry is `- [title](url)`. The
subsection ends at the next line starting with `##` (that is, any `##` or
`###` heading). An MR is already tracked when `(url)` occurs in the subsection.

### Tags

One line of whitespace-separated `#tags`. All lines in the section are joined
and split on whitespace when reading. Three kinds of tag share the line:

- **Status**: one of the configured workflow statuses (default `in-progress`,
  `ready`, `waiting`, `blocked`, `later`). Absent on done tasks and on tasks
  with no status.
- **Priority**: `A`, `B` or `C`. Absent means `B`.
- **Topic tags**: everything else (for example `#gitlab`, `#review-request`).

Create writes topic tags first, then priority, then status: `#gitlab #A #ready`.
Status and priority are not tags in the `tasq` model; the format layer maps
them. **TBD:** behaviour when two status tags or two priority tags are present
(the script keeps the last one seen).

### Progress

Entries `- YYYY-MM-DD HH:MM: note`, local time. Legacy entries `- YYYY-MM-DD:
note` (no time) must parse. The latest note is the last `- ` line. Note text is
a single line.

### Worktrees

Entries `- /abs/path` or ``- /abs/path (`branch`)`` with the branch in backticks
inside parentheses. The path contains no whitespace (the script reads it as the
second whitespace-separated field). A worktree is already tracked when a line
equals `- <path>` or starts with `- <path> `.

### Sessions

Entries ``- YYYY-MM-DD HH:MM: `session-id` `` optionally followed by
` — description` (space, U+2014 EM DASH, space). A session is already tracked
when `` `session-id` `` occurs anywhere in the file.

### Source (new, optional)

Written only by `tasq`, ignored by nb and the script. Records the external
origin used for reconciliation, one line per origin: `<source-name>: <url>`,
for example `gitlab: https://host/group/project/-/merge_requests/123`.
**TBD:** external ids without a URL, and more than one origin per task.

### HTML comments (new, optional)

`<!-- tasq: {...} -->` carries metadata the model needs but humans should not
see. **TBD:** placement and JSON shape. Renderers hide it; the old script and
nb ignore it.

## Editing rules

These reproduce the script's helpers so that files edited by either tool look
the same.

### Appending an entry to a list section (`append_to_section`)

Used for Progress, Worktrees and Sessions.

1. If the section is missing:
   - if the section is not Progress and `## Progress` exists, insert `## Name`
     and a blank line immediately before `## Progress`;
   - otherwise append a blank line and `## Name` at the end of the file.
2. If the section has at least one `- ` line, insert the entry right after the
   last one.
3. Otherwise insert a blank line and the entry right after the heading.

### Setting the project (`set_project`)

- If `## Project` exists, replace its body with a blank line and the path, drop
  everything up to the next `## ` heading, and keep one blank line before it.
- If it does not exist and the first section is `## Description`, insert the
  section after Description (before the second heading, or at end of file).
- If it does not exist and the first section is anything else, insert the
  section right after the title line.
- In both insert cases: a blank line precedes `## Project` unless the previous
  line is blank; a blank line follows the path when the next line is not blank.

### Adding a merge request (`append_mr_entry`)

1. If `### Merge requests` is missing:
   - if `## Related` is missing, create it: insert `## Related` and a blank line
     before `## Progress` when that exists, else append a blank line and
     `## Related` at the end;
   - then insert `### Merge requests` and a blank line before the first `## `
     heading after `## Related`, or, when Related is the last section, append
     a blank line and `### Merge requests` at the end.
2. Insert `- [title](url)` after the last `- ` line of the subsection, else a
   blank line and the entry right after the subsection heading.

### Setting status or priority (`cmd_set`)

- With a `## Tags` section: on the first line of the section containing `#`,
  remove every tag of the same kind (all status tags, or all of `#A #B #C`),
  then append the new tag at the end of the line. Topic tags keep their order.
- Without a `## Tags` section: append a blank line, `## Tags`, a blank line and
  `#value` at the end of the file. Note this does not insert before Progress;
  **TBD** whether `tasq` keeps this exact behaviour or uses the general
  insertion rule.

### Marking done (`cmd_done`)

1. Optionally append the final note to Progress.
2. Flip the title line to `# [x]`.
3. Remove the status tag. If the tags line becomes empty, the line is dropped;
   the `## Tags` heading stays.

### Lossless editing

`tasq` parses a file into a task plus a document that keeps unknown sections,
unknown lines, ordering and blank lines verbatim. Writing back touches only the
sections it changed. For any file produced by the script,
`write(parse(x)) == x` byte for byte, and `parse(write(parse(x))) == parse(x)`
for every file. Unknown sections are never reordered or reformatted.

## Open points

- Filename collision rule (see "File and identity").
- CRLF handling on write.
- Non-ISO due values in existing files.
- Labelled links in the top-level Related list.
- Duplicate status or priority tags.
- Exact `## Source` grammar for id-only origins and multiple origins.
- Placement and schema of `<!-- tasq: ... -->` comments.
- Whether `set` without a Tags section inserts before Progress or at the end.
