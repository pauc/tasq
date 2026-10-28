# Plan: one switch per reason sync marks a task done

Task: tasq 61 ("sync: replace close_when_done with done_when_closed and done_when_unassigned").
Decision record: [ADR 0032](../../../docs/adr/0032-done-reasons-and-source-keys-per-kind.md).
Progress: [PROGRESS.md](PROGRESS.md). Planned 2026-10-28.

## Problem Statement

Silverfin reassigns an issue to the MR reviewer when it moves to `Core::Review` and drops the
person who worked on it. `WorkItems::check` (`crates/sources/src/work_items.rs:96`) reports an
open issue no longer assigned to me as `ItemState::Done`, and `reconcile` closes every matched
task whose item is done as long as `Policy::close_when_done` is on
(`crates/core/src/source.rs:349`). Task 216 in the Silverfin store closed that way while its
work was still in review.

`close_when_done` is one switch for four different events (closed, merged, approved,
unassigned), and the reason only survives as the note string. Turning it off to keep 216 open
would also stop tasks from closing when their issue closes.

Separately, `SourceConfig` is one flat struct: `labels` on a review-requests source or `drafts`
on a work-items source are accepted and silently ignored.

## Solution

Sources report **why** an item is done, as a `DoneReason`, and the source's `Policy` decides
per reason whether that closes the task. `close_when_done` is replaced by one boolean per
reason, all defaulting to `true`:

| Key | Review requests | Work items | LLM bridge |
|-----|-----------------|------------|------------|
| `done_when_closed` | MR closed without merge | issue closed | no |
| `done_when_merged` | MR merged | no | no |
| `done_when_approved` | I approved the MR | no | no |
| `done_when_unassigned` | no | open, no longer assigned to me | no |

An item that no longer exists (404) always closes the task. An item the LLM bridge reports as
`done` always closes it; its prompt decides when.

`close_when_done` anywhere is a config error naming the new keys. A key set on a source whose
kind does not use it is a config error at load time, with the file, line and column of the key:

```
config.toml:12:1: source "gitlab-work-items": `done_when_approved` does not apply to kind gitlab-work-items
```

## Implementation Decisions

- **`DoneReason`** (`tasq_core::source`): `Gone`, `Closed`, `Merged`, `Approved`, `Unassigned`,
  `Reported`. `ItemState::Done` becomes `ItemState::Done(DoneReason)`.
- **Wire format unchanged.** The LLM bridge JSON keeps `"state": "open" | "done" |
  "needs-attention"`. `ItemState` gets a hand-written `Deserialize`/`Serialize` through a private
  `WireState` enum (`#[serde(from, into)]`): `done` reads as `Done(Reported)`, every `Done(_)`
  writes `done`. Nothing reads a serialized forge state back, so the reason is not on the wire.
- **The note stays as is.** Sources keep setting `note` (`"closed"`, `"approved by you"`, ...);
  the reason decides, the note explains. No mapping from reason to text.
- **`Policy`**: `close_when_done: bool` becomes `done_when: DoneWhen { closed, merged,
  approved, unassigned }` (all `true` in `Default`) plus `Policy::closes(DoneReason) -> bool`,
  where `Gone` and `Reported` are always `true`. `changes_for_known` asks `closes(reason)`;
  `reconcile`'s "a new done item is ignored" rule matches `Done(_)`.
- **A done item that does not close stays open.** With `done_when_unassigned = false` an
  unassigned issue is treated like an open one: no `Close`, and the `flag` tag still applies.
  No note, so every sync does not log "no longer assigned to you" again.
- **`SourceConfig`**: `close_when_done` removed; `done_when_closed`, `done_when_merged`,
  `done_when_approved`, `done_when_unassigned` added, `#[serde(default = "default_true")]`.
  `registry::policy_for` maps them into `DoneWhen`. A key that does not apply to the kind keeps
  its default, so `Policy` never needs to know the kind.
- **Keys per kind live in core**: `SourceKind::uses(key) -> bool` (or a `keys()` slice) with the
  table above plus `forge`, `title` (forge kinds), `exclude_authors`, `drafts` (review
  requests), `labels`, `exclude_labels`, `projects` (work items), `command`, `prompt_file` (LLM
  bridge). `name`, `kind`, `tags`, `status`, `enabled`, `auto`, `create_new` and `flag` apply to
  every kind.
