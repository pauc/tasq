# Plan: store v2, frontmatter, a tasq-owned directory and ULID file names

Task: tasq 56 ("Store v2: frontmatter, a tasq-owned directory and random file names"). Decision
record: [ADR 0026](../../../docs/adr/0026-store-v2-frontmatter-tasq-directory-ulid-files.md),
superseding [ADR 0024](../../../docs/adr/0024-format-v2-frontmatter-and-stable-ids.md). Terms:
[CONTEXT.md](../../../CONTEXT.md) (Store, Task file, Id, Frontmatter). Progress:
[PROGRESS.md](PROGRESS.md). Started 2026-10-18.

## Problem Statement

Tasks live in an nb notebook, in nb's own shape, and the user has stopped running nb on it.
Three things about that shape hurt now:

1. A task's id is the line number of its file in nb's `.index`. Ids move when the index is
   rebuilt, `tasq doctor` warns about it on every run, and `tasq store reconcile` exists only to
   keep the index honest.
2. A task file is named after the second it was created in. Two creates in the same second,
   which an agent or a loop produces routinely, pick the same name; the loser fails
   (task 47: 9 of 50 parallel creates failed).
3. Every scalar field is a markdown section with its own placement rule (`## Project` after
   `## Description`, `## Due` after `## Project`, status and priority as `#tags` next to topic
   tags, a new `## Tags` at end of file). Each new field needs a new rule, and most of the
   format code exists to reproduce them.

On top of that, every write goes through git: a lock under `.git`, a background job that
commits, pulls and pushes, and no lock at all when the notebook is not a repository. The user
wants the store correct on its own and sync designed later as a feature.

## Solution

`tasq` owns its storage. A store is a directory under the data directory, named by
`store.name`; its task files are `<ULID>.md`; each task file starts with a frontmatter block that
holds the id and the other scalar fields; the body keeps the title and the list sections. Ids
are short numbers written in the file, allocated sequentially under a lock in the store
directory, so `tasq set 47 ready` keeps working and never changes meaning. Creation time is the
ULID's timestamp. Nothing is committed to git.

`tasq migrate <notebook>` converts an nb notebook into a store once, keeping every id, and
prints the config to switch over. From then on nb is unreachable except through `migrate`;
the nb store crate stays only as `migrate`'s dependency until a follow-up task deletes it.

## User Stories

1. As a user, I want a task's id to stay the same for the life of the task, so that scripts,
   worktree names, session labels and my memory of "47" never go stale.
2. As a user, I want `tasq set 47 ready`, `tasq log 47 ...`, `tasq done 47` and every other
   id-taking command to work unchanged after the migration, so that nothing I do day to day
   changes.
3. As a user, I want two `tasq create` calls in the same second, or in parallel, to both
   succeed with distinct files and distinct ids, so that agents and loops can create tasks
   without retries.
4. As a user, I want to create a task when the store directory does not exist yet and have it
   created for me, so that a fresh machine needs no setup step.
5. As a user, I want `tasq list` on a store that does not exist yet to print nothing and exit
   zero, so that a fresh config is not an error.
6. As a user, I want my tasks in `~/.local/share/tasq/stores/<name>` by default, so that they
   sit with the rest of my application data and nothing points at `~/.nb`.
7. As a user, I want a top-level `data_dir` setting and `store.name`, so that I can keep more
   than one store and choose one per directory through `.tasq.toml` or a profile.
8. As a user, I want `TASQ_DATA_DIR` and `TASQ_STORE_NAME` to override the config, so that a
   script or a launched session can point `tasq` at a store without a config file.
9. As a user, I want a launched session (`tasq next`, `tasq pick`) to inherit the store I
   launched from, so that a nested `tasq` inside the session edits the same tasks.
10. As a user, I want `tasq store info` to print the resolved store directory, where the
    name and data directory came from, and the task count, so that an unexpectedly empty
    list is one command away from its explanation.
11. As a user, I want to open a task file in an editor and see its id, status, priority, rank,
    due, closed, project, source and tags as a small block at the top, so that the scalar
    fields are readable and editable in one place.
12. As a user, I want a done task to say `status: done` instead of `[x]` in the title, so
    that the title line is just the title.
