# Progress: tasq rewrite — Phases 0 and 1

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

Phase 2 (nb store) started 2026-10-04. nb is always driven with `NB_DIR` and `NBRC_PATH` pointing
inside a temp copy of a fixture notebook in the repo, never at the real `~/.nb`.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-200 | nb test harness (fixture notebook, NB_DIR isolation, CI nb install) | done | commits 5c0fa3e, e7f663f; nb-gated tests enforced in CI via TASQ_REQUIRE_NB=1 |
| T-201 | Notebook resolution and index reading | done | commit 4d957d9 |
| T-202 | Reading and writing tasks through the store | done | commits 801581d (Store trait), 4d957d9; 79 store-nb tests; mutants 139 tested, 0 missed |
| T-204 | Store capability reporting | done | commit 4d957d9 |
| T-203 | Creating tasks | in progress | wave B |
| T-206 | Bookkeeper (nb CLI vs native) | in progress | wave B |
| T-205 | doctor / config show | in progress | diagnostics functions in wave B, CLI in Phase 3 |

### Phase 0 and 1 status

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-001 | Workspace and quality gates | done | commits cc023aa, 01e2ead; edition 2024, MSRV 1.90 |
| T-002 | CI pipeline | done | commit ab28fc1; cargo-deny 0.20.2 passes locally |
| T-003 | ADRs | done | commit f79c210; 8 ADRs + docs/file-format.md draft |
| T-004 | cargo-mutants setup | done | commit 07d3203; cargo-mutants 27.1.0 |
| T-101 | Domain model | done | commit f9e606c; 59 tests; mutants 121 total, 0 missed, 1 skip (SystemClock::now) |
| T-105 | Clock and progress logging | done | commit f9e606c; `When` enum keeps date-only entries lossless |
| T-102 | Markdown parser | done | commits 5ed472e, 7bd3ff5, dce1403, e9c8779; mutants 269 tested, 0 missed, 0 timeouts |
| T-103 | Markdown writer and round trip | done | commits 5ed472e, 7bd3ff5, dce1403, e9c8779; mutants 269 tested, 0 missed, 0 timeouts |
| T-104 | Queries and grouping | done | commit 72790ee; 29 tests; 0 missed mutants |
| T-106 | Configuration loading | done | commits ce8a7d9, 6171567; 45 tests; mutants 88 tested, 0 missed, 0 timeouts |

## Phase 0 and Phase 1: complete (2026-10-04)

| Metric | Value |
|---|---|
| Commits | 19 on `main` |
| Workspace tests | 196 (core unit 90, config 43, format 63), all passing |
| Mutation testing | model+clock 121, query 161 (crate sweep), format 269, config 88: **0 missed, 0 timeouts** |
| Skips | `SystemClock::now`, `LoadOptions::from_process` (process state only) |
| Gates | fmt, clippy `-D warnings` (pedantic), test, doc with `missing_docs`: clean |

Next: Phase 2 (nb-compatible store, T-201 to T-206). Rules for every future agent: one agent at a
time for anything that compiles; every cargo call through `scripts/guard`; mutants via
`just mutants-in <path>` (or the equivalent `--no-config` command) with `--jobs 2`.

### Config decisions (T-106)

- Layers are partial TOML tables deep-merged key by key (scalars/arrays replace), then
  deserialised once into `Config`; each file is also checked on its own with
  `deny_unknown_fields` to get file:line:column errors.
- Walk-up for `.tasq.toml` stops at the home directory. Profiles are partial configs applied from
  every loaded file in order, only when selected. `Loaded::file_for("store.notebook")` gives
  the file T-201's missing-notebook error must name.
- Additions beyond plan 4.6: `[ui]`, `enabled` on sources, `report.summary.model`, XDG and
  `TASQ_CONFIG` support. Reference: `docs/config.md`.

## Wave plan

1. T-001 alone: everything else needs the workspace. Pre-creates the `justfile` and stub
   modules so later parallel agents own disjoint files.
