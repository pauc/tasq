# Task file format

**Status:** normative (2026-10-04). Derived from plan section 4.4 and from what
`original/tasks` reads and writes; the open points of the first draft were
settled while implementing the format (T-102/T-103), the store (T-203) and the
sources (T-501), and are recorded here. The two points still marked **TBD** are
not implemented; do not infer behaviour for them from this document.

A task is one markdown file in an nb notebook. nb, the old `tasks` script and
`tasq` all read and write the same files. `tasq` must preserve anything it does
not understand (see "Lossless editing").

## File and identity

- Filename: `<stamp>.todo.md`, where `<stamp>` is nb's timestamp
  (`YYYYMMDDHHMMSS`). When that name is taken the stamp is bumped one second at
  a time, as nb does; after 60 attempts `create` fails with nothing written.
- Id: the 1-based line number of the filename in the notebook's `.index` file.
  Ids are positional and can change after deletions or `nb index reconcile`.
- Index lines that are not `*.todo.md`, or whose file no longer exists, are
  skipped when listing.
- Encoding UTF-8. New files use LF. In an existing file every line keeps its
  own ending when the file is rewritten, and lines `tasq` adds use the ending
  of the first line, so CRLF and even mixed endings survive an edit.
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

Reading takes the first non-empty line. `tasq` writes ISO dates `YYYY-MM-DD`
(and accepts `today`, `tomorrow`, `yesterday` on input). The old script stored
whatever string it was given, so existing files may contain other shapes: a
value that is not an ISO date gives `due = None` in the model, and the raw line
is preserved in the document, so a round trip does not touch it.

### Related

A list of `- <url>` lines, one link per line. `- [label](url)` is accepted
here too (the same link parser serves both lists); `tasq` writes bare URLs in
the top-level list, as the script did, and labelled links only under
`### Merge requests`.

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
them. When two status tags or two priority tags are present the last one seen
wins, as in the script. A tag with two leading hashes (`##A`, `##ready`) is a
topic tag, because the script's patterns required exactly one `#`.

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
origin `tasq sync` uses for reconciliation, as one line
`<source-name>: <external-id> [<url>]`, for example
`gitlab-review-requests: group/project!123 https://host/group/project/-/merge_requests/123`.
When the only value after the colon is an `http(s)://` URL it serves as both
id and URL. `Document::from_task` writes the section after `## Due`. One origin
per task is read; **TBD:** more than one origin per task.

### HTML comments (new, optional)

`<!-- tasq: {...} -->` is reserved for metadata the model needs but humans
should not see. **TBD:** placement and JSON shape; nothing writes or reads such
comments today, and like any unknown line they are preserved verbatim.
Renderers hide them; the old script and nb ignore them.

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
  `#value` at the end of the file. This does not insert before Progress; `tasq`
  keeps the script's behaviour so both tools produce the same file.

### Marking done (`cmd_done`)

1. Optionally append the final note to Progress.
2. Flip the title line to `# [x]`.
3. Remove the status tag. If the tags line becomes empty, the line is dropped;
   the `## Tags` heading stays.

### Edits the script never made (`tasq` only)

The TUI's edit form (`e`) and `tasq apply` can change the title, the due
date, the project and the topic tags of a task. The script had no command for
any of these, so the rewrite rules are `tasq`'s own; they follow the shape of
the operations above.

- **Title** (`set_title`): line 1 is rewritten with the new title after the
  same `# [ ] ` or `# [x] ` marker. Nothing else moves.
- **Due** (`set_due`): writes the ISO date like `set_project` writes a path:
  every `## Due` body becomes a blank line and the date, followed by a blank
  line when another heading follows. A missing section is inserted after
  `## Project`, else after `## Description`, else right after the title line,
  with the blank lines `set_project` would put around it.
- **Clearing due or project** (`clear_due`, `clear_project`): every `## Due`
  (or `## Project`) section is removed, heading and body up to the next `## `
  heading, so the line before the heading is followed by what came after the
  section. When the removed section was the last one, the blank lines left at
  the end of the file go too. A done or an open task is the same here.
- **Topic tags** (`set_tags`): on every `#` line of `## Tags` the topic tags
  are removed and the status and priority tags keep their places; the new
  topic tags go at the front of the first `#` line, the order create wrote
  (`#gitlab #A #ready`). A line left empty is dropped, the heading stays.
  Without a `#` line, a blank line and the tags are inserted after the
  heading; without a `## Tags` section, one is appended at the end of the
  file like `set` does. No tags and no section changes nothing.

### Lossless editing

`tasq` parses a file into a task plus a document that keeps unknown sections,
unknown lines, ordering and blank lines verbatim. Writing back touches only the
sections it changed. For any file produced by the script,
`write(parse(x)) == x` byte for byte, and `parse(write(parse(x))) == parse(x)`
for every file. Unknown sections are never reordered or reformatted.

## Open points

- More than one origin per task in `## Source`.
- Placement and schema of `<!-- tasq: ... -->` comments (reserved, unused).

Settled while implementing (see the sections above): same-second filename
collisions bump the stamp; line endings are preserved per line; non-ISO due
values read as no due date and are kept verbatim; labelled links parse in the
top-level Related list; the last status or priority tag wins; `## Source` is
`name: id [url]`; `set` without a Tags section appends at the end of the file.