13. As a user, I want the title heading, the description, the related links, the merge
    requests, the progress log, the worktrees and the sessions to stay in the body as markdown
    sections, so that they diff well and can be appended by hand.
14. As a user, I want every edit through `tasq` to keep everything in the file I did not ask
    to change, byte for byte (blank lines, line endings, unknown keys, unknown sections, key
    order), so that hand edits and tasq edits coexist.
15. As a user, I want `tasq view --json` and the TUI detail to show when a task was created,
    so that I can tell an old task from a new one.
16. As a user, I want a task's creation time to survive the migration, so that tasks created in
    2025 do not all look created today.
17. As a user, I want a directory listing of the store to sort by creation time, so that
    `ls` is useful without tasq.
18. As a user, I want `tasq migrate Tasq` to turn my notebook into a store with the same ids,
    print a mapping from id to file and title, and print the config lines to add, so that
    switching over is one command plus one config edit.
19. As a user, I want `tasq migrate --dry-run` to print the mapping and write nothing, so that I
    can check the conversion first.
20. As a user, I want `tasq migrate` to refuse a target directory that exists and is not
    empty, so that it never mixes two stores or overwrites one.
21. As a user, I want `tasq migrate` to never write to the nb notebook, so that I can go back
    if something is wrong.
22. As a user, I want done tasks migrated with their closed time, so that history survives.
23. As a user, I want a migrated notebook's merge requests, worktrees, sessions and progress
    entries to come through unchanged, so that nothing is lost.
24. As a user, I want `tasq doctor` to tell me when a file in the store has no frontmatter, no
    id, or an id that another file also has, naming the files, so that a bad hand edit or a
    bad copy is found rather than silently ignored.
25. As a user, I want `tasq list` to skip such files rather than fail, so that one broken
    file does not take the whole store down.
26. As a user, I want a stale lock left by a crashed `tasq` to be broken automatically after a
    short while, and reported by `tasq doctor` meanwhile, so that a crash never wedges the
    store.
27. As a user, I want an edit to a task that changed on disk since I read it to fail with a
    clear conflict message rather than overwrite the change, so that concurrent edits are
    safe.
28. As a user, I want `tasq apply` on a document from `tasq view --json` to work on a v2 store
    exactly as before, so that multi-field edits keep working.
29. As a user, I want the removed commands (`tasq store sync`, `tasq store reconcile`) and
    the removed config keys (`store.kind`, `store.notebook`, `store.sync_interval`) to be
    rejected with a message that names the replacement or the reason, so that an old config or
    an old script fails loudly.
30. As a user, I want `tasq config show` and `docs/config.md` to document `data_dir` and
    `store.name` with their defaults and env variables, so that the new layout is
    discoverable.
31. As a user, I want `docs/file-format.md` to describe the v2 task file as the normative
    format, including the exact frontmatter subset, so that other tools and agents can read
    and write task files.
32. As a user, I want `tasq doctor` to stop warning that ids are positional, so that the
    output is clean on a v2 store.
33. As a maintainer, I want the frontmatter parsed with no YAML library, so that the core crate
    stays dependency-free, no-I/O and cheap to mutation test.
34. As a maintainer, I want the store crate to run with injected clock and environment like
    the rest of the workspace, so that its tests run thousands of times against temp
    directories.
35. As a maintainer, I want the nb-specific code to be reachable only from `migrate` after
    this task, so that deleting it afterwards is a mechanical removal.
36. As an agent working in this repo, I want the issue tracker instructions and the plugin's
    prompts to no longer mention `.index` or positional ids, so that I do not warn about things
    that no longer exist.

## Implementation Decisions

### The task file (format v2)

- A task file is a frontmatter block followed by the body. The block is the first line `---`,
  then `key: value` lines, then a line `---`. A file whose first line is not `---` is not a v2
  task file.
- The frontmatter is a flat subset: one key per line, the value is the rest of the line after
  `: ` with surrounding whitespace trimmed, no nesting, no quoting, no multi-line values. A
  list is one line `[a, b, c]` with comma-separated bare items. Dates and numbers are bare.
  Nothing but tasq parses the block; YAML compatibility is a convenience, not a promise.