2. T-002, T-003, T-004 in parallel (disjoint files: `.github/`, `docs/adr/`, `.cargo/mutants.toml` + justfile recipes).
3. T-101 + T-105: the model everything in Phase 1 depends on. Declares `format`, `query`, `config` modules as stubs.
4. T-102+T-103 (one agent), T-104, T-106 in parallel, each owning one module directory.

## Learnings

### Store decisions (T-201/T-202/T-204)

- Conflict detection: a side map of `Revision` (mtime + length + hash) per `TaskId` captured on
  read; `update` refuses with `Conflict` if the file changed. Kept out of `Task` so the model
  stays store-agnostic.
- `update` applies `format::ops` for the differences it can express, re-projects, and returns
  `Unsupported{fields}` if the result still differs (title edits, tag removal, etc.), rather
  than silently dropping changes. No write when the rendered text is unchanged.
- The store resolves the `nb` executable from the injected env's PATH itself; tests and the CLI
  are explicit about which nb runs. No process is spawned when `$NB_DIR/<notebook>` exists.
- Added `format::ops::set_open` (reopen) to core.

### nb facts (probed 2026-10-04 with nb 7.25.4 in an isolated NB_DIR)

- **nb needs a git identity**: with `HOME` pointing at a dir without `.gitconfig`, every nb
  command prints the welcome and does nothing. The harness writes one.
- `nb notebooks show <name> --path` and `nb todo do` need the notebook to be a git repo; listing
  and `nb index reconcile` work without `.git`.
- `nb index reconcile` on a missing index renumbers ids (observed 3→6, 7→5): the warning is needed.
- `nb index verify` prints "Index corrupted" for a missing file but still exits 0.

- `NB_DIR` and `NBRC_PATH` fully isolate nb; `NB_AUTO_SYNC=0` prevents remote sync attempts.
- On a fresh `NB_DIR` the first nb command prints a welcome and initializes `home` (a git repo
  with an `[nb] Initialize` commit); that first command's own action may be swallowed. Initialize
  explicitly before use.
- Todo files are `YYYYMMDDHHMMSS.todo.md`; same-second collisions bump to the next second.
- `.index`: one filename per line, new files appended, id = line number. `nb index add <file>`
  appends; `nb index verify` exits 0 when consistent; `nb git dirty` exits 0 when uncommitted;
  `nb git checkpoint "<msg>"` commits with that message. nb's own messages: `[nb] Add: <file>`,
  `[nb] Done: <file>`.
- `nb todo do <id>` only flips `# [ ]` to `# [x]` on the title line and commits.
- `nb todo add --tags ready,A` writes `\n## Tags\n\n#ready #A\n`, the same shape our format uses.
- `nb notebooks show <name> --path` prints a clean absolute path (no escapes) in this environment;
  keep the sanitizing anyway, the script needed it elsewhere.

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

### Format decisions (T-102/T-103)

- Non-ISO `## Due`: `task.due = None`, raw line preserved in the document.
- `## Source` grammar: one line `source: external-id [url]`; a single `http(s)://` value is both id
  and url. Written after `## Due` by `Document::from_task`.
- CRLF: each line keeps its own ending; new lines use the first line's ending. Mixed endings survive.
- `##A` / `##ready` are topic tags, not priority/status (the script's regexes required one `#`).
- `set` without a Tags section appends `## Tags` at end of file, like the script.
- Edit API is operation-based (`format::ops::*`), one function per awk pass of the script.

### Mutation-testing learnings

- Equivalent mutants are better removed by simplifying the code shape than argued in a report
  (two cases in format: an unreachable guard and a redundant newline choice).
- Proptest round-trip tests cannot kill writer mutants whose output parses back to the same value;
  add exact-string assertions for those.
- Shared helpers (`Document::body_range`, `set_body`) make arithmetic mutants catchable by every
  fixture round trip at once.

- A full format-module run (301 mutants) takes 3 minutes with `--jobs 2` under the guard: fast
  enough to run per task. Timeouts come from `i += 1` → `i -= 1` style loops; bounded `for`
  iteration turns those into caught mutants.

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
