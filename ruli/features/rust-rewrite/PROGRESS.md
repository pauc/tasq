# Progress: tasq rewrite

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

**Phase 3 (CLI parity) complete (2026-10-04).** Every command of the script except `next`/`pick`
(Phase 4), `update`/`update-support` (Phase 5 `sync`), `summary` (Phase 6) and `tlogs` (external
plugin) exists under `tasq`, each with `--json`. 468 workspace tests (cli 49 unit + 66
integration). Mutants: core `work` 9/0 missed, `clock` 22/0, `dates` 8/0, model 0 missed; launch
crate 0 missed (process wrappers skipped). Next: Phase 4 (launchers, `next`/`pick`, T-401 to
T-405).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-301 | CLI skeleton, output modes, errors | done | 27 unit + 37 integration tests (`crates/cli/tests/cli.rs`, insta snapshots) |
| T-302 | `list` (default command) | done | grouped/single-status/tag/priority views, `--json` |
| T-205 | `doctor` / `config show` commands | done | config failures are reported as a FAIL check, not a crash |
| T-204 | `store info` | done | plus `store sync` from T-206 |
| T-303 | `create` | done | `--due` words via `tasq_core::dates::parse_day` (mutants 8 tested, 0 missed); `--mr` falls back to a `group/project!123` label until T-307 |
| T-304 | `set`, `log`, `done` | done | whole-task `Store::update`; `TASQ_NOW` fixes timestamps in tests |
| T-305 | `view` | done | `linkify_pre/post`, `unwrap_urls`, `shortref` ported as pure functions with unit tests; `--raw`; glow only on a TTY |
| T-306 | `project`, `worktree` | done | `tasq_core::work` (trait + pure helpers), `tasq_launch::worktree` (`GwmManager`, `GitManager`, fake gwm + real git tests) |
| T-307 | `session`, `mr` | done | idempotent; resume hint CLI-side until the `Launcher` trait exists; MR title fallback from T-303 |
| T-308 | `apply` | done | `{"schema":1,"task":{...}}` on stdin or a file; `docs/json.md`; `view --json \| apply` is a no-op (mtime-checked) |

### Phase 2 status

**Phase 2 complete (2026-10-04).** 29 commits; 331 workspace tests (nb-gated ones enforced with
`TASQ_REQUIRE_NB=1`); store-nb mutants 270 tested, 0 missed, 0 timeouts (skips only on the four
process spawners `Nb::run/run_in/version`, `Git::run`); fmt, clippy, doc clean. Next: Phase 3
(CLI parity, T-301 to T-308), which also wires `tasq doctor` and `tasq config show` (T-205).

Phase 2 (nb store) started 2026-10-04. nb is always driven with `NB_DIR` and `NBRC_PATH` pointing
inside a temp copy of a fixture notebook in the repo, never at the real `~/.nb`.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-200 | nb test harness (fixture notebook, NB_DIR isolation, CI nb install) | done | commits 5c0fa3e, e7f663f; nb-gated tests enforced in CI via TASQ_REQUIRE_NB=1 |
| T-201 | Notebook resolution and index reading | done | commit 4d957d9 |
| T-202 | Reading and writing tasks through the store | done | commits 801581d (Store trait), 4d957d9; 79 store-nb tests; mutants 139 tested, 0 missed |
| T-204 | Store capability reporting | done | commit 4d957d9 |
| T-203 | Creating tasks | done | commit 5ef8efc |
| T-206 | Bookkeeper (nb CLI vs native) | done | commit 1d36cf8 |
| T-205 | doctor / config show | library done | commit 95ec027 (`doctor::checks`, `tool_check`); CLI command in Phase 3 |

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

### CLI decisions (T-301/T-302/T-205)

- Crate layout: `src/lib.rs` (`tasq_cli`, named so `cargo doc` does not collide with the binary)
  holds `cli` (clap derive), `app` (config → store wiring), `output` (colour, pager, JSON),
  `error`, `json` and one module per command under `commands/`. `main.rs` is one line.
- Exit codes: 0; 1 for user errors (`tasq: <message>`); **2 for clap usage errors and internal
  errors** (clap prints its own message with usage). `tasq doctor` with a FAIL returns
  `CliError::Silent(1)` so the checks are the only output.