- Keys: `id` (integer, required), `status` (a workflow status name, or `done`), `priority`
  (`A`, `B`, `C`), `rank` (integer), `due` (`YYYY-MM-DD`), `closed` (`YYYY-MM-DD HH:MM`),
  `project` (a path), `source`, `source_id`, `source_url` (the origin, flat), `tags` (a list of
  topic tags without `#`). Absent means unset. No `created` key, no `title` key.
- A freshly written block has the keys in that order. An edited block keeps the file's key
  order, its unknown keys, and its blank lines. A key is set by replacing its line, added at
  its position in the canonical order among the known keys present, and cleared by removing
  its line.
- `status: done` is the only encoding of done. The `[ ]`/`[x]` marker is gone from the title
  line, which is `# <title>`. The model keeps `done` as a bool and the invariant that a done
  task has no workflow status; the format maps `status: done` to and from that pair. `done`
  stays a reserved word the workflow refuses as a status name.
- The body is the title heading and the list sections, unchanged from v1: `## Description`,
  `## Related` with `### Merge requests`, `## Progress`, `## Worktrees`, `## Sessions`. Their
  insertion rules relative to each other stay; the scalar sections (`## Project`, `## Due`,
  `## Source`, `## Closed`, `## Rank`, `## Tags`) do not exist in v2 and are unknown sections
  if present.
- The document model gains a frontmatter region in front of the body. Parse and render stay
  lossless: `render(parse(x)) == x` for every v2 file, including CRLF files.
- The reader picks v1 or v2 by the first line. v1 stays in core until the nb crate is deleted.
- The typed projection reads `id` from the frontmatter for v2 files; the store no longer
  supplies it. The projection also carries the creation time when the caller knows it (the
  store derives it from the file name); it is absent for v1 files.

### The store

- A store is a directory, `<data_dir>/<store.name>`. `data_dir` is a top-level config key,
  default `$XDG_DATA_HOME/tasq/stores` (`$XDG_DATA_HOME` defaults to `~/.local/share`), `~/`
  expanded. `store.name` defaults to `default`. Env overrides: `TASQ_DATA_DIR`,
  `TASQ_STORE_NAME`.
- A directory that does not exist is an empty store. The first write creates it. No marker
  file, no index, no cache, no git.
- Task files are `<ULID>.md`, 26 uppercase Crockford base32 characters. The ULID is minted at
  create time from the clock (so tests are deterministic) and a random part. The creation
  time of a task is the ULID's timestamp, shown in local time. Nothing else is read from the
  name.
- Any other `.md` file in the directory that is not a v2 task file (no frontmatter, no `id`)
  is skipped by reads and reported by doctor. Non-`.md` files and the `.tasq/` directory are
  ignored.
- Ids are positive integers. On create, the id is the highest id present plus one (`1` in an
  empty store). Ids are never reused. On migrate, the id is the nb id.
- The id map (id to path) is built by reading the frontmatter of every task file on each
  command. Two files with the same id make that id ambiguous: `get`, `update` and every
  command naming it fail with an error that names both files; `list` returns both; doctor
  reports it.
- A write (create, update) takes the store lock, `.tasq/lock` inside the store directory,
  for its whole duration: read the id map, allocate, write, release. The lock is an exclusive
  create; a holder that is stale (older than two minutes) is removed; a waiter polls. There is
  no hand-over to any job. Reads never lock.
- Create writes to a temporary file in the store directory and renames it into place without
  overwriting; on "already exists" it mints a fresh ULID and tries again, a bounded number of
  times.
- Update keeps the revision check: a revision (mtime, length, hash) captured on read must
  match on write, otherwise `Conflict`. The diff-and-apply path (whole task in, list of ops
  out, re-parse and compare, `Unsupported` when the result still differs) moves from the nb
  crate to the new crate and is used as is.
- `describe()` reports the store name `tasq`, the directory, the count, stable ids, and that
  ids never change on reconcile.
- Doctor checks: every `.md` file parses as a v2 task file with an id; no duplicate ids; no
  stale lock. Each finding names the file(s). The positional-id warning and every git check
  go.
