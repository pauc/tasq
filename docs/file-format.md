# Task file format

**Status:** normative (2026-10-18, ADR 0026). A task is one markdown file in a
store directory: a frontmatter block that holds the id and the other scalar
fields, then a markdown body with the title and the list sections. This
document is the reference for the task file format.

`tasq` must preserve anything it does not understand (see "Lossless editing").
The frontmatter is tasq's own flat subset, parsed line by line; it looks like
YAML and simple blocks are valid YAML, but YAML compatibility is a convenience,
not a promise.

## Store and identity

- A store is a directory, `<data_dir>/<store.name>` (`docs/config.md`). A
  directory that does not exist is an empty store; the first write creates it.
  There is no marker file, no index and no cache.
- Task files are `<ULID>.md`: 26 uppercase Crockford base32 characters, the
  first at most `7`. The ULID's millisecond timestamp (UTC) is the task's
  creation time, shown in local time (`created` in JSON); its random part
  keeps two creates in the same millisecond apart. Nothing else is read from
  the name. A directory listing sorts by creation time.
- Every other non-dot file whose extension is `md` (any case) is read too. A
  file that parses as a task file is a task even when its name is not a
  ULID (it has no creation time); one that does not is skipped by reads and
  reported by `tasq doctor`. Dot files, non-`.md` files, directories (even one
  named `x.md`) and the `.tasq/` directory (it holds the write lock,
  `.tasq/lock`) are ignored.
- Id: the `id` key of the frontmatter, a positive integer. It is written once
  and never changes: `create` takes the highest id in the store plus one (`1`
  in an empty store). Ids are never reused while a higher one exists. Two
  files with the same id make that id ambiguous: any command naming it fails
  naming both files, `list` shows both, `doctor` reports them.
- Encoding UTF-8. New files use LF. In an existing file every line keeps its
  own ending when the file is rewritten, and lines `tasq` adds use the ending
  of the first line, so CRLF and even mixed endings survive an edit. A file
  that is not valid UTF-8 is read with the invalid bytes replaced, and a write
  that would change it is refused.

## A complete file

```
---
id: 12
status: in-progress
priority: A
rank: 1024
due: 2026-10-10
project: /home/me/code/tasks
source: gitlab
source_id: group/project!123
source_url: https://gitlab.example.invalid/group/project/-/merge_requests/123
tags: [gitlab, review-request]
---
# Full task

## Description

Free text, any number of lines.

## Related

- https://example.invalid/issue/1

### Merge requests

- [MR title](https://example.invalid/-/merge_requests/123)

## Progress

- 2026-10-04 10:15: created via tasq create
- 2026-10-04 11:40: note

## Worktrees

- /abs/path/to/worktree (`branch-name`)

## Sessions

- 2026-10-04 10:15: `session-id` — short description
```

## Frontmatter

### Grammar

```
file        = fence block fence blank* title-line body
fence       = "---" EOL                 ; exactly three dashes, nothing else
block       = line*                     ; every line up to the first fence
entry       = WS* key WS* ":" value EOL ; key: one word, no whitespace
value       = any text after the first ":", surrounding whitespace trimmed
title-line  = "# " title EOL            ; title non-empty
```

- Line 1 must be `---`; a file whose first line is anything else is not a
  task file. The block runs to the next line that is exactly `---`; with none,
  the file is a format error (`the frontmatter has no closing '---'`).
- A line of the block is an entry when it has a `:` and the text before the
  first `:`, trimmed, is non-empty and has no whitespace. Every other line
  (blank, a comment, free text, `: orphan`) is kept verbatim and ignored.
- The value is everything after the first `:`, trimmed, so a value may itself
  contain `: ` (`note: a: b` has the value `a: b`). There is no quoting, no
  escaping, no nesting and no multi-line value. An empty value (`project:`)
  reads as unset.
- When a key appears on more than one line, the first one is read.
- The block must have an `id` entry whose value is a positive integer written
  plainly (`0`, `007`, `+1`, `1.0` are refused); otherwise the file is a format
  error naming the line.
- After the closing fence, blank lines are allowed; the first non-blank line
  must be `# Title` with a non-empty title. A done task's title has no marker:
  `# [x] Title` is a title that starts with `[x]`.

### Keys

| Key | Value | Read as | Written |
|---|---|---|---|
| `id` | positive integer | the task id (required) | always, first |
| `status` | a workflow status name, or `done` | `done`: the task is done and has no status; a name the workflow does not know: no status | when the task has a status, or `done` when done |
| `priority` | `A`, `B` or `C` | `#A`, `#B`, `#C` too; anything else, or absent: `B` | always |
| `rank` | integer (`i64`) | anything else: no rank (ADR 0025) | when set |
| `due` | `YYYY-MM-DD` | anything else: no due date | when set |
| `closed` | `YYYY-MM-DD HH:MM`, local time | on a done task only; a bare date or any other text: no closing time | on a done task with a closing time |
| `project` | a directory path | the value as a path | when set |
| `source` | the origin's source name | an origin needs `source` and `source_id` | with the origin |
| `source_id` | the origin's external id | | with the origin |
| `source_url` | the origin's url | the url and nothing else | when the origin has a url |
| `tags` | `[a, b, c]` | the items between the brackets, split on `,` and trimmed, without `#`; items that are not valid tags are dropped; no brackets: no tags | when there are topic tags |