- `tasq <word>` is a root positional; `tasq list <word>` and the explicit `--status/--tag/--prio/
  --text` flags combine with it. `args_conflicts_with_subcommands` cannot be used: it also
  conflicts the global flags (`tasq --json store info` fails), so a word plus a subcommand is
  rejected in `app::run` instead.
- Every `--json` document is an object carrying `"schema": 1` (`{"schema":1,"tasks":[...]}` for
  `list`, not the bare array the plan sketched), so one envelope serves `apply` too. Tasks are
  serialised as the core `Task`; `Session.at` now serialises as `YYYY-MM-DD HH:MM` like progress
  entries (`clock::timestamp_serde`), so every timestamp in a document has the file's shape.
- `list` parity details kept from the script: `[%2s]` id padding, chips with a space of padding
  (a trailing space without colour), a bare blank line after each group, and the one-status view
  printing the lowercase status as its header, uncoloured. Deviation: an empty single-status or
  priority view prints `No open todos with status x.` / `with priority #A.` where the script
  printed nothing. Header colours come from the five defaults (blue/green/yellow/red/magenta),
  cyan for any other configured status, dim for `NO STATUS`, overridable under `[ui.colors]`
  by status name (`no-status` for the last group), values `red`...`white`, `dim`, or `0`-`255`.
- Colour: `--color auto|always|never`, `--no-color`, `NO_COLOR`; auto only on a TTY. Pager: only
  on a TTY, `ui.pager` split with `shell-words` (no shell), `cat`/empty disables, a pager that
  fails to spawn is a warning and the text prints directly. Only `list`, `doctor` and
  `config show` page.
- `config show` prints the effective config as TOML with a `# <origin>` comment on every leaf
  (`Loaded::explain`) and the layer list as a header; the text is valid TOML that reads back
  into the same `Config` (unit-tested). Top-level tables follow the struct order.