- The new crate is `tasq-store` at `crates/store`. The revision and diff modules move there
  from the nb crate; the nb crate depends on the new crate for them until it is deleted. The
  nb crate's bookkeeper, job, git, which, index, resolve and native modules are not touched.

### The CLI and config

- The CLI always opens a tasq store. `store.kind`, `store.notebook`, `store.sync_interval`
  and `TASQ_NOTEBOOK` are removed from config; a config file that still sets them fails the
  unknown-field check with file, line and column like any unknown key, and the error text for
  these three names the replacement (`data_dir` / `store.name`, or "removed, see ADR-0026").
- `tasq store sync` and `tasq store reconcile` are removed. `tasq store info` stays and prints
  the resolved directory and the provenance of `data_dir` and `store.name`. There is no
  `tasq store init`.
- Launched sessions receive `TASQ_DATA_DIR` (the resolved absolute directory) and
  `TASQ_STORE_NAME` instead of `TASQ_NOTEBOOK`.
- `created` appears in the JSON document of a task (ISO local time, absent when unknown) and in
  `tasq view` and the TUI detail.
- `tasq migrate <notebook> [--to <dir>] [--dry-run]`: `<notebook>` resolves the way
  `store.notebook` did (a name is `$NB_DIR/<name>`, default `~/.nb/<name>`; a value containing
  `/` is a directory). `--to` defaults to the resolved store directory. The target must not
  exist or must be empty. Every task of the notebook, open and done, becomes a v2 task file
  with the same id, a ULID minted from the nb file's timestamp (the second from the file name,
  falling back to the first progress entry's time, then to now), `status: done` plus `closed`
  for done tasks, and the body sections copied. Output: one line per task (id, file name,
  title) then the config lines. The notebook is opened read-only; nothing in it is written,
  no git command runs anywhere.
- After this task the nb crate is a dependency of the CLI only for `migrate`.

### Docs and agent surfaces

- `docs/file-format.md` describes v2 as normative and keeps a short v1 section marked "read by
  `tasq migrate` only". `docs/config.md` documents `data_dir`, `store.name`, the env variables
  and the removals. `docs/json.md` adds `created`. `docs/testing.md` replaces the nb section's
  role with the temp-store pattern. `docs/agents/issue-tracker.md`, `CLAUDE.md` and the plugin
  prompts drop `.index` and positional ids.

## Testing Decisions

A good test exercises a seam from outside and asserts on observable results: the bytes of a
rendered file, the task returned by the trait, the exit code and output of the binary. No test
reads private state or asserts on how a result was computed. Exact-string assertions are
preferred over property checks where they cut mutants.

Three seams, all existing:

1. **The document round trip in core.** v2 fixture files next to the v1 ones (minimal, full,
   done with closed, ranked, source, CRLF, unknown keys, unknown sections, blank lines inside
   the block) that must parse and render byte-identical, a v2 arm in the existing format
   property test, and one test per frontmatter op (set, add in canonical position, clear,
   unknown key untouched). Prior art: the fixture, parse, ops and props test files of the core
   crate. Mutation testing over the format module with zero missed mutants is the acceptance
   bar for every ticket that touches it.
2. **The `Store` trait over a temp directory.** The new crate's integration tests build an
   empty temp directory and an injected clock, then exercise list, get, create, update, doctor
   and describe through the trait: empty store, first id is 1, ids are max plus one, parallel
   creates all succeed with distinct ids and files, a missing directory is empty and is created
   on write, update after an external change is a conflict, duplicate ids are ambiguous, a
   non-task `.md` is skipped and reported, a stale lock is broken, a fresh lock is waited on.
   Prior art: the nb crate's create, write, doctor and describe tests, without the nb
   environment.
3. **The CLI end to end.** `assert_cmd` plus `insta` snapshots in the CLI test file for
   `tasq store info`, `tasq doctor`, `tasq migrate` (dry-run and real, on a copy of the nb
   fixture notebook, asserting ids, file count, the printed mapping and that the notebook is
   unchanged), the config keys and env variables, the removed commands and keys, and the
   `created` field in `view --json`. Prior art: the existing CLI snapshot tests and their
   support module.

