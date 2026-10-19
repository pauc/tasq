# Plan: remove the nb implementation

Task: tasq 58 ("Delete the nb store: tasq-store-nb, the v1 format and the parity tests"). Decision
record: [ADR 0027](../../../docs/adr/0027-nb-implementation-removed.md), superseding
[ADR 0002](../../../docs/adr/0002-nb-compatible-markdown-store.md) and
[ADR 0023](../../../docs/adr/0023-no-nb-at-runtime.md), completing
[ADR 0026](../../../docs/adr/0026-store-v2-frontmatter-tasq-directory-ulid-files.md). Terms:
[CONTEXT.md](../../../CONTEXT.md) (Store, Task file, Id, Frontmatter). Progress:
[PROGRESS.md](PROGRESS.md). Started 2026-10-19.

## Problem Statement

Task 56 moved the CLI to a tasq-owned store. Nothing opens an nb notebook any more except
`tasq migrate`, and every notebook the user wanted converted (Tasq, Silverfin) already has
been. What is left of the nb implementation is weight with no user:

- A whole crate (the nb store, its bookkeeper and background job, git, index, resolve, doctor)
  that is built, tested, mutated, listed for publishing, and makes CI install nb.
- A second task file shape in core: every scalar op dispatches on the shape, and a v1 reader,
  a v1 document writer and v1 fixtures exist only for migrate and for tests.
- Vocabulary that no longer describes anything: positional ids and `ids_may_change_on_reconcile`
  in `store info`, three removed config keys with tailored errors, a `TASQ_NOTEBOOK` rejection,
  "notebook" in the glossary, a version suffix (`v2`) on the only format there is.
- Docs that describe a v1 format "read by migrate only", nb test gating, and a notebook
  conversion procedure.

Someone reading the repo today has to learn two formats and a dead store to change one.

## Solution

Remove the nb implementation entirely, `tasq migrate` included. Afterwards the code, tests,
fixtures, snapshots, docs, glossary and plugin prompts read as if tasq never had an nb-backed
store: one task file format with no version in its name, one store, no notebook, no migrate.

Records stay as they are: git history, the changelog, the finished plans under `ruli/features/`
and the ADRs (superseded, not deleted). `original/tasks` is still described as the nb-based
bash script tasq rewrites, because it is.

The work lands as six commits on this branch, in one PR to `main`: a prefactor that moves the
list-section op tests onto the current format, three deletions (migrate, the nb crate, the old
shape in core), a rename-only commit that drops the `v2` names, and a docs sweep.

## User Stories

1. As a maintainer, I want the nb store crate gone from the workspace, so that a build, a test
   run and a mutation run no longer spend time on code nothing executes.
2. As a maintainer, I want CI to stop installing nb, so that the pipeline has one less external
   dependency to break.
3. As a maintainer, I want the release workflow to stop listing the nb crate for publishing, so
   that the first release publishes only crates that exist.
4. As a maintainer, I want `TASQ_REQUIRE_NB` and the nb-gated test helpers gone, so that no test
   can skip silently depending on what is on `PATH`.
5. As a maintainer, I want the mutants config to stop examining the nb crate, so that
   `cargo mutants` runs reflect only live code.
6. As a maintainer, I want the changelog config to stop naming the nb crate scope, so that the
   next generated changelog has no dangling group.
7. As a user, I want `tasq migrate` gone from `tasq --help`, so that the help lists only commands
   that make sense for a tasq store.
8. As a user, I want `tasq migrate ...` to fail as an unknown command, so that nothing pretends to
   read an nb notebook.
9. As a maintainer, I want the store crate's migrate module and its error type gone, so that the
   store crate is the store and nothing else.
10. As a maintainer, I want the committed fixture store to stay byte for byte as it is, so that
    every CLI test that reads it keeps passing unchanged.
11. As a maintainer, I want the fixture store described as a hand-maintained store, so that nobody
    looks for a generator that no longer exists.
12. As a maintainer, I want core to read exactly one task file format, so that parsing has one
    path and one set of errors.
13. As a maintainer, I want the scalar ops to write frontmatter keys with no shape check, so that
    each op is one branch and mutation testing has nothing to find in a dead arm.
