# ADR-0007: nb under the hood: hybrid native/nb bookkeeping

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

nb (AGPLv3, bash) already solves bookkeeping we do not want to re-implement:
the `.index` id mapping (one filename per line, id = line number), filename
generation rules, git auto-commit (`nb git checkpoint`) and remote sync
(`nb sync`, `auto_sync`). nb is also slow to invoke, one bash startup per
command, which is what makes the current script sluggish on listing. nb's own
documentation warns that manual use of `nb index` "will probably corrupt the
index". Finally, the tool should work for people who do not run nb at all.

## Decision

A hybrid, implemented as a `Bookkeeper` strategy chosen by the store config
`bookkeeper = "auto" | "nb" | "native"` (`auto` picks nb when found on `PATH`).

- **Read natively.** `list`, `view`, `next` parse `.index` and the markdown
  files directly. No nb process is spawned on the hot path. The notebook
  directory is resolved as `$NB_DIR/<name>` when it exists, falling back to
  `nb notebooks show <name> --path` with its output sanitised.
- **Write files natively.** Edits are atomic rewrites of the single task file
  (temp file + rename in the same directory). The format layer guarantees nb
  can still read the result (ADR-0002).
- **Delegate bookkeeping to nb when present (`NbCli`).** After `create`, run
  `nb index add <file>`; after any write, run `nb git checkpoint "<message>"`,
  which also pushes when nb's `auto_sync` is on; use `nb index verify` in
  `tasq doctor`. The exact index and git semantics stay nb's, not ours. Commit
  messages follow nb's style (`[tasq] Update: <file>`) so `nb history` stays
  readable. nb output is sanitised (ANSI, CR) and hidden unless `-v`.
- **Native fallback (`Native`).** Without nb installed, append to `.index` and
  commit with `git` when the notebook is a repository, otherwise do nothing.
  The `.index` line written must be byte-identical to what `nb index add`
  produces. `tasq store sync` runs `nb sync` or `git pull --rebase && git push`.
- **Never reorder or rebuild the index ourselves.** We only ever call `add` and
  `verify`, never `rebuild`. When `nb index verify` fails, `tasq doctor`
  suggests `nb index reconcile`. A missing `.index` makes the store attempt
  `nb index reconcile` once and warn that ids may have changed.
- Bookkeeping failure after a successful file write is reported as a warning
  with the manual fix, never as data loss.

nb is invoked only as a separate program through its CLI. It is never linked,
vendored or copied; see ADR-0008 for the licensing consequence.

## Consequences

### Positive

- Listing is fast: no process spawn, target under 50 ms on 500 tasks.
- Ids, filenames, commits and remote sync behave exactly as nb users expect,
  because nb does them.
- Users without nb get a working tool with a plain directory of markdown files
  and a plain-text index.
- Only two nb index subcommands are used, both documented as safe.

### Negative

- Two code paths (`NbCli`, `Native`) to test; `NbCli` is tested with a fake `nb`
  recording its argv, `Native` on a temporary git repository.
- The filename rule and `.index` append format are duplicated from nb's `_add`
  implementation for the native path and must be verified against nb (T-203).
- Positional ids can still shift after deletions or `nb index reconcile`;
  `Store::describe` exposes this so the CLI can warn.
- Concurrent edits by nb and `tasq` are detected (mtime + hash conflict on
  `update`) but not merged.

## Alternatives considered

- **Drive nb for everything** — correct by construction but slow, and unusable
  without nb.
- **Re-implement all of nb's bookkeeping natively** — removes the dependency
  but duplicates filename, index and git-sync semantics that nb may change, and
  risks the index corruption nb warns about.
- **Rebuild the index ourselves when inconsistent** — explicitly rejected;
  `nb index reconcile` is nb's job and `tasq doctor` only points at it.

## References

- Plan sections 4.5, 9; tasks T-201, T-202, T-203, T-206, T-204, T-205.
- ADR-0002, ADR-0008.