Every cargo invocation goes through the memory guard; one build at a time; nb-gated tests are
irrelevant to the new crate and its tests never call `nb` or `git`.

## Out of Scope

- Git: commits, the background job, pull and push, `tasq store sync`, the failed-job warning,
  and duplicate ids across machines. Task 57 ("Git sync as an opt-in feature for tasq
  stores"), blocked by this one.
- Deleting the nb crate, the v1 reader, the nb fixtures and the parity tests. A new task
  created when this one lands.
- Done as a real workflow status (a done status group, `--status done` filters). The model
  keeps done as a flag.
- An id cache, a marker file, `tasq store init`.
- Changing the body sections, their rules, or the JSON shape beyond adding `created`.
- The TUI beyond showing `created` in the detail pane.
- Making the retry on a same-second collision work in the nb store (task 47 closes with this
  task instead).

## Further Notes

- Both readers coexist in core only for `migrate`; no ordinary edit ever converts a v1 file,
  and the tasq store refuses to list one.
- The frontmatter subset is documented as tasq's own; a `: ` inside a value (a project path on
  Windows, say) would confuse a strict YAML reader but not tasq, which splits on the first
  `: ` only.
- The stores root convention replaces the marker file's safety role: a mistyped name creates a
  harmless empty directory under `~/.local/share/tasq/stores` instead of writing into an
  arbitrary folder.
- Task 2 ("one-week side-by-side run with the tasks script") becomes obsolete and is closed
  when this task lands.
- The user's own notebook has 54 tasks; migrate output is expected to be 54 lines.

## Sub-tasks

Tracer-bullet slices, blockers first. Work the frontier: any sub-task whose blockers are done.
The CLI switch (SV-009) is a wide refactor (every CLI test and snapshot runs on the nb fixture
notebook, ten command files name the nb store type), so the sequence is expand, migrate,
contract: two prefactors, the format and the store built and verified at their own seams, the
fixture store produced by migrate itself, then the CLI switched in one batch. Each sub-task
closes with a mutation run on the modules it touched (zero missed) and a row update in
PROGRESS.md. Statuses: `in-progress`, `ready`, `waiting`, `blocked`, `later`.

### SV-001: the CLI talks to the `Store` trait; `store sync` and `store reconcile` go (prefactor)

**What to build:** every command opens the store as a trait object and never names the nb store
type. `tasq store sync`, `tasq store reconcile` and `store.sync_interval` are deleted with their
tests and docs lines; the nb background commit after a write is untouched. Nothing a user sees
changes except the two commands being gone.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] No command module names the nb store type; `App::open_store` is the one construction site and returns a `Box<dyn Store>`.
- [x] `tasq store sync` and `tasq store reconcile` are unknown subcommands; `store.sync_interval` is an unknown key.
- [x] Every existing CLI test and snapshot is green on the nb fixture notebook.
- [x] Zero missed mutants in the touched code.

### SV-002: `tasq-store` crate skeleton, revision and diff moved out of store-nb (prefactor)

**What to build:** the new crate exists in the workspace with the revision check (mtime, length,
hash) and the diff-and-apply module moved in, with their tests. store-nb depends on the new crate
for both and behaves exactly as before.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] The workspace has `tasq-store`; store-nb's revision and diff come from it.
- [x] Every store-nb test, including the nb-gated ones, is green.
- [x] The moved tests run in the new crate.

### SV-003: core reads and renders a v2 task file

**What to build:** a file whose first line is `---` parses to a document with a frontmatter
region and a body, and projects to a `Task` whose id comes from the frontmatter, whose `status:
done` means done with no workflow status, and whose creation time is the one the caller passes
(absent for v1). Rendering gives the original bytes back: line endings, blank lines, unknown keys,
key order, unknown sections. v1 files parse as before.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] v2 fixtures (minimal, full, done with closed, ranked, source, CRLF, unknown keys, blank lines inside the block, unknown sections) parse and render byte-identical.
- [x] A v2 file with no `id`, a non-integer `id`, or no closing `---` is a format error naming the line.
- [x] `status: done` projects to done with no status; a workflow status projects as today; `tags` projects to topic tags without `#`.
- [x] The format property test has a v2 arm.
- [x] Zero missed mutants in the format module.