14. As a maintainer, I want the v1 document writer gone, so that a new document is built one way.
15. As a maintainer, I want the v1 scalar-section reader gone, so that `## Due`, `## Tags` and
    friends are just unknown sections, as they already are in the current format.
16. As a maintainer, I want the v1 fixtures and the v1 arm of the property test gone, so that the
    fixture directory holds only files the reader accepts.
17. As a maintainer, I want every list-section op (progress, worktree, session, merge request,
    related, description, generic append) tested on current-format documents before the old
    tests go, so that deleting v1 drops no coverage.
18. As a maintainer, I want the list-section op tests to keep the before/after fixture-pair
    shape, so that each scenario's expected bytes are readable as a file.
19. As a maintainer, I want the op test harness to stop claiming its pairs come from the script's
    awk, so that the test doc says what the pairs actually are.
20. As a maintainer, I want the insertion rules that came from the script (new sections before
    `## Progress`, merge requests nested in `## Related`, last duplicate heading wins, duplicate
    checks per section) still pinned by tests, so that the body behaves as before.
21. As a user, I want `tasq store info` without an `ids:` line, so that it shows only facts that
    can vary.
22. As a script author, I want `store info --json` without `id_scheme` and
    `ids_may_change_on_reconcile`, so that the JSON has no constant fields.
23. As a maintainer, I want `IdScheme` gone from core's store module, so that `StoreInfo` is
    name, location, task count and bookkeeper.
24. As a maintainer, I want `StoreInfo::bookkeeper` to stay, so that task 57 (opt-in git sync)
    has its slot.
25. As a user, I want a config that sets `store.kind`, `store.notebook` or `store.sync_interval`
    to get the same unknown-field error as any other typo, with file, line and column, so that
    there is one error shape for unknown keys.
26. As a user, I want `TASQ_NOTEBOOK` to be an ordinary unknown variable, so that the config has
    no special case for a store kind that does not exist.
27. As a maintainer, I want the plugin test that bans `.todo.md`, `.index` and `notebook` in
    skills gone, so that the plugin tests check only rules about the current store.
28. As a maintainer, I want nb wording gone from code comments, doc comments and snapshots
    (models, doctor, the LLM bridge source, the claude launcher prompt), so that reading the
    code never sends me looking for a notebook.
29. As a maintainer, I want the `v2` suffix gone from function names, test files and the fixture
    directory, so that the one format has plain names.
30. As a maintainer, I want the rename in its own commit with no logic change, so that the
    deletion diffs and the rename diff can each be reviewed on their own.
31. As a reader of the docs, I want the file format doc to describe one format, so that there is
    no "v1, read by migrate only" section to skip.
32. As a reader of the docs, I want the testing doc without the nb crate's sections, so that it
    describes the harnesses that exist.
33. As a reader of the docs, I want the architecture doc and CLAUDE.md to list the crates that
    exist, so that the crate table matches the workspace.
34. As a reader of the docs, I want the config doc without the removed keys and their errors, so
    that it documents only keys that load.
35. As a reader of the docs, I want the JSON doc's `store info` row to list the keys that are
    emitted, so that scripts can trust it.
36. As someone who used the bash `tasks` script, I want the migration doc to keep its command map
    from `tasks` to `tasq`, so that I can still find the new name for each old command.
37. As a reader of the docs, I want the migration doc without the notebook conversion procedure,
    so that it does not describe a command that does not exist.
38. As a reader of the glossary, I want Store and Id defined without notebook or migrate, so
    that the language matches the code.
39. As a future maintainer, I want an ADR that records why migrate went too and what was
    rejected, so that the missing import path is not a mystery.
40. As a future maintainer, I want the nb ADRs kept and marked superseded, so that the reasons
    the store is a ULID directory with the id in the file stay readable.
41. As an agent working in this repo, I want CLAUDE.md and the agent docs free of nb commands and
    nb crate guidance, so that I never try to build on, run or test against nb.
42. As a maintainer, I want every slice to pass fmt, clippy and the full test suite on its own,
    so that any commit in the PR can be checked out and built.