- **Checked per file, at load, before `FileSchema`** (`config/load.rs`, `parse_file`): a private
  `SourceKeys` schema reads each `[[source]]` as `BTreeMap<Spanned<String>, Value>`, so only
  keys present in the file are seen and each has a span. Per source, in key order:
  - `close_when_done` → `` `close_when_done` was replaced by `done_when_closed`,
    `done_when_merged`, `done_when_approved` (review requests) and `done_when_unassigned`
    (work items); see docs/sources.md `` at the key.
  - a `kind` that parses and does not use the key → the wrong-kind error at the key.
  - no `kind`, or one that does not parse: skipped, `FileSchema` reports it as today.
  Running before `FileSchema` matters: once the field is gone, `deny_unknown_fields` would
  otherwise answer `close_when_done` with a generic "unknown field". Profile files go through
  `parse_file` too, so their `[[source]]` lists are covered. `--set` and `TASQ_*` cannot address
  `source[]` entries (`template()` has no array leaves), so the files are the only way in.
- **Guard test**: every field of `SourceConfig` (from serialising a fully populated value) is
  either common or used by at least one kind, so a new source key cannot skip the table.

## Testing Decisions

- Core reconcile (`source.rs` unit tests): for each `DoneReason`, with its switch on and off,
  `changes_for_known` gives `Close` or nothing; `Gone` and `Reported` close with every switch
  off; an off reason still gets the `flag` tag; a new item with `Done(_)` is never created.
- Wire: `"done"` parses to `Done(Reported)`; `Done(Merged)` serialises to `"done"`; `open` and
  `needs-attention` round-trip.
- Sources: `work_items::check_states...` and `review_requests::check_states` assert the reason,
  not just `ItemState::Done`. `registry` tests: each `done_when_*` key reaches `Policy`.
- Config (`crates/core/tests/config_load.rs`): defaults are all `true`; `close_when_done` in the
  global file and in a profile errors with the exact message and `file:line:col`; one
  wrong-kind case per row of the key table (including `drafts = false` on work items and
  `forge` on the LLM bridge); an absent key on the wrong kind is fine; a source without `kind`
  still gets the existing "missing field `kind`" error.
- Exact-string assertions on every message, so mutants on the text and the key table die.
- Mutants: zero missed in `crates/core/src/source.rs`, `crates/core/src/config/`,
  `crates/sources/src/{registry,work_items,review_requests,llm_bridge}.rs`.

## Out of Scope

- Per-reason notes or a different note text for reasons that do not close.
- Flags on the LLM bridge.
- Reopening tasks: a task closed by sync stays closed when its item reopens (unchanged).
- Detecting the reviewer handoff itself (e.g. by label); `done_when_unassigned = false` is the
  fix for Silverfin.

## Sub-tasks

### SD-001: `DoneReason` and the policy in core

`ItemState::Done(DoneReason)` with the wire format unchanged, `DoneWhen`, `Policy::closes`,
`changes_for_known` and `reconcile` updated, doc comments describing the per-reason rule.

Acceptance: core tests above pass; existing `source.rs` tests compile with explicit reasons;
zero missed mutants in `source.rs`.

### SD-002: sources report reasons

`WorkItems::check` → `Gone` / `Closed` / `Unassigned`; `ReviewRequests::check` → `Gone` /
`Merged` / `Closed` / `Approved`; LLM bridge `done` → `Reported`. `registry::policy_for` builds
`DoneWhen` from the four config keys (which exist from SD-003; land SD-002 and SD-003 in the
order that keeps each commit green, or together).

Acceptance: source tests assert reasons; a registry test per key; zero missed mutants in the
touched files.

### SD-003: config keys and the per-kind check

`SourceConfig` fields swapped, `SourceKind::uses`, the `SourceKeys` pass in `parse_file`, the
guard test, the `close_when_done` error.

Acceptance: config tests above pass with exact messages and positions; the committed
fixtures and `docs/config.md` examples still load; zero missed mutants in `config/`.

### SD-004: docs, ADR, changelog

- ADR 0032 (done reasons, keys checked per kind) and the index in `docs/adr/README.md`; ADR 0004
  stays as written.
- `docs/sources.md` step 4 and a "When a task is marked done" table per kind.
- `docs/config.md`: the commented example block and the `source[]` key table (new rows, a
  "Kinds" column or note, `close_when_done` gone).
- `CHANGELOG.md` `[Unreleased]`: Changed (`close_when_done` replaced, breaking) and Changed
  (wrong-kind keys are errors, breaking).
- `cargo doc` clean.

### SD-005: the user's `sf` profile

Add `done_when_unassigned = false` to the `gitlab-work-items` source in
`~/.config/tasq/profiles/sf.toml` (the task text says `config.toml`; the `sf` sources moved to
the profile file with ADR 0031). Outside the repo, so only with explicit go-ahead, and after
installing the new build. Check with `tasq --profile sf sync --dry-run`.

## Further Notes

- Every commit passes `fmt`, `clippy -D warnings` and the workspace tests through
  `scripts/guard`; one build at a time.
- Breaking config change, no migration: the error message is the migration.
