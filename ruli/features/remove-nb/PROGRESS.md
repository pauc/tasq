# Progress: remove the nb implementation

Plan: [PLAN.md](PLAN.md). Task: tasq 58. ADR: 0027.

## Status

| Id | Title | Status | Notes |
|---|---|---|---|
| RN-001 | The list-section op tests run on the current format (prefactor) | done | cd5f55f; 31 current-format pairs under `crates/core/tests/fixtures/v2/ops/` (every progress, worktree, session, merge request, description and clear-description scenario), derived from the v1 pairs with `## Tags`/`## Project`/`## Due` moved into the block (`## Notes` where that heading closed the section under test); the three scenario checks take a directory; `mod current_format` in `format_ops.rs` mirrors the ten free-standing list-section tests; `every_ops_fixture_round_trips` in `format_v2.rs`; no library change, no mutation run |
| RN-002 | `tasq migrate` is gone | done | 6c16e55; `crates/cli/src/commands/migrate.rs`, `Command::Migrate`, `From<MigrateError> for CliError`, the CLI `mod migrate` tests, their two snapshots and the `notebook`/`nb_fixture_dir`/`dir_bytes` helpers deleted; `crates/store/src/migrate.rs` and `crates/store/tests/migrate.rs` (with the `TASQ_UPDATE_FIXTURES` path) deleted, `thiserror` dropped from tasq-store; the nb crate's parity test `crates/store-nb/tests/migrate.rs` deleted; help snapshot loses the `migrate` line; `tasq migrate home` now fails like any unknown word (the `list` shortcut rejects the extra argument, exit 2); the fixture store is unchanged; `public_api.rs` never named migrate, so no change there; `mutants-in crates/store/src`: 165 mutants, 130 caught, 33 unviable, 2 timeouts (the known lock and name loops), 0 missed |
| RN-003 | The nb crate is gone | done | d4c4c3d; `crates/store-nb` deleted whole (37 files: the crate, its fixture notebook, its tests and the `TASQ_REQUIRE_NB` gate in `tests/support`), its workspace member and `Cargo.lock` entry gone; `tasq-store-nb` dropped from the `release.yml` publish loop and its order comment, `crates/store-nb/**` from `examine_globs` in `.cargo/mutants.toml`, the `feat(store-nb)` example from `cliff.toml`, and `store-nb` from the `std::process::Command` reason in `crates/core/clippy.toml` (now "see worktree, launch"; review caught that `tasq-store` runs no process); the CI "Install nb" step and `TASQ_REQUIRE_NB` env removed from `ci.yml`; the two comments naming the crate (`crates/core/src/store.rs` module doc, `crates/store/tests/public_api.rs` header) rewritten; no other crate depended on it; 1239 workspace tests pass; deletion only, no mutation run |
| RN-004 | Core has one task file format | done | e4e3c9d; `format::parse` (the shape-sniffing entry point), the v1 branch of `Document::parse`, `read_v1_scalar`/`read_tags`/`classify`, `Document::from_task`, `is_v2` and every op branch it guarded, `strip_status_tag` (now `clear_status`, a plain key removal), `OPEN_PREFIX`/`DONE_PREFIX`, `entry::{parse_origin, format_origin}`, `section::{PROJECT, DUE, TAGS, SOURCE, CLOSED, RANK, ALL}` deleted; a file whose first line is not `---` is `NotFrontmatter` ("not a task file: ..."), `FormatError::NotATask` gone; `Document::frontmatter()` returns `Frontmatter`, `Document::id()` returns `TaskId`, `format::project(doc, workflow)`, `set_status`/`set_priority`/`set_tags`/`set_done` take no workflow, `set_key` leaves `id` alone (review caught that the infallible `id()` was otherwise breakable); `IdScheme`, `id_scheme`, `ids_may_change_on_reconcile` and the dead `StoreError::Index` gone, `store info` text and JSON snapshots regenerated; the three `Removed` store keys and their deserialisers gone (an old key is `unknown field `kind`, expected `name`` at file:3:1, pinned in `config_load.rs` and `cli.rs`; `TASQ_NOTEBOOK` pinned as an ignored variable); the plugin ban on `.todo.md`/`.index`/`notebook` gone; nb/v1/v2 wording swept from comments, test data and snapshots in core, store, cli, launch, tui, sources (launcher prompt snapshot regenerated with a current-format file); 9 v1 fixtures, the 100 `fixtures/ops/` pairs, `format_fixtures.rs` deleted; `format_parse.rs` (body grammar), `format_props.rs`, `format_ops.rs` (the RN-001 module hoisted) rewritten on the current format, the document-view tests moved to `format_v2.rs`, `diff.rs` tests rewritten on frontmatter documents; `insert_section` → `insert_description`, `Document::terminate` removed, pairs `description_inserts_before_adjacent_section` and `clear_description_last` added; `mutants-in crates/core/src/format`: 323 mutants, 289 caught, 33 unviable, 1 timeout (`Document::remove` → `()` spins `trim_trailing_blank_lines`), 0 missed |
| RN-005 | The `v2` names are gone (rename only) | done | 8b3e4d4; `format::parse_v2` → `format::parse`, `Document::from_task_v2` → `Document::from_task`, `v2_title_of` → `title_of`; `crates/core/tests/fixtures/v2/*.md` → `fixtures/*.md` and `fixtures/v2/ops/` → `fixtures/ops/` (75 files, all 100% renames), `format_v2.rs` → `format.rs`, `format_v2_write.rs` → `format_write.rs`; test helpers `render_v2` → `render_task`, `v2_fixture`/`V2_FIXTURES` → `fixture`/`FIXTURES`, the CLI `v2()` → `task_text()`, `const V2` in `view.rs` → `TEXT` (review: `task_file` and `FILE` read as paths next to `commands::task_file`, a local `render` shadowed the verb of `format::render`); two test names lose `v2` (`..._only_through_parse`, `a_full_draft_is_written_as_a_canonical_task_file`); `apply_v2` never existed; left as is: the `ruli/features/store-v2/` link in `crates/store/src/lib.rs` (a record path) and the "unknown in v2" prose inside `fixtures/extra-sections.md` (fixture content); 1183 workspace tests pass; `mutants-in crates/core/src/format`: 323 mutants, 289 caught, 33 unviable, 1 timeout (`Document::remove` → `()`, as in RN-004), 0 missed |
| RN-006 | The docs describe what exists | ready | blocked by RN-002..005 |