## Implementation Decisions

- **Scope line.** Living surfaces (code, comments, tests, fixtures, snapshots, docs, README,
  CONTRIBUTING, CLAUDE.md, CONTEXT.md, `docs/agents/`, plugin prompts) are rewritten. Records
  (git history, CHANGELOG, `ruli/features/*`, ADR texts) are not. Git history is never rewritten.
- **`tasq migrate` goes with the v1 reader.** No read-only v1 parser is kept. ADR 0027 records
  the rejected alternative. No release ever shipped migrate, so nobody else depends on it.
- **The fixture store stays.** It keeps its bytes and its ids (1, 3, 4, 6, 7; gaps are normal
  because ids are never reused). Its regeneration path (`TASQ_UPDATE_FIXTURES`) goes with migrate.
- **The nb crate goes whole.** It is the workspace member, the release publish list entry,
  the mutants config entry, the changelog scope, the CI nb install and `TASQ_REQUIRE_NB`, plus
  any mention in core's clippy config. Nothing outside it depends on it after migrate is gone:
  the store crate already holds `Revision`, `diff`, `draft` and the lock, and the launch crate
  holds `find_in_path`/`tool_check` (used by `view` and `doctor`).
- **Core keeps one format.** The scalar ops lose their shape check and always write a
  frontmatter key. The shape check itself (`is_v2`) goes. The v1 document writer, the v1
  scalar-section reader, the shape-sniffing parse entry point and the v1-only ops
  (`strip_status_tag` and any other op that only rewrites a tag line or a scalar section) go.
  The list-section ops stay as they are: they already work on the body of the current format.
- **`format::parse` becomes the current-format reader.** In the rename commit, `parse_v2`
  takes the name `parse`, `from_task_v2` becomes `from_task`, `render_v2`/`apply_v2` take their
  plain names, and the test files and fixture directory lose `v2`. No logic changes in that commit.
- **`StoreInfo` shrinks.** `IdScheme`, `id_scheme` and `ids_may_change_on_reconcile` go;
  `name`, `location`, `task_count`, `bookkeeper` stay. `store info` text loses its `ids:` line,
  and its JSON loses the two keys. `MemoryStore`, `TasqStore` and the test doubles drop the fields.
- **Config guards go.** The `Removed` handling for `store.kind`, `store.notebook` and
  `store.sync_interval` and the `TASQ_NOTEBOOK` rejection go. Per-file `deny_unknown_fields`
  then reports them like any other unknown key. If the `Removed` mechanism has no other user, it
  goes too.
- **The plugin ban goes.** The plugin test that bans nb file names and "notebook" in skills goes.
- **Comments and snapshots.** nb, notebook, v1/v2, positional id and migrate wording goes from
  doc comments and comments across core, store, sources, launch and cli. The claude launcher
  prompt snapshot is regenerated if its prompt mentions them.
- **Slice order: expand, then contract.** First move the list-section op tests to the current
  format. Then delete migrate, then the nb crate, then the old shape in core, then rename, then
  the docs. Each step compiles and passes on its own. Docs go last in one sweep, as in store-v2:
  for a few commits docs describe deleted code, but no code is ever wrong.
- **Docs.** `file-format` loses its v1 section and calls the format "the task file format".
  `testing` loses the nb crate's sections, `NbEnv` and `nb_or_skip`. `architecture`,
  CONTRIBUTING and CLAUDE.md lose the nb crate row and its guidance, and the fixture-store
  regeneration line. `config` loses the removed keys. `json` loses the two `store info` keys and
  the `migrate` row. `migration` keeps only the `tasks` → `tasq` command map. `release` loses
  the nb crate. README and the plugin prompts lose any notebook mention.
- **ADR and glossary** are written during the grilling and committed with this plan: ADR 0027,
  the superseded status lines of 0002 and 0023, the index, and the Store and Id entries in
  CONTEXT.md.

## Testing Decisions

- A good test here checks behaviour at the highest existing seam: the bytes a document has
  after an op, the text and JSON a command prints, the error a config produces. It never checks
  which branch ran. A removal is proven by the remaining tests still passing and by mutation
  runs finding nothing missed, not by tests that assert something is absent from the source.
