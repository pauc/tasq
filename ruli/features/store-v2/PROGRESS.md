# Progress: store v2

Plan: [PLAN.md](PLAN.md). Task: tasq 56. ADR: 0026.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| SV-001 | CLI talks to the `Store` trait; `store sync` / `store reconcile` go (prefactor) | done | 2d527e5; `App::open_store` returns `Box<dyn Store>`; `Store` gained `take_warnings` and `doctor` (required); `Check` moved to `tasq_core::doctor`; 40 mutants in the diff, 0 missed |
| SV-002 | `tasq-store` crate skeleton, revision and diff moved (prefactor) | done | 7920eb3; `crates/store` (`tasq-store`, deps: core only); `revision.rs` and `diff.rs` moved with their 16 unit tests, store-nb re-exports `Revision`, `diff`, `revision`; `mutants-in crates/store/src`: 44 mutants, 42 caught, 2 unviable, 0 missed (two tests added) |
| SV-003 | Core reads and renders a v2 task file | done | 710f1ff, c1d2bf1; `Document` keeps the block as a `front` region ahead of the body, `format::parse_v2(text, created_at, workflow)`, `Document::{frontmatter, id}`, `format::key`, `Task::created_at`; nine v2 fixtures under `crates/core/tests/fixtures/v2/`, `format_v2.rs` (23 tests), a v2 arm in `format_props.rs`; `mutants-in crates/core/src/format`: 385 mutants, 347 caught, 38 unviable, 0 missed |
| SV-004 | Core writes v2: new documents and one op per key | done | dba6a08; `Document::from_task_v2`, `key::ORDER`, `ops::{set_key, clear_key, set_origin, clear_origin}`, every scalar op dispatches on the shape; `diff::apply` writes the origin on v2; `format_v2_write.rs` (28 tests), two v2 props; `mutants-in crates/core/src/format`: 415 mutants, 376 caught, 39 unviable, 0 missed (one `||` survivor in `clear_origin` fixed by reshaping it to a `map` over the three keys); `mutants-in crates/store/src`: 45 mutants, 0 missed |
| SV-005 | The tasq store reads a directory | done | 5e1226b, 7e222fc; `TasqStore::new(dir, workflow)` (`crates/store/src/store.rs`), `tasq_store::ulid::Ulid`, `StoreError::AmbiguousId`; `tests/read.rs` (12 tests) over a temp directory; `mutants-in crates/store/src`: 127 mutants, 108 caught, 19 unviable, 0 missed; `--in-diff main` (before the review follow-ups): 276 mutants, 0 missed |
| SV-006 | Create under the store lock | ready | |
| SV-007 | Update with conflict detection, and doctor | blocked (SV-006) | |
| SV-008 | Migrate as a library, and the committed v2 fixture store | blocked (SV-006) | |
| SV-009 | The CLI opens the tasq store | blocked (SV-001, SV-007, SV-008) | |
| SV-010 | Creation time and store provenance surfaces | blocked (SV-009) | |
| SV-011 | `tasq migrate <notebook> [--to] [--dry-run]` | blocked (SV-008, SV-009) | |
| SV-012 | Docs, agent surfaces and the hand-over | blocked (SV-010, SV-011) | |

## Learnings

- 2026-10-18: design settled by grilling (four rounds); ADR 0026 and the `CONTEXT.md` terms
  written and committed (868a1c7). Git is out of the store entirely (task 57 brings it back as opt-in); ULID file names
  carry the creation time so there is no `created:` key; no marker file, no `store.kind`, no
  `store init`; stores live at `<data_dir>/<store.name>`.