- Test harness (`tests/support`): a temp copy of the store-nb fixture notebook, `HOME` with a
  `.gitconfig`, and `PATH` = a dir holding only a `git` symlink, so output is identical whether
  or not nb is installed (doctor's `nb` check is always WARN there). Snapshots normalise the temp
  root to `[ROOT]`; `INSTA_UPDATE=always cargo test -p tasq` accepts them (no `cargo insta`).
  `assert_cmd::Command::new(env!("CARGO_BIN_EXE_tasq"))` avoids the deprecated `cargo_bin`.
- Manual TTY check with `script -qec` (not covered by tests): colour, pager, pager fallback.
- `TASQ_NOW="YYYY-MM-DD HH:MM"` (env, not a config key) makes `App::clock()` a `FixedClock`
  and is also handed to the store (`NbStore::with_clock`), so tests pin progress timestamps
  and new filenames (`20261007093000.todo.md`, next second when taken). Documented in
  `docs/testing.md`.
- `create --mr <url>`: the store refuses unlabelled merge requests, and the forge title lookup
  is T-307/T-503, so `commands::mr::link_for` labels GitLab/GitHub URLs with their short
  reference (`group/project!77`, `owner/repo#7`) and warns; other URLs need an explicit
  title. The lookup slots in front of the fallback later.

### CLI decisions (T-305 to T-308)

- **Minute precision is a model invariant.** `Store::update` compares the re-read task with the
  wanted one field by field; a progress entry stamped with seconds never matched the file's
  `HH:MM` and would have made every real-clock `log`/`set` fail with `Unsupported{progress}`
  (the tests passed only because `TASQ_NOW` has no seconds). `When::from(NaiveDateTime)`,
  `ProgressEntry::new` and the new `Session::new` now truncate via `clock::to_minute`;
  `store-nb/tests/write.rs` has the regression test with a 42-second clock.
- `view`: glow runs only when stdout is a TTY and `glow` is on the injected `PATH`; otherwise
  the file is printed (paged on a TTY). `--raw` is always verbatim. The awk passes are
  `commands::view::{linkify_pre, linkify_post, unwrap_urls, shortref}`; a few awk quirks are
  kept deliberately and pinned by tests (a lone URL ending in `.` leaves the `.` as its own
  line; an empty link label becomes `****`). Width: `$COLUMNS`, else `tput cols`, else 100.
  The script's `tasks view <id> [nb args]` passthrough is gone (`--raw` instead).
- `worktree --create`: `tasq_core::work::WorktreeManager` (trait, `CreatedWorktree`, `WorkError`,
  `find_up`, `branch_slug`, `sibling_worktree`, `project_dir`) with the process-running impls
  in `tasq-launch` (`process::run` is the only `#[mutants::skip]`). `GwmManager` reproduces the
  script: `gwm.yml` found upwards from the project, `-b` only when the branch exists neither
  locally nor on `origin`, `GWM_SHELL_MODE=1 gwm create [-b] <branch> --no-tmux -s` in the
  project, last output line = path, other lines shown to the user. `GitManager` (new, for
  people without gwm) uses `git worktree add` into `<project>-<branch-slug>` next to the
  project and reuses the directory when it exists.
- `session`: the file never stores a launcher, so `Session.launcher` stays `None` on write (a
  `Some` would be `Unsupported`); the resume hint (`claude --resume <id>`, `tmux attach -t`)
  comes from `--launcher`/`launch.default` in `commands::session::resume_hint`, to move onto the
  `Launcher` trait in Phase 4. Session ids may not contain backticks (the file delimiter).
- `apply` reads the `view --json` envelope only (`schema` must be 1, `task` required); errors
  are prefixed `apply:` and name the field via serde's message. Unsupported edits surface the
  store's `unsupported: changing title of task 3 (...)` message unchanged.
- Test harness: `TestEnv::fake_tool(name, body)` installs scripts in the `PATH` dir; fakes must
  use absolute paths for anything that is not a shell builtin (`/bin/mkdir`), because that
  `PATH` holds only `git`. `cargo test --workspace` stops at the first failing test binary; use
  `--no-fail-fast` to see every crate.

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

### Bookkeeper and create decisions (T-203/T-206)

- Filename: `YYYYMMDDHHMMSS.todo.md` from the injected clock, bumped one second while taken, error
  after 60 attempts with nothing written.
- Merge requests need a label at the store level (`- [title](url)`); the CLI resolves titles.
- Default creation note: `created via tasq create`.
- `nb git checkpoint` is always run with `--wait` and immediately after the write; nb pushes when
  the user's `auto_sync` is on (kept, per ADR 0007).
- Register failure after a written file is an error naming the file (no id exists yet); later
  checkpoint failures are warnings carrying the manual fix (`nb index reconcile`).
- `Bookkeeper::checkpoint -> bool` (committed or skipped), `verify -> Verification`, plus `sync`.

### nb facts (probed 2026-10-04 with nb 7.25.4 in an isolated NB_DIR)

- **`nb git checkpoint` commits asynchronously unless `--wait`**; and any nb read command
  (`nb todos` included) commits a dirty notebook in the background as `[nb] Commit`. Checkpoint
  right after writing, with `--wait`, or nb takes the commit with its own message.
- `nb index verify` exits 1 with "Index corrupted" on stderr in this version (an earlier probe saw
  exit 0): parse the text, not just the exit code.
- `nb index add` appends `basename\n` and skips names already listed; it does not repair a missing
  trailing newline on the previous line.
- `nb git dirty` exits 1 both when clean and when the notebook is not a git repo; check `.git` first.
- `nb sync` without a remote exits 1 ("No remote configured"); reported as not synced, not an error.
- A cwd inside a notebook makes nb treat it as the current notebook: that is how `git`/`sync`
  (no folder argument) are targeted; index subcommands take the folder as final argument.
- Test harness: a freshly written fake executable can hit `ETXTBSY` when other test threads fork;
  probe it until it runs.

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

- T-301: usage errors exit 2 (clap convention), not 1; only domain errors exit 1 with `tasq: ...`.
- T-302: `--json` emits `{"schema":1,"tasks":[...]}` rather than a bare array (FR-4 asks for a
  versioned schema on every command).
- T-305: no nb passthrough (`tasks view <id> [args]`); `--raw` prints the file instead.
- T-306: a `git` worktree manager exists besides `gwm` (config `work.worktree_manager = "git"`),
  placing worktrees at `<project>-<branch-slug>`; the plan only described gwm.
- T-307: the resume hint is a CLI table keyed by launcher name until Phase 4 adds the trait.
- T-001: repository URL in `Cargo.toml` is a placeholder (`https://example.invalid/tasq`) until a
  GitHub repo exists. Extra just recipes `default` and `fmt-check`.

## Open questions raised during implementation

(none yet)
