# Progress: tasq rewrite — Phases 0 and 1

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-001 | Workspace and quality gates | done | commits cc023aa, 01e2ead; edition 2024, MSRV 1.90 |
| T-002 | CI pipeline | done | commit ab28fc1; cargo-deny 0.20.2 passes locally |
| T-003 | ADRs | done | commit f79c210; 8 ADRs + docs/file-format.md draft |
| T-004 | cargo-mutants setup | done | commit 07d3203; cargo-mutants 27.1.0 |
| T-101 | Domain model | done | commit f9e606c; 59 tests; mutants 121 total, 0 missed, 1 skip (SystemClock::now) |
| T-105 | Clock and progress logging | done | commit f9e606c; `When` enum keeps date-only entries lossless |
| T-102 | Markdown parser | acceptance tests done, mutants running | commits 5ed472e, 7bd3ff5; 308 mutants in progress |
| T-103 | Markdown writer and round trip | acceptance tests done, mutants running | commits 5ed472e, 7bd3ff5; 308 mutants in progress |
| T-104 | Queries and grouping | done | commit 72790ee; 29 tests; 0 missed mutants |
| T-106 | Configuration loading | gates green, mutants pending | wave 4; agent killed by OOM before mutants pass and commit |

## Wave plan

1. T-001 alone: everything else needs the workspace. Pre-creates the `justfile` and stub
   modules so later parallel agents own disjoint files.
2. T-002, T-003, T-004 in parallel (disjoint files: `.github/`, `docs/adr/`, `.cargo/mutants.toml` + justfile recipes).
3. T-101 + T-105: the model everything in Phase 1 depends on. Declares `format`, `query`, `config` modules as stubs.
4. T-102+T-103 (one agent), T-104, T-106 in parallel, each owning one module directory.

## Learnings

- **Mutation testing needs a per-test memory limit.** A mutant that turns a loop infinite while
  allocating outruns any timeout or OOM daemon. The cargo `runner` + `ulimit -v` wrapper in
  `scripts/test-runner` is the systemic fix; prefer bounded `for` iteration over manual
  `while i < len` index loops in code that allocates per iteration.
- **Never bypass `scripts/guard`, not even for a "quick" reproduction.** That is exactly what
  killed the session the second time.
- **Parallel agents on one Rust workspace are a memory hazard.** Cap `build.jobs`, wrap builds in
  a cgroup (`scripts/guard`), and serialize anything that compiles. See Blockers.

- **`ruli/` is ignored by the user's global git excludes** (`~/.config/git/ignore`). The repo
  `.gitignore` adds `!/ruli/` so the plan and this file are tracked. Anyone cloning is unaffected.
- `rust-toolchain.toml` pinning `stable` made rustup install 1.99.0 on first use (1.98.1 was the
  default toolchain, not the latest). MSRV 1.90 set with a comment; edition 2024.
- `just` is not installed on this machine; the justfile recipes mirror the exact cargo commands
  run by hand. Install with `cargo install just` or use cargo directly.
- Clippy `all`/`pedantic` are enabled with `priority = -1` so individual allows win. Allowed:
  `module_name_repetitions`, `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`.
- **CI msrv job must call `cargo +<version>`**: `rust-toolchain.toml` pins stable and otherwise wins silently.
- `cargo mutants --in-diff` on PRs needs `fetch-depth: 0` and an explicit fetch of the base branch.
- `deny.toml` with `version = 2` sections avoids deprecated-key warnings on cargo-deny 0.20.x.
- **Skip attribute is plain `#[mutants::skip]`**, with the `mutants` crate as a regular dependency.
  cargo-mutants finds the attribute by scanning source text, so `#[cfg_attr(test, mutants::skip)]`
  (what the plan said) works only by accident and is misleading. Plan corrected.
- `cargo mutants --emit-schema config` prints the supported config keys for the installed version.
- The nightly mutants job never fails the run; the tracking issue "Surviving mutants (nightly)" is the signal. The PR diff job does fail on survivors.

### Model decisions (T-101/T-105)

- `ProgressEntry.at` is `When { Date, DateTime }` so legacy date-only entries round-trip byte-for-byte.
  `Session.at` is a plain `NaiveDateTime` (the script always wrote a time there).
- `Priority::from_str` accepts only `A/B/C` and `#A/#B/#C`; lowercase is rejected because `#a` is a
  topic tag in the file.
- `Status` is a validated newtype (lowercase kebab) with the five defaults as consts; membership is
  checked by `Workflow::parse_status`, which accepts an optional leading `#`.