- 2026-10-18 (SV-001): the CLI needed three things from `NbStore` that the trait did not
  have: the open/after-write warnings, the doctor checks and the bookkeeper name. They are
  now `Store::take_warnings(&mut self) -> Vec<String>`, `Store::doctor(&self) -> Vec<Check>`
  (`Check`/`CheckStatus`/`any_failed` moved to `tasq_core::doctor`, re-exported by
  `tasq_store_nb::doctor`) and `StoreInfo::bookkeeper: Option<String>` (skipped in JSON when
  `None`; `store info` keeps it top-level so its JSON is unchanged). The two methods are
  required, not defaulted: a default body returning `Vec::new()` has the equivalent mutant
  `vec![]`, which cargo-mutants reports as missed. `MemoryStore` implements them for real (a
  `warn` queue, one `memory` check with the task count) so the test doubles in `edit.rs`,
  `source.rs` and the TUI forward to it.
  `impl<S: Store + ?Sized> Store for Box<S>` was needed because `&mut Box<dyn Store>` does not
  coerce to `&mut dyn Store` (rustc tries the unsizing coercion first and asks for
  `Box<dyn Store>: Store`); a recording-store test checks every forwarded method.
- 2026-10-18 (SV-001): `tasq store reconcile` was the setup step of seven CLI tests that
  place a task file by hand; `TestEnv::register(name)` (appends the name to `.index`) is the
  replacement. The two fix texts that named the removed commands changed: the index fix is
  now "edit .index by hand: blank the line of a file that is gone, append a file that is not
  listed" (what reconcile did, ids kept), a file written but not registered says "append it
  to .index by hand", the job fix "check 'git status' in the notebook and commit by hand".
- 2026-10-18 (SV-001): removing `store.sync_interval` would have silently stopped every
  notebook's pull and push. The CLI pins the old default instead
  (`tasq_store_nb::DEFAULT_SYNC_INTERVAL`, 3600, in `App::store_options`), so the nb store
  syncs hourly as before and only the knob is gone; task 57 removes the constant. The error
  for a config that still sets the key is the generic unknown-field one; the text naming the
  replacement is SV-009's bullet.
- 2026-10-18 (SV-001): mutation run `cargo mutants --jobs 2 --in-diff <git diff main>`:
  40 mutants, 26 caught, 14 unviable, 0 missed.

- 2026-10-18 (SV-002): `tasq-store` sits between core and store-nb in the dependency
  order: store-nb's `Cargo.toml` takes it as a path dependency and its `lib.rs` re-exports
  `tasq_store::{Revision, diff, revision}`, so `crate::revision::Revision` and `crate::diff` in
  `store.rs` resolve unchanged and nothing outside the nb crate moved (the CLI never named
  either). The release workflow publishes `tasq-store` right after `tasq-core`, the mutants
  config examines `crates/store/**`, and the architecture, release, testing and contributing
  docs list the crate. The new crate depends on `tasq-core` only (`tempfile` for tests); no
  `mutants`, `serde` or `chrono` until a slice needs them.