### SV-004: core writes v2: new documents and one op per key

**What to build:** a `Task` becomes a canonical v2 document (fixed key order, body sections as
today), and each scalar key can be set, added in canonical position among the keys present, or
cleared on an existing document without touching any other line. Closing a task writes `status:
done` and `closed`; reopening writes the status and clears `closed`.

**Blocked by:** SV-003.

**Status:** done

- [x] A fresh document for every fixture task renders the canonical block and round-trips to the same `Task`.
- [x] Setting, adding and clearing each of the eleven keys leaves every other line byte-identical; adding puts the key in canonical order among the known keys present.
- [x] Unknown keys and blank lines in the block survive every op.
- [x] Zero missed mutants in the format module.

### SV-005: the tasq store reads a directory

**What to build:** `list`, `get` and `describe` through the `Store` trait over a directory of
`<ULID>.md` files: the id map is built from the files' frontmatter on every call, the creation
time comes from the ULID, an id claimed by two files is an error naming both, a `.md` file that
is not a v2 task file is skipped, and a directory that does not exist is an empty store.

**Blocked by:** SV-002, SV-003.

**Status:** done

- [x] `list` on an absent directory is empty and creates nothing; `describe` reports it.
- [x] `get` returns the task whose frontmatter id matches, with `created_at` equal to the ULID's time.
- [x] Two files with the same id: `get` and any id-taking call fail naming both files; `list` returns both.
- [x] A non-task `.md`, a non-`.md` file and the `.tasq/` directory are ignored by reads.
- [x] `describe` reports stable ids and that ids never change on reconcile.
- [x] Zero missed mutants in the touched code.

### SV-006: create under the store lock

**What to build:** `create` allocates the highest id plus one (1 in an empty store) under
`.tasq/lock`, mints a ULID from the injected clock, writes the file no-clobber and retries with
a fresh ULID when the name exists, creating the directory on the first write. Parallel creates
all succeed. A stale lock is broken, a live one is waited on.

**Blocked by:** SV-004, SV-005.

**Status:** done

- [x] Creating in an absent directory creates it and the task gets id 1.
- [x] Ids are highest-plus-one; a deleted file's id is not reused when a higher one exists.
- [x] Ten parallel creates produce ten files, ten distinct ids and no error.
- [x] The file name is a ULID whose time is the clock's; `get` reports it as `created_at`.
- [x] A lock older than the stale threshold is removed and the create proceeds; a fresh lock makes the create wait until it is released.
- [x] Zero missed mutants in the touched code.

### SV-007: update with conflict detection, and doctor

**What to build:** `update` takes the whole task under the lock, diffs it against the file,
applies the v2 ops, re-parses and refuses with `Unsupported` when the result still differs,
and fails with `Conflict` when the file changed since it was read. Doctor reports a stale lock,
naming the file (the non-task-file and duplicate-id checks landed with SV-005).

**Blocked by:** SV-006.

**Status:** done

- [x] Every scalar and list change made through `update` is visible on re-read and leaves unrelated lines untouched.
- [x] A change the ops cannot express is refused with `Unsupported` and writes nothing.
- [x] An external edit between read and write is a `Conflict`; the external edit survives.
- [x] Doctor on a clean store reports only `Ok` checks; on a store with a stale lock it reports it with the lock's path, next to SV-005's `task-file` and `ids` checks.
- [x] Zero missed mutants in the touched code.

### SV-008: migrate as a library, and the committed v2 fixture store

**What to build:** a function in the store crate that converts an nb notebook directory into a
store directory: same ids, a ULID minted from the nb file's timestamp (fallbacks: first progress
entry's time, then now), done tasks as `status: done` with `closed`, bodies copied. A planning
step yields the id-to-file mapping without writing. The nb fixture notebook is converted once
and committed as the v2 fixture store the CLI tests will use.

**Blocked by:** SV-006.

**Status:** done