- **Core format layer (main seam).** Parse, apply an op, render, compare byte for byte against
  a before/after fixture pair, through the existing op scenario table. The prefactor adds a
  current-format pair for every list-section scenario and for every free-standing list-section
  test (last duplicate heading, duplicates per section, merge request scanning, worktree prefix,
  line endings, round trip through the task). Only then are the v1 pairs deleted. The reader and
  writer keep their existing parse, write and property tests. Prior art: the scenario table and
  the current-format write tests.
- **CLI integration harness.** It runs over the copied fixture store under an isolated `HOME`
  with snapshots. It covers `--help` without `migrate`; `store info` text and JSON without the id
  scheme; and a config file setting `store.notebook` failing with the generic unknown-field error
  and its location. Snapshots are regenerated and reviewed. Prior art: the existing `store info`
  and config error tests and the help snapshot.
- **Store public-API test.** It drops `migrate` from the pinned exports.
- **Mutation bar.** Zero missed mutants. Run `mutants-in` on core's format module after the
  core deletion, when the current-format tests alone must kill every mutant in the ops that lost
  their shape branch. Run it again after the rename, because `--in-diff` sees a rename as no
  hunks. Run `mutants-in` on the store crate's sources after migrate goes. The prefactor, the nb
  crate deletion and the docs sweep get no mutation run (tests only, deletions only, docs only).
- Every command goes through `scripts/guard`, and only one build runs at a time.

## Out of Scope

- Git sync for tasq stores (task 57). `StoreInfo::bookkeeper` stays for it.
- Rewriting git history, the changelog, finished plans under `ruli/features/`, or the text of
  any ADR beyond the status lines of 0002 and 0023.
- `original/tasks` and the user's nb notebooks (never modified by this project).
- Converting the notebooks still in nb (Zantop, Territoris, archive, Palestina): the user does
  not want them converted.
- Changing the current task file format, its sections or their insertion rules.
- Any JSON change beyond the two `store info` keys and the `migrate` row.
- The TUI, beyond whatever compiles away with `IdScheme`.

## Further Notes

- The list-section ops were tested only on v1 fixture pairs; the current-format write tests
  cover the scalar ops only. That gap is why the prefactor comes first.
- The last commit with `tasq migrate` is `f7b09b1` (main at the start of this task).
- The user's real stores (Tasq, Silverfin) are untouched by this work; the installed `tasq`
  binary keeps working until it is reinstalled from the merged branch.

## Sub-tasks

Blockers first. Work the frontier: any sub-task whose blockers are done. This is a removal, so
the sequence is expand, then contract, not tracer bullets: a prefactor that makes the list-section
op tests independent of v1, three deletions, a rename, the docs. RN-001, RN-002 and RN-003 are
all on the first frontier, but only one build runs at a time, so they go in the order listed.
Every sub-task passes fmt, clippy and the full test suite on its own and closes with a row update
in PROGRESS.md. Statuses: `in-progress`, `ready`, `waiting`, `blocked`, `later`.

### RN-001: the list-section op tests run on the current format (prefactor)

**What to build:** every list-section scenario in the op scenario table (progress, worktree,
session, merge request, related, description, generic append) has a current-format before/after
pair next to the v1 pair it mirrors, and every free-standing list-section op test has a
current-format counterpart. Coverage of the list-section ops no longer depends on a v1 document.
No library code changes.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] Each list-section scenario has a current-format pair, checked byte for byte through the scenario table; the on-disk/scenario consistency checks cover the new pairs.
- [x] Last duplicate heading wins, duplicates are checked per section, merge request scanning, worktree path-prefix matching, line endings and the round trip through the task each have a current-format test.
- [x] The insertion rules from the script (new sections before `## Progress`, merge requests nested in `## Related`) are pinned on the current format.
- [x] No library change; no mutation run.

### RN-002: `tasq migrate` is gone