- 2026-10-18 (SV-002): `cargo mutants --in-diff` sees a `git mv` as a rename with no hunks,
  so it mutated nothing in the moved files (the 40 mutants it reported were SV-001's). For a
  move, run the `mutants-in` recipe on the new location. Doing so found five survivors in
  `diff::apply` that store-nb's integration write tests used to kill: the `&&` guarding the
  status op (an unchanged task whose status tag is not last would have had the tag moved to
  the end by `set_status`) and the four `==` in the list-append filters (with `!=`, a second
  entry next to an existing one is never appended). Two unit tests in `diff.rs` now pin
  both, so the module is self-sufficient when the nb crate goes.

- 2026-10-18 (SV-003): the frontmatter is a second run of lines (`Document::front`: opening
  fence, block, closing fence and the blank lines before the title) in front of the body, not
  an offset into one line buffer. The body is numbered from the title line in both shapes, so
  `Section::heading`, `Document::lines()`/`line_count()` and every list-section op in
  `format::ops` work on a v2 document unchanged; SV-004 adds the frontmatter ops next to them.
  `Document::parse` checks, in file order: the closing `---` (`UnterminatedFrontmatter`), the
  `id` key (`MissingId { line }` names the closing fence, `InvalidId { line, value }` the first
  `id:` line; the value is trimmed and must be a plainly written positive integer, so `0`,
  `007`, `+1` are refused), then the first non-blank line after the fence, which must be
  `# Title` (`MissingTitle { line, found }`). A `# [x] Title` in a v2 body is a title that
  starts with `[x]`; `status: done` is the only done marker, and `closed` is read on done tasks
  only, as `## Closed` was.
- 2026-10-18 (SV-003): `format::parse(text, id, workflow)` keeps its signature and reads both
  shapes; for a v2 file the frontmatter id wins and `id` is not used (the nb store and
  `diff::apply` call it unchanged). `format::parse_v2(text, created_at, workflow)` is the
  store's entry point: it refuses a v1 file with `FormatError::NotFrontmatter` and sets
  `Task::created_at`, which is `#[serde(skip)]` until SV-010 picks the JSON shape (it is not
  a field of the file, so `diff::differing_fields` never compares it). `parse_v2` checks the
  first line itself, so a v1 task file and any other markdown file are both `NotFrontmatter`
  (the review caught the first cut, which let a README through to `NotATask`); SV-005 skips
  and reports on that one variant.
- 2026-10-18 (SV-003): a value that does not parse (`priority: D`, `due: next friday`, a
  `status` the workflow does not know, `rank: 1.5`) reads as unset and stays in the file,
  the way an invalid `## Rank` did in v1; an empty value (`project:`) is unset too. `tags`
  must be bracketed (`[a, b]`), items are trimmed and anything `Tag::new` refuses is dropped.
  An origin needs `source` and `source_id`; `source_url` is the url and nothing else is
  (the v1 rule that a URL-shaped id doubles as the url was dropped on review: it made
  `url: None` unrepresentable for such an id, and SV-004's writer always writes
  `source_url` when there is one). The v1 scalar sections (`## Due`, `## Tags`, ...) are
  unknown sections in a v2 body and are not read (`extra-sections` fixture). The scalar ops in
  `format::ops` (`set_title`, `set_done`, `set_status`, ...) are v1-only until SV-004:
  `set_title` on a v2 document would write a `# [ ] ` marker; the store's re-parse compare
  turns that into `Unsupported`, and nothing calls them on a v2 document yet.

- 2026-10-18 (SV-004): the scalar ops kept their names and gained a shape check
  (`Document::is_v2`, the block is non-empty) at the top: `set_status`, `set_priority`,
  `set_rank`, `set_due`, `set_closed`, `set_project`, `set_tags`, `set_title`, `set_done`,
  `set_open`, `strip_status_tag` and the `clear_*` write a frontmatter key on a v2 document
  and the v1 section or tag otherwise. `diff::apply` therefore works on both shapes with no
  branch of its own: closing a v2 task is `set_done` (`status: done`) plus `set_closed`,
  reopening is `set_open` (drops `status: done`) plus `set_status` and `clear_closed`,
  which is the sequence the apply tests pin. The one new case in `apply` is the origin:
  `set_origin`/`clear_origin` are v2-only (no v1 command ever rewrote `## Source`), so on a
  v1 document they do nothing and `origin` is still reported as unsupported, as before.
- 2026-10-18 (SV-004): the primitives are `Document::set_key`/`clear_key` (crate-private)
  behind `ops::set_key`/`ops::clear_key`. Set replaces the first line of the key and removes
  any later duplicate, so a write leaves one line per key; a missing key goes after the last
  line whose key precedes it in `key::ORDER` (`id` is always present and first, so every
  known key has a predecessor; `map_or(1, ..)` is the unreachable top-of-block fallback), a
  key outside the order goes right before the closing fence. Clear removes every line of
  the key; `ops::clear_key` refuses `id`. The body, its trailing newline included, is not
  touched by a frontmatter op (the v1 "every awk rewrite terminates the file" rule does not
  apply to the block). The written values are `Status::as_str`, `Priority::as_str`,
  `format_date`, `format_timestamp`, `Path::display`, `i64` and `[a, b]`
  (`write::format_tag_list`).
- 2026-10-18 (SV-004): `Document::from_task_v2(task)` writes `id` and `priority` always
  (`priority: B` is written even though absent reads as `B`, so the block shows every field
  the model has), `status` when open with a status or `done` when done, `closed` on a done
  task only, `source_url` whenever the origin has one (also when it equals `source_id`),
  `tags` when non-empty; then `# Title`, Description, Related, Progress (always), and merge
  requests, Worktrees, Sessions through the append ops, which insert them before
  `## Progress` as on v1. No blank line between the closing fence and the title. The v1
  fixtures' tasks round-trip through a v2 document too, including the v1 `source` fixture
  whose url-shaped id became an explicit `source_url`.

- 2026-10-18 (SV-005): the read path is one function, `TasqStore::scan`: `read_dir`, keep
  the non-dot names whose extension is `md` (ignoring case, clippy's
  `case_sensitive_file_extension_comparisons`), sort them, read each one and classify the
  error (`NotFound` and `IsADirectory` are dropped: a file that vanished, a dangling symlink,
  a directory named `x.md`; anything else is `StoreError::Io` naming the file), then
  `format::parse_v2`. A parse failure of any variant is "not a v2 task file": skipped by
  `list`, `NotFound` to `get`, a `task-file` `Fail` in `doctor`. Reading first and checking
  the error, instead of `file_type()` then read, is what makes the vanished-file branch
  reachable from a test (a dangling symlink); with `file_type()` first it was a missed
  mutant. A symlink to a task file is a task file; `file_of` returns the link.
- 2026-10-18 (SV-005): `created_at` is the ULID's millisecond timestamp converted with
  `chrono::Local` (`store::local_time`), the one zone-dependent conversion in the store crate:
  the model's timestamps are local wall-clock naive times (`tasq_core::clock`), and the
  file name is UTC by the ULID spec. The tests compute the expectation with the same
  conversion from the known millisecond, so they hold in any `TZ`; SV-006 needs the inverse
  for minting. A `.md` file that parses as v2 but whose stem is not a ULID (uppercase
  Crockford, 26 chars, first at most `7`) is a task with `created_at: None`, not a skipped
  file: doctor's definition of a task file is "frontmatter with an id", and a stray copy
  (`x (copy).md`) is then reported as a duplicate id instead of silently ignored.
- 2026-10-18 (SV-005): `ulid::Ulid` is parse, `from_parts`, `timestamp` and `Display`, no
  crate. The spec's example `01ARZ3NDEKTSV4RRFFQ69G5FAV` decodes to 1469922850259 ms
  (2016-07-30 23:54:10.259 UTC), checked with an independent Python decode before it became
  the test constant (the first guess, 1469918176385, was the spec's monotonicity example).
  The base32 accumulation is `(value << 5) + digit`, not `| digit`: `|` has the equivalent
  mutant `^` (the low bits are zero after the shift), `+` has none.
- 2026-10-18 (SV-005): an id claimed by two files is `StoreError::AmbiguousId { id, paths }`
  (new in core, `"id 7 is claimed by more than one file: a.md, b.md"`) from `get` and
  `file_of`; `list` returns both tasks and queues one warning per such id (the store keeps
  `RefCell<Vec<String>>` because `list` takes `&self`), which also gives `take_warnings` a
  real test instead of the `Vec::new()`/`vec![]` equivalent-mutant problem of SV-001.
- 2026-10-18 (SV-005): `cargo mutants -E 'impl Display'` does not exclude a `impl
  fmt::Display for Ulid` block: the pattern matches the text as written in the source.
- 2026-10-18 (SV-005, for SV-007): `scan` reads a file with `String::from_utf8_lossy`, as the
  nb store does. An `update` that diffs against the lossy text and writes it back would
  replace invalid bytes with U+FFFD; SV-007 should refuse (or keep the bytes) rather than
  rewrite them. Reads are unaffected.

## Deviations

- SV-001's first criterion read "No file in the CLI crate names the nb store type". The
  construction site has to: `App::open_store` in `crates/cli/src/app.rs` is the one place that
  names `NbStore` and `NbStoreOptions`, and boxes the result. No command module names it.
  The criterion in PLAN.md was reworded to that shape.
- `-v` now prints `store: nb at <dir>` instead of `notebook: <dir>` (the generic code has
  only `describe()` to go on). `tasq doctor`'s index fix and the job-failure warning name no
  command (texts above). The `store` subcommand's help line and the top-level blurb no
  longer mention sync. Everything else a user sees is unchanged.
- Still reached through `tasq_store_nb` from the CLI, not the store type: `doctor::tool_check`
  and `which::find_in_path` (PATH lookups for optional tools). They need a home outside the nb
  crate before SV-009 makes it `migrate`'s dependency only.
- SV-004: the second criterion reads "each of the eleven keys"; `id` is set-only. `ops::set_key`
  replaces it, `ops::clear_key` refuses it (a v2 document always has one), and nothing adds it
  (the parse guarantees it). The plan's "added at its position in the canonical order among the
  known keys present" was read as "after the last line whose key precedes it in `key::ORDER`"
  (the other reading, before the first key that follows it, differs only on a hand-edited block
  whose keys are out of order); a duplicated key is collapsed to one line on set and every line
  goes on clear (the reader takes the first, so the result reads the same); a key outside
  `key::ORDER` goes at the end of the block. `set_key` writes the value as given (a line
  break, or an `id` that is not a positive integer, makes a file the parser refuses, like
  `set_title` with an empty title); the typed ops never pass one. `ops::set_key`/`clear_key`
  stay public with no production caller: the integration tests need them for `id` and for
  unknown keys, and `pub(crate)` would hide them from `crates/core/tests`.
- SV-004: `diff::apply` (store crate) gained the origin branch, which SV-007 would otherwise
  have added: the v2 ops only prove themselves composed through `apply` (close, reopen), and the
  origin was the one field with no op at all. On a v1 document `set_origin`/`clear_origin` do
  nothing and `origin` is reported as before.
- SV-004 review follow-ups (2ec89b2): the two writers share `push_section` and
  `with_appended_lists`, `format_tag_list` moved to `entry.rs` next to the other `format_*`
  helpers, `frontmatter_lines` is private. Kept: the `if doc.is_v2()` prelude in the thirteen
  scalar ops (one op per command is the module's rule; a dispatch helper would hide which key
  each writes), and the `map_or(1, ..)` top-of-block fallback in `Document::set_key`
  (unreachable after parse, 0 missed mutants).
- SV-005: `Store::doctor` is required by the trait, and a stub returning `Vec::new()` fails
  the zero-missed bar, so two of SV-007's three checks landed here: `store` (the directory:
  absent, unreadable, or its task file count), one `task-file` `Fail` per `.md` that is not a
  v2 task file (or `task-files` `Ok`) and one `ids` `Fail` per duplicated id (or `ids`
  `Ok`). The stale-lock check waits for the lock (SV-006). `create`, `update` and `set_done`
  return `StoreError::Unsupported` ("the tasq store does not write yet") until SV-006 and
  SV-007. `TasqStore` has no `dir()`/`workflow()` accessors: nothing needed them and the
  untested `workflow()` was a missed mutant; `describe().location` is the directory.
- SV-005 review follow-ups (same commit): `StoreError::AmbiguousId` is built once
  (`Scan::single`) and its `Display` is the `get` error, the `list` warning (plus "; see
  'tasq doctor'") and the doctor `ids` detail, so the wording has one pin (`error_messages`
  in core); `describe` on a directory it cannot read reports `0` and queues the I/O error as
  a warning, so `tasq store info` does not pass a permission problem off as an empty store;
  the `task-file` fix text follows the parse error (five variants, each pinned);
  `docs/architecture.md` describes the crate's read path and the two new modules. SV-007's
  text in PLAN.md was narrowed to the stale-lock check.
- SV-003: `FormatError::NotATask`'s message still names only the v1 title shapes; the CLI and
  store-nb snapshots pin it, and the tasq store (SV-005) classifies files through
  `NotFrontmatter` instead. The v2 file format is implemented but `docs/file-format.md` is
  SV-012's; the normative grammar lives in the `format::key` and `Frontmatter` doc comments
  until then.

## Blockers

- One cargo build at a time on this machine (`scripts/guard`); independent sub-tasks (SV-001,
  SV-002 and SV-003 were; SV-006 is now the frontier, SV-007 and SV-008 wait on it) are worked
  one after another, not in parallel agents.
