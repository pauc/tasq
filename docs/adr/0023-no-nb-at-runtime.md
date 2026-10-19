# ADR-0023: No nb at runtime: native bookkeeping, commits and sync off the critical path

- **Status:** Superseded by ADR-0027 (the nb implementation is removed; ADR-0026 is the store)
- **Date:** 2026-10-08

## Context

ADR-0007 delegated bookkeeping to nb when it is on `PATH`: `nb index add` after a create,
`nb git checkpoint <msg> --wait` after every write. Measured on the author's notebook
(nb todos 22 and 48):

- With nb's `auto_sync` on (nb's default), `_git_checkpoint` commits **and** runs `_git sync`
  (`git ls-remote`, `git fetch`, merge, `git push` to GitHub over SSH) in the subshell that
  `--wait` waits for. `NB_AUTO_SYNC_SECONDS` only throttles the sync nb's read commands start;
  a checkpoint syncs every time. `tasq done` took 5-6 s, all of it network
  (`ls-remote` 1.5 s, `fetch` 2.3 s, push not measured).
- Without the sync, a commit still costs bash startup plus `git`, and ~440 ms more when the
  user's git config signs commits (`commit.gpgsign = true`, the author's setup).
- Every other nb role already had a native twin: reads parse `.index` and files directly,
  `NativeBookkeeper` appends to `.index` byte-identically to `nb index add`, commits and
  syncs with `git`. What only nb did: resolve a notebook outside `$NB_DIR`
  (`nb notebooks show`), rebuild a missing `.index` (`nb index reconcile`), and its own
  `nb index verify`.

Two code paths with different semantics (one async-with-sync, one synchronous) and a
dependency that makes every write wait on the network are a poor trade.

## Decision

`tasq` never runs `nb`. The file format stays nb-compatible (ADR-0002): nb can keep using
the same notebook, and the nb parity tests keep `nb` as a test-only dependency.

- **One bookkeeper.** `NbCliBookkeeper`, the `Nb` runner and `store.bookkeeper` are removed.
  `NativeBookkeeper` registers new files (append to `.index`), commits and syncs.
- **Commits leave the critical path.** After a write the store returns as soon as the file
  is on disk. The commit runs in a detached job (`sh` running `git add -A`, `git commit -q
  -m "[tasq] <Verb>: <file>"`), so nb's `[tasq] ...` message convention stays. A lock file
  `.git/tasq/lock` held by the job serialises jobs; every write first waits for the previous
  job to finish (stale after 2 minutes), so commits never interleave and a pull never runs
  under a write. A failed job leaves its message in `.git/tasq/failed`; the next store open
  turns it into a bookkeeping warning and removes it.
- **Sync is throttled and in the same job.** When the notebook has a remote and the last sync
  (`.git/tasq/last-sync`, epoch seconds) is at least `store.sync_interval` seconds old
  (default 3600; 0 disables), the job then runs `git pull --rebase -q` and `git push -q`,
  with SSH in batch mode so it can never prompt on the terminal. `tasq store sync` still
  syncs on demand, synchronously, after waiting for the lock.
- **Notebook resolution.** `store.notebook` is a name under `$NB_DIR` (default `~/.nb`) or,
  when it contains a `/` (`~/...` expanded), a directory path. No `nb notebooks show`.
- **Index repair is native and append-only.** A missing `.index` is rebuilt from the
  notebook's non-hidden `*.md` files in name order (= creation order for nb's timestamped
  names), with the existing "ids may have changed" warning. `tasq store reconcile` fixes an
  inconsistent index without renumbering: it blanks lines naming missing files (nb's own
  representation of a deleted file) and appends unlisted files. `tasq doctor` points at it.

## Consequences

### Positive

- `tasq done`, `set`, `log`, `create` and every TUI edit return in milliseconds, whatever
  the network and the signing setup.
- One bookkeeping path to test; no fake `nb` in the store tests.
- Works the same with or without nb installed; nb is no longer a runtime requirement.
- `tasq store reconcile` keeps ids stable, which `nb index reconcile` does not.

### Negative

- A write's commit lands shortly after the command returns, not before. A crash between the
  write and the job leaves the file uncommitted; the next job's `git add -A` picks it up
  under its own message. Mitigation: the window is the job's runtime.
- Commit and sync failures surface one command late, as a warning.
- tasq now owns filename, index and sync semantics that nb could change. Mitigation: the
  nb parity tests (`TASQ_REQUIRE_NB=1` in CI) still run nb against tasq's files.
- The job needs a POSIX `sh`; tasq only targets Unix.
- Notebooks nb resolves through other means (nb's local notebooks, archived notebooks) need
  `store.notebook` set to their path.

## Alternatives considered

- **Keep nb, run the checkpoint with `NB_AUTO_SYNC=0`** — removes the network but keeps bash
  startup and the signing cost on every write, and keeps two code paths.
- **Commit synchronously, natively** — ~10 ms unsigned, ~450 ms signed; still visible on
  every TUI key.
- **Push on every write in the background** — closest to nb's behaviour, but a pull per
  write races the next write for little gain; the interval keeps the hourly cadence nb was
  configured for.
- **Sync only on demand** — simplest, but a forgotten `tasq store sync` leaves other
  machines behind.

## References

- ADR-0007 (superseded), ADR-0002, ADR-0008.
- nb todos 22 and 48; `crates/store-nb/src/native.rs`, `crates/store-nb/src/store.rs`.