## Learnings

- 2026-10-19: scope settled by grilling. Full removal including `tasq migrate` (every
  notebook the user wanted is already migrated; no release ever shipped migrate). The repo
  reads as if the nb implementation never existed: living surfaces rewritten, records (git
  history, CHANGELOG, `ruli/features/*`, ADR texts) kept. `IdScheme` dropped, `v2` names
  dropped, nb-only config guards dropped. ADR 0027 written, superseding 0002 and 0023 (0007
  was already superseded by 0023).
- 2026-10-19 (RN-001): the current-format pairs were derived mechanically from the v1 pairs,
  not from the code, and all matched on the first run. A dropped `## Tags` can remove the
  heading that closes the section under test (review caught it in
  `mr_related_exists_without_sub`); those pairs keep a `## Notes` section in its place.
- 2026-10-19 (RN-004): the first mutation run after the deletion missed five mutants, all in
  `insert_section`, `remove_sections` and `Document::terminate`: the v1 scalar scenarios
  (`project_after_title`, `due_project_last_no_blank`, `clear_due_last`, ...) were what
  exercised them, and the one remaining caller (`set_description`) only inserts after the
  title. Fixed by shape, not by argument: `insert_section` became `insert_description` with
  the position fixed, `terminate` went (every caller already terminates through `insert` or
  `remove`), and two description pairs were added for the cases the script's scenarios had
  covered indirectly (a non-blank line right after the title, a last section removed).
- 2026-10-19 (RN-004): making `Document::id()` infallible made `ops::set_key(doc, "id", "x")`
  a way to build a document whose `id()` panics; review caught it. `set_key` now leaves `id`
  alone, as `clear_key` already did (the id is written once, on create), and the test that
  rewrote the id through `set_key` pins the no-op instead.
- 2026-10-19 (RN-004): leftovers that are not this ticket's: "Keep the nb files unchanged." in
  two CLI snapshots is a task description inside the committed fixture store (byte for byte,
  story 10); "tasq v1" in `plugins/claude/README.md` is the product version (RN-006 decides);
  `DIR = "v2/ops"`, `const V2` in `view.rs`, the `v2()` CLI test helper and
  `a_full_draft_is_written_as_a_canonical_v2_file` are identifiers (RN-005). `ops::clear_status`
  is a new name the plan did not anticipate: `strip_status_tag` was not v1-only (`diff.rs` used
  its frontmatter arm), so the key removal needed a name.
- 2026-10-19 (RN-005): dropping a suffix can land a name on an existing one with another
  meaning. The CLI test helper first became `task_file`, the name of the path lookup in
  `commands`, and the `view.rs` constant `FILE`; review renamed them `task_text` and `TEXT`.
  The duplicated `FIXTURES`/`fixture`/`parse` helpers in `format.rs` and `format_write.rs` are
  now byte-identical; sharing them is left for later, not a rename.

## Blockers

None.

## Deviations

None.