- `Tag::new` is strict; `Tag::from_str` strips one leading `#` (mirrors `--tag`).
- The default creation note "created via tasks create" is CLI wording, left out of the model.
- `model.rs` is a facade over `model/` (error, id, priority, status, tag, task). For mutants use
  `-f crates/core/src/model.rs -f 'crates/core/src/model/*.rs'` (quote globs under zsh).

### Query decisions (T-104)

- **Id ordering is a total order**: digit-only ids sort numerically and before every other id; the
  rest sort as text. A naive "numeric when both parse, else lexicographic" comparator is not
  transitive and `sort_by` can panic on it since Rust 1.81.
- `Filter::from_word` handles the script's `tasks A` case explicitly: status first, then `A|B|C`
  (with or without `#`) as a priority filter, then a tag. Without that, priority words would
  silently become topic-tag filters matching nothing.
- Grouping keeps tasks whose status is no longer in the workflow: they get their own groups after
  the workflow ones, before "no status", instead of being dropped.
- `next` = first task of the first non-empty group among the first two workflow statuses
  (`NEXT_STATUSES = 2`); `next_from(&[Status])` is the explicit form.

### Mutation-testing learnings

- **`-f` never narrows while `.cargo/mutants.toml` is loaded**: cargo-mutants 27.1 unions the CLI
  filter with `examine_globs` (557 mutants with or without `-f`). Use `--no-config -f <glob>` plus
  the `-E` excludes, wrapped as `just mutants-in <path>` (308 mutants for the format module).

- `exclude_re` for Display/Default/Debug works, but `From<T> for String` / `TryFrom<String>` on
  newtypes are mutated: serde round-trip tests are what kills them.
- Clippy 1.99 added `assert_is_empty` under `all`: write `assert_eq!(v, Vec::new())`, not
  `assert!(v.is_empty())`.

### Findings from reading the script (relevant to T-102/T-103)

- Section insertion rules in the script are **not uniform**: `append_to_section` (before `## Progress`),
  `set_project` (after `## Description` or first), `append_mr_entry` (`### Merge requests` at end
  of `## Related`), `cmd_set` (new `## Tags` appended at end of file). Test each separately.
- Idempotence checks differ per list: worktrees match the path prefix, sessions match the backticked
  id anywhere in the file, MRs match `(url)` inside the subsection.
- The Sessions separator is a real EM DASH (U+2014) with spaces, not a hyphen.
- `### Merge requests` ends at any line starting with `##`, so another `###` also closes it.
- `--due` is stored verbatim by the script; existing non-ISO values must still parse (keep raw string
  on failure or treat as TBD; decided in T-102).
- Open points are listed at the end of `docs/file-format.md`.

## Blockers

- **2026-10-04 OOM incident (resolved).** Three agents building concurrently on a 32-core / 62 GB
  machine (default `cargo` = 32 parallel rustc each, plus cargo-mutants building copies of the
  tree) exhausted memory and killed the GNOME session. The wave 4 agents died with uncommitted but
  green work. Fixes: `.cargo/config.toml` caps `build.jobs = 8`; `scripts/guard` runs commands in
  a systemd scope with `MemoryMax=16G`; all `just` recipes use it; cargo-mutants limited to
  `--jobs 2`; dev profiles use `line-tables-only` debuginfo; **agents now run one at a time** and
  must use the guard for every cargo call.
- **2026-10-04 second OOM incident (resolved).** Root cause found: the cargo-mutants mutant
  `replace + with * in Section::subsections` (format/document.rs) made `end == i`, so the loop
  pushed a `Section` forever and allocated tens of GB in under a second, faster than the mutants
  timeout. Under the guard the cgroup was killed and the run reported "interrupted" (desktop
  survived); one unguarded reproduction run killed GNOME again. Fixes: (1) `scripts/test-runner`
  is cargo's `runner` on Linux and caps every test binary's address space at 4 GiB, so an
  allocation bomb aborts and the mutant is *caught*; (2) `subsections` rewritten as bounded
  iteration over heading positions; (3) guard lowered to `MemoryMax=12G`, `MemorySwapMax=0`.
  Verified: the 13 subsections mutants are all caught in 9 s.

## Deviations from the plan

- T-001: repository URL in `Cargo.toml` is a placeholder (`https://example.invalid/tasq`) until a
  GitHub repo exists. Extra just recipes `default` and `fmt-check`.

## Open questions raised during implementation

(none yet)