**What to build:** `tasq migrate` is an unknown command and is not in `tasq --help`. The store
crate has no migrate module, no `MigrateError`, no notebook resolver and no fixture-store
regeneration path; the nb crate's parity test, which calls migrate, goes with it. The committed
fixture store keeps its bytes and every CLI test that reads it is unchanged.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] `tasq migrate ...` fails as an unknown command; the help snapshot no longer lists it.
- [x] The CLI's migrate command module, its error conversion and its tests are gone; the store crate's public-API test no longer pins migrate.
- [x] The fixture store is byte for byte unchanged.
- [x] `mutants-in` on the store crate's sources: zero missed.

### RN-003: the nb crate is gone

**What to build:** the workspace has no `tasq-store-nb`. The release workflow, the mutants config,
the changelog config and core's clippy config stop naming it; CI no longer installs nb and
`TASQ_REQUIRE_NB` exists nowhere.

**Blocked by:** None (can start immediately).

**Status:** ready

- [ ] The crate directory, its fixtures and its tests are deleted; the workspace member and the lockfile entry are gone.
- [ ] The release publish list, the mutants config, the changelog scope and core's clippy config have no nb crate entry.
- [ ] CI installs no nb; nothing reads `TASQ_REQUIRE_NB`.
- [ ] Deletion only; no mutation run.

### RN-004: core has one task file format

**What to build:** core reads and writes only the current format. The v1 reader, the v1 document
writer, the shape check and every op branch it guards, the v1-only ops, the v1 fixtures and op
pairs, and the v1 property arm are gone. `StoreInfo` is name, location, task count and
bookkeeper: `store info` has no `ids:` line and its JSON no `id_scheme` or
`ids_may_change_on_reconcile`. A config setting `store.kind`, `store.notebook` or
`store.sync_interval` gets the generic unknown-field error with its location, `TASQ_NOTEBOOK`
is an ordinary unknown variable, and the plugin ban on nb words in skills is gone. Comments, doc
comments and snapshots across the crates no longer mention nb, notebooks, v1/v2, positional ids
or migrate.

**Blocked by:** RN-001, RN-002, RN-003.

**Status:** ready

- [ ] Every scalar op writes a frontmatter key with no shape check; the shape check and the v1-only ops are gone.
- [ ] Parsing a v1 file is a plain "not a task file" error, the same as any other markdown file.
- [ ] `store info` text and JSON snapshots regenerated and reviewed; `IdScheme` exists nowhere.
- [ ] A CLI test shows an old `store.*` key failing with the generic unknown-field error and file:line:column.
- [ ] No nb, notebook, v1/v2, positional-id or migrate wording left in code, comments or snapshots, except in names RN-005 renames.
- [ ] `mutants-in` on core's format module: zero missed.

### RN-005: the `v2` names are gone (rename only)

**What to build:** the one format has plain names. `parse_v2` becomes `parse`, `from_task_v2`
becomes `from_task`, `render_v2` and `apply_v2` take their plain names, the v2 fixture directory
becomes the fixture directory, and the v2 test files take plain names. No logic changes.

**Blocked by:** RN-004.

**Status:** ready

- [ ] No identifier, file or directory carries a `v2` suffix or prefix for the task file format.
- [ ] The diff is renames only: no logic, assertion or fixture content changes.
- [ ] `mutants-in` on core's format module: zero missed (`--in-diff` sees a rename as no hunks).

### RN-006: the docs describe what exists

**What to build:** every living doc describes one format, one store and no notebook:
`file-format` (no v1 section, "the task file format"), `testing` (no nb crate sections, the
fixture store as hand-maintained), `architecture`, `config` (no removed keys), `json` (no id
scheme keys, no `migrate` row), `migration` (only the `tasks` → `tasq` command map), `release`,
README, CONTRIBUTING, CLAUDE.md, `docs/agents/` and the plugin prompts.

**Blocked by:** RN-002, RN-003, RN-004, RN-005.

**Status:** ready

- [ ] No living doc mentions nb, notebooks, v1/v2, positional ids, `tasq migrate` or the nb crate, except where it describes `original/tasks`.
- [ ] The CLAUDE.md and architecture crate tables match the workspace.
- [ ] The fixture store is described as a hand-maintained store, with no regeneration command.
- [ ] Docs only; no mutation run.
