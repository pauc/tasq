# ADR-0026: Store v2: frontmatter, a tasq-owned directory, ULID file names, no git

- **Status:** Accepted
- **Date:** 2026-10-18

## Context

ADR-0024 proposed format v2 (frontmatter for the scalar fields, a stable id in
the file, `tasq migrate`) and deferred it until nb compatibility ends. The
side-by-side period is over for the author: nb's own commands are no longer run
on the notebook, and the remaining forces are the ones ADR-0024 listed plus
three the store adds:

1. **Ids are positional.** A task's id is its line number in nb's `.index`.
   `tasq store reconcile` and `tasq doctor` exist mostly to keep that file
   honest.
2. **File names collide.** nb's `YYYYMMDDHHMMSS.todo.md` rule names a file by
   the second it was created in. Two concurrent creates pick the same name and
   the loser fails with `EEXIST` (task 47, reproduced on 2026-10-08: 9 of 50
   parallel creates failed).
3. **Git is on the write path.** Every write takes `.git/tasq/lock`, hands it
   to a background job that commits, and a notebook that is not a repository
   gets no lock at all (ADR-0023). Remote sync is wanted eventually but is not
   what makes the store correct.

## Decision

`tasq` stores tasks in its own directory, in format v2, with no git.

- **Format v2** as ADR-0024 proposed, with two changes. There is no
  `created:` key: the creation time is read from the file name. The keys are
  `id`, `status`, `priority`, `rank`, `due`, `closed`, `project`, `source`,
  `source_id`, `source_url` and `tags` (a one-line list `[a, b]`). The block
  is a flat `key: value` subset parsed line by line in `tasq-core`, no YAML
  crate; unknown keys, their order, unknown sections and blank lines survive
  every edit. `status: done` replaces the `[x]` marker. The model does not
  change: `Task::done` stays a bool, a done task has no status, and `done`
  stays a refused status name. The body keeps the title heading,
  `## Description`, `## Related` with `### Merge requests`, `## Progress`,
  `## Worktrees` and `## Sessions`.
- **A store is a directory**: `<data_dir>/<store.name>`, by default
  `~/.local/share/tasq/stores/default` (`data_dir` is a top-level setting
  defaulting to `$XDG_DATA_HOME/tasq/stores`). No marker file, no `.index`,
  no `.git`. A directory that does not exist yet is an empty store and is
  created on the first write. `.tasq/` inside it holds the write lock.
- **Task files are `<ULID>.md`.** The ULID's millisecond timestamp is the
  task's creation time (`Task::created_at`, shown in local time, `created` in
  JSON); the random part makes two creates in the same millisecond distinct.
  Create writes atomically with no-clobber and retries with a fresh ULID when
  the name exists. A directory listing sorts chronologically. Nothing else is
  read from the name.
- **Ids are sequential numbers in the file.** `id:` is written on create
  (highest existing id plus one, never reused) and on migrate (the nb id,
  unchanged). The store maps id to file by reading every file's frontmatter
  on each command; there is no cache. `describe()` reports `Stable`.
- **One lock, no job.** A write takes `.tasq/lock` (exclusive create, stale
  after two minutes) across reading the id map, allocating and writing, and
  releases it when the command ends. `update` keeps the revision check (mtime,
  length, hash). Reads never lock. Nothing is committed anywhere; commits and
  sync return as an opt-in feature in task 57.
- **Doctor** checks that every `.md` file has a frontmatter with an `id`, that
  no two files share an id, and that the lock is not stale. A file failing the
  check is listed and skipped by `list`.
- **Config**: `data_dir` and `store.name` only. `store.kind`,
  `store.notebook`, `store.sync_interval`, `TASQ_NOTEBOOK`, `tasq store sync`
  and `tasq store reconcile` go. Launched sessions receive `TASQ_DATA_DIR`
  and `TASQ_STORE_NAME`.
- **`tasq migrate <notebook> [--to <dir>] [--dry-run]`** converts an nb
  notebook into a store: same ids, new ULID file names minted from the nb
  timestamps, done tasks included, mapping and config lines printed, notebook
  never written. After this change `migrate` is the only command that reads
  an nb notebook; `crates/store-nb` stays only as its dependency until a
  follow-up task deletes it together with the v1 reader and the parity tests.

## Consequences

### Positive

- Stable ids with nothing to reconcile; the positional-id warning goes.
- No same-second collision by construction (task 47 closes with this).
- No git on the write path: a write is a lock, a file and an unlock. Sync
  becomes a feature with its own switch (task 57) instead of a precondition.
- One block with one insertion rule for scalars; `format::ops` shrinks to the
  list sections.
- Creation time for free, in the file name, without a key to keep in sync.

### Negative

- An explicit one-off migration per notebook, mitigated by `--dry-run`, a
  printed mapping, and the notebook being left untouched.
- The file name carries meaning after all (the creation time). Renaming a
  file by hand changes it. Accepted: the alternative is a key that duplicates
  the same value.
- A typo in `data_dir` or `store.name` shows an empty store instead of an
  error. Mitigated by `tasq store info` and `tasq doctor` printing the
  resolved path with its config provenance.
- Id allocation is per store and per machine; two machines creating tasks
  offline and syncing later can allocate the same id. Out of scope until git
  sync exists (task 57), where doctor reports it rather than merging silently.
- Until the follow-up deletes it, store-nb is dead weight in the workspace.

## Alternatives considered

- **Keep the nb notebook and add stable ids only** (ADR-0024's deferral) —
  keeps the collision, the `.index` and git on the write path.
- **UUID v4 file names** — no collision either, but carries no creation time
  and does not sort; a `created:` key would be needed.
- **A marker file (`.tasq/store.toml`, `format = 2`) and `tasq store init`**
  — guards against a mistyped path, at the cost of a command and a file;
  rejected because the stores root makes a mistyped name land harmlessly.
- **A cache of id to file under `.tasq/`** — faster for thousands of files,
  but a second thing to rebuild; not needed at the current size (54 tasks).
- **Keep git bookkeeping as today** — keeps the background job, the lock
  hand-over and the failed-job warning in the store; rejected so the store is
  correct on its own and sync is designed once, as a feature.
- **Both store kinds behind `store.kind`** — doubles the surface for a
  transition that is one `tasq migrate` away.

## References

- Supersedes ADR-0024. Ends the premise of ADR-0002 (nb-compatible store) and
  ADR-0023 (native bookkeeping) for the v2 store; both stand for store-nb
  until it is deleted.
- tasq 56 ("Store v2: frontmatter, a tasq-owned directory and random file
  names"), 47 (the collision), 57 (opt-in git sync), 2 (side-by-side run,
  obsolete).
- `ruli/features/store-v2/PLAN.md`, `CONTEXT.md` (Store, Task file, Id,
  Frontmatter).
- ULID spec: <https://github.com/ulid/spec>.
