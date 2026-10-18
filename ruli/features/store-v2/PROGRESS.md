# Progress: store v2

Plan: [PLAN.md](PLAN.md). Task: tasq 56. ADR: 0026.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| SV-001 | CLI talks to the `Store` trait; `store sync` / `store reconcile` go (prefactor) | done | 2d527e5; `App::open_store` returns `Box<dyn Store>`; `Store` gained `take_warnings` and `doctor` (required); `Check` moved to `tasq_core::doctor`; 40 mutants in the diff, 0 missed |
| SV-002 | `tasq-store` crate skeleton, revision and diff moved (prefactor) | done | 7920eb3; `crates/store` (`tasq-store`, deps: core only); `revision.rs` and `diff.rs` moved with their 16 unit tests, store-nb re-exports `Revision`, `diff`, `revision`; `mutants-in crates/store/src`: 44 mutants, 42 caught, 2 unviable, 0 missed (two tests added) |
| SV-003 | Core reads and renders a v2 task file | ready | |
| SV-004 | Core writes v2: new documents and one op per key | blocked (SV-003) | |
| SV-005 | The tasq store reads a directory | blocked (SV-003) | |
| SV-006 | Create under the store lock | blocked (SV-004, SV-005) | |
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

## Blockers

- One cargo build at a time on this machine (`scripts/guard`); SV-001, SV-002 and SV-003 are
  independent but are worked one after another, not in parallel agents.