- [x] Converting the nb fixture notebook yields one file per task with the nb id, and each file's ULID time equals the nb file name's second.
- [x] A done nb task becomes `status: done` with its closed time; an open one keeps its status and tags.
- [x] Re-reading the converted store through the trait gives tasks equal to the nb store's, field by field, plus `created_at`.
- [x] The plan step writes nothing; the target must be absent or empty, otherwise an error.
- [x] The v2 fixture store is committed and listed in the testing docs.
- [x] Zero missed mutants in the touched code.

### SV-009: the CLI opens the tasq store

**What to build:** `data_dir` (default `$XDG_DATA_HOME/tasq/stores`) and `store.name` (default
`default`), overridable by `TASQ_DATA_DIR` and `TASQ_STORE_NAME`; the CLI opens the tasq store
at `<data_dir>/<store.name>`; `store.kind`, `store.notebook` and `TASQ_NOTEBOOK` are gone and a
config that still sets them fails naming the replacement; the CLI test support builds a temp
copy of the v2 fixture store and every CLI test and snapshot is green on it.

**Blocked by:** SV-001, SV-007, SV-008.

**Status:** ready

- [ ] With no config, `tasq list` reads `~/.local/share/tasq/stores/default` (under the test `HOME`); `data_dir`, `store.name`, `.tasq.toml`, a profile and the two env variables override it in the documented precedence.
- [ ] `store.kind`, `store.notebook` and `store.sync_interval` in a config file fail with file, line, column and a message naming `data_dir` / `store.name` or ADR-0026.
- [ ] Every CLI test and snapshot is green on the v2 fixture store; no CLI code path references store-nb except `migrate`'s future entry point.
- [ ] `tasq store info` prints the resolved directory.
- [ ] Zero missed mutants in the touched code.

### SV-010: creation time and store provenance surfaces

**What to build:** `created` in `view --json`, `tasq view` and the TUI detail; `store info` and
`doctor` print the resolved directory with the provenance of `data_dir` and `store.name` and no
positional-id warning; launched sessions receive `TASQ_DATA_DIR` (resolved, absolute) and
`TASQ_STORE_NAME`.

**Blocked by:** SV-009.

**Status:** blocked

- [ ] `view --json` carries `created` as ISO local time; absent when unknown; `tasq view` and the TUI detail show it.
- [ ] `store info` shows `data_dir` and `store.name` with where each was set.
- [ ] `doctor` on a clean store prints no warning about ids.
- [ ] A launched session's environment has `TASQ_DATA_DIR` and `TASQ_STORE_NAME` and not `TASQ_NOTEBOOK`.
- [ ] Zero missed mutants in the touched code.

### SV-011: `tasq migrate <notebook> [--to <dir>] [--dry-run]`

**What to build:** the command end to end: the notebook resolves like `store.notebook` did,
`--to` defaults to the resolved store directory, a non-empty target is refused, the mapping
(id, file, title) and the config lines are printed, `--dry-run` writes nothing, the notebook is
never written and no git command runs.

**Blocked by:** SV-008, SV-009.

**Status:** blocked

- [ ] On a copy of the fixture notebook, `tasq migrate home --to <tmp>` prints one line per task and the config lines; the notebook directory is byte-identical afterwards.
- [ ] `tasq list` on the new store shows the same ids and titles as the notebook did.
- [ ] `--dry-run` prints the same mapping and leaves the target absent.
- [ ] An existing non-empty target is refused before anything is written.
- [ ] Zero missed mutants in the touched code.

### SV-012: docs, agent surfaces and the hand-over

**What to build:** `docs/file-format.md` with v2 normative and v1 marked "read by `tasq migrate`
only", `docs/config.md`, `docs/json.md`, `docs/testing.md`, `docs/agents/issue-tracker.md`,
`CLAUDE.md` and the plugin prompts updated; task 47 and task 2 closed; the store-nb removal task
created, blocked by nothing once this task lands, and linked from task 56.

**Blocked by:** SV-010, SV-011.

**Status:** blocked

- [ ] No doc or prompt mentions `.index`, positional ids, `store.notebook`, `store sync` or `store reconcile` except the v1 section and the ADRs.
- [ ] The frontmatter subset is specified exactly (grammar, keys, values, canonical order, what survives an edit).
- [ ] Tasks 47 and 2 are done with a note pointing at 56; the removal task exists and is linked.