There is no `title` key (the title is the body's first line) and no `created`
key (the creation time is the file name's). A value that does not parse stays
in the file untouched and reads as unset, so a hand-edited typo never makes a
task unreadable; the next write of that key replaces it. Any other key is an
unknown key: kept, never read.

### Canonical order

A block written from scratch (`create`) has the keys in this order, with no
other lines and no blank line between the closing fence and the title:

```
id, status, priority, rank, due, closed, project, source, source_id, source_url, tags
```

### Editing the block

An edit changes the lines of the keys it writes and nothing else.

- **Set** a key that is present: its first line becomes `key: value` and every
  later line of the same key is removed, so after a write there is one line
  per key.
- **Add** a key that is absent: the line goes right after the last line whose
  key comes before it in the canonical order (`id` is always present, so there
  is always one). Keys outside the canonical order never move it.
- **Clear** a key: every line of the key is removed. `id` is never cleared.

What survives every edit, byte for byte: the order of the keys already
present, unknown keys and their values, lines that are not entries, blank
lines inside the block, the blank lines between the closing fence and the
title, line endings, and the whole body. Values are written as `Status`,
`A`/`B`/`C`, `YYYY-MM-DD`, `YYYY-MM-DD HH:MM`, the path as given, the integer,
and `[a, b]` for tags (items joined with `, `). Setting the tags to none
clears the key.

Closing a task sets `status: done` and `closed`. Reopening it removes
`status: done`, removes `closed`, and sets the status it is reopened to, if
any.

## Body

The body starts at the title line `# Title` and holds the list sections:

```
## Description
## Related
### Merge requests      (inside ## Related)
## Progress
## Worktrees
## Sessions
```

Sections are level-2 headings `## Name`, matched exactly (`## ` plus the name,
no trailing text). A section runs until the next line starting with `## `.
Each heading is followed by one blank line, then its content, then one blank
line before the next heading. A fresh file writes Description and Related when
they have content, Progress always (`- <now>: created via tasq create` unless
a note is given), then merge requests, Worktrees and Sessions through the
append rules below. Any other section (`## Notes`, or `## Due` and `## Tags`
written by hand) is an unknown section: kept verbatim, never read.

### Description

Free markdown. `tasq` treats it as opaque text. Several `## Description`
sections are read joined with a newline.

### Related

A list of `- <url>` lines, one link per line. `- [label](url)` is accepted
here too (the same link parser serves both lists); `tasq` writes bare URLs in
the top-level list and labelled links only under `### Merge requests`.

#### `### Merge requests`

A level-3 subsection inside `## Related`. Each entry is `- [title](url)`. The
subsection ends at the next line starting with `##` (that is, any `##` or
`###` heading). An MR is already tracked when `(url)` occurs in the subsection.

### Progress

Entries `- YYYY-MM-DD HH:MM: note`, local time. Legacy entries `- YYYY-MM-DD:
note` (no time) must parse. The latest note is the last `- ` line. Note text is
a single line.

### Worktrees

Entries `- /abs/path` or ``- /abs/path (`branch`)`` with the branch in backticks
inside parentheses. The path contains no whitespace. A worktree is already
tracked when an entry of a `## Worktrees` section has the same path.

### Sessions

Entries ``- YYYY-MM-DD HH:MM: `session-id` `` optionally followed by
` — description` (space, U+2014 EM DASH, space). A session is already tracked
when an entry of a `## Sessions` section has the same id.

### HTML comments (reserved)

`<!-- tasq: {...} -->` is reserved for metadata the model needs but humans
should not see. **TBD:** placement and JSON shape; nothing writes or reads such
comments today, and like any unknown line they are preserved verbatim.

## Editing the body

These rules come from the original script's helpers.

### Appending an entry to a list section (`append_to_section`)

Used for Progress, Worktrees and Sessions.

1. If the section is missing:
   - if the section is not Progress and `## Progress` exists, insert `## Name`
     and a blank line immediately before `## Progress`;
   - otherwise append a blank line and `## Name` at the end of the file.
2. If the section has at least one `- ` line, insert the entry right after the
   last one.
3. Otherwise insert a blank line and the entry right after the heading.

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

### Title and description

- **Title** (`set_title`): the title line is rewritten as `# <title>`. Nothing
  else moves.
- **Description** (`set_description`, `clear_description`): the first
  `## Description` body becomes a blank line, the lines of the text and a
  blank line before the next heading; any further `## Description` section
  is removed, because reading joins them and only one can round-trip. A
  missing section is inserted right after the title line. The text has no
  blank lines at either end (reading trims them, so they could not
  round-trip). An empty description removes the section, like clearing.

## Lossless editing

`tasq` parses a file into a task plus a document that keeps the block, unknown
keys, unknown sections, unknown lines, ordering and blank lines verbatim.
Writing back touches only the lines it changed: `render(parse(x)) == x` byte
for byte for every file that parses, CRLF included, and
`parse(render(parse(x))) == parse(x)`. An update applies one operation per
changed field, re-reads the result and refuses the write
(`StoreError::Unsupported`) when the task still differs from the one asked
for, rather than drop a change silently.

## Open points

- More than one origin per task.
- Placement and schema of `<!-- tasq: ... -->` comments (reserved, unused).

