# PLAN: `tasq` — rewrite of the `tasks` script as a Rust terminal task manager

> Placeholder name: **tasq** (binary `tasq`, crates `tasq-*`). See "Naming" for alternatives.
> The existing `original/tasks` bash script, its nb notebooks and the current Claude skills
> (`/wrapup`, `/update-tasks`, `/update-support-tasks`, `/time-logs`) are **never modified**
> by this project. The new tool ships under a new binary name and a new Claude Code plugin,
> so both systems run side by side until you choose to switch.

## 1. Introduction

`original/tasks` is a ~1200-line bash script that manages todos stored as nb markdown files.
It mixes three concerns in one file:

1. **A task store**: nb todo files (`# [ ] Title` + `## Description/Project/Due/Related/Tags/Progress/Worktrees/Sessions` sections), with status and priority encoded as tags, and ids taken from nb's `.index`.
2. **A terminal UI**: status-grouped list, glow rendering, OSC 8 links, paging, standup summary.
3. **Personal integrations**: Claude Code sessions, herdr workspaces, gwm worktrees, direnv, glab, GitLab/Slack/Gmail/Freshdesk refreshes via Claude slash commands, HiBob time logs.

The rewrite separates these into a **core library** with a small, well-tested domain model and
three trait-based extension points (**Store**, **Source**, **Launcher**), consumed by a **CLI**
first and a **ratatui TUI** second. The result should be a tool other people can install and
extend, while the author keeps every workflow the script offers today.

## 2. Goals

- A `tasq-core` crate with no terminal, network or process dependencies in its public model, covered by unit tests.
- Day-one storage adapter that reads and writes **existing nb notebooks unchanged** (same files, same ids, nb keeps working alongside), behind a `Store` trait so SQLite/other stores can be added later without touching the CLI or TUI.
- External sources (GitLab, GitHub, Slack/Gmail via an LLM bridge) behind a `Source` trait; **each adapter decides** whether it is deterministic, LLM-driven or hybrid. The core only reconciles the items it gets back into tasks.
- Session launching behind a `Launcher` trait: plain shell, Claude Code, tmux, herdr. The core models "work context" (project dir, worktrees, sessions) itself.
- CLI functional parity with the script (`list/create/set/log/done/view/project/worktree/session/mr/next/pick/sync/summary`), every command with `--json` output.
- Configuration in TOML (global + per-project discovery + env overrides) instead of `TASKS_*` env vars.
- A ratatui TUI built only on `tasq-core`, proving the core/UI boundary.
- A Claude Code plugin shipping the agent-side skills (`wrapup`, `sync`) for the new tool.
- CI (fmt, clippy, tests, MSRV), release binaries, README and docs good enough to publish.
- A documented plugin-mechanism decision (ADR) taken **after** the CLI exists, with the architecture already keeping both in-process and out-of-process plugins possible.

## 3. Delivery order and rationale

You asked for the order that yields the cleanest, most maintainable code. Recommendation:

1. **Core library first** (model, markdown format, Store trait, nb store), fully tested with fixtures. This is where correctness lives and where a bad model would cost the most later.
2. **CLI as the first consumer**, reaching parity so you can dogfood early. Writing the CLI against the library immediately exposes leaks in the core API.
3. **Sources and launchers** as adapters, each landing with tests (HTTP mocks, fake launchers).
4. **TUI as the second consumer**. A second consumer is the best test of the boundary; building it last means it never drives the model.
5. **Plugin mechanism** decided once we know which commands plugins actually need.

Phases are sequential; tasks inside a phase can be parallelized.

## 4. Architecture

### 4.1 Workspace layout

```
tasq/
├── Cargo.toml                 # workspace, shared lints, MSRV, deny warnings
├── crates/
│   ├── core/                  # tasq-core: domain, traits, config, reconciliation, reports
│   ├── store-nb/              # tasq-store-nb: nb-compatible markdown Store
│   ├── sources/               # tasq-sources: gitlab, github, llm-bridge (feature-gated)
│   ├── launch/                # tasq-launch: shell, claude, tmux, herdr Launchers
│   ├── cli/                   # tasq: the binary (clap, rendering, pager, OSC 8, --json)
│   └── tui/                   # tasq-tui: ratatui app (depends on core only)
├── plugins/
│   └── claude/                # Claude Code plugin: skills + commands for the new tool
├── docs/
│   ├── adr/                   # architecture decision records
│   ├── file-format.md         # the markdown task format, normative
│   └── plugins.md             # plugin API once decided
├── original/tasks             # the bash script, kept read-only as reference
└── ruli/features/rust-rewrite/PLAN.md
```

### 4.2 Core domain (`tasq-core`)

```rust
pub struct Task {
    pub id: TaskId,                 // store-local, opaque (nb index line number today)
    pub title: String,
    pub done: bool,
    pub status: Option<Status>,     // configurable workflow; default in-progress/ready/waiting/blocked/later
    pub priority: Priority,         // A | B | C, default B
    pub due: Option<NaiveDate>,
    pub description: Option<String>,
    pub project: Option<PathBuf>,
    pub tags: Vec<Tag>,             // topic tags only; status/priority are NOT tags in the model
    pub related: Vec<Link>,         // url + optional label
    pub merge_requests: Vec<Link>,
    pub worktrees: Vec<Worktree>,   // path + optional branch
    pub sessions: Vec<Session>,     // timestamp, session id, launcher kind, description
    pub progress: Vec<ProgressEntry>, // timestamp + note
    pub origin: Option<Origin>,     // source name + external id/url, for reconciliation
}
```

Core services (pure where possible):

- `format`: parse/serialize the markdown format, lossless for unknown sections (round-trip tested).
- `query`: filter by status/tag/priority/text, group by status, sort (prio, due, id), `next()` selection.
- `workflow`: status/priority transitions, `done` semantics (strip status), progress logging with injected clock.
- `reconcile`: merge `SourceItem`s into tasks (create / update / close / mark-for-review), deterministic and unit tested.
- `report`: `Report` trait (`summary` default), `Summarizer` trait (raw passthrough vs LLM).
- `config`: layered TOML loading, profile selection, validation with helpful errors.

Extension traits:

```rust
pub trait Store {
    fn list(&self, filter: &Filter) -> Result<Vec<Task>>;
    fn get(&self, id: &TaskId) -> Result<Task>;
    fn create(&mut self, draft: TaskDraft) -> Result<Task>;
    fn update(&mut self, task: &Task) -> Result<()>;   // whole-task write; adapters diff as needed
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<()>;
    fn describe(&self) -> StoreInfo;                    // name, location, capabilities
}

pub trait Source {
    fn name(&self) -> &str;
    fn fetch(&self, ctx: &SyncContext) -> Result<Vec<SourceItem>>;        // full sweep
    fn check(&self, items: &[Origin]) -> Result<Vec<SourceItemState>>;    // re-check specific tasks
}
// SourceItem: external id, url, title, suggested status/tags/priority, body, state (open/merged/closed/needs-attention)

pub trait Launcher {
    fn name(&self) -> &str;
    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome>;        // workdir, env, prompt, task
}
```

### 4.3 How the deferred plugin decision stays open

- Everything the CLI can do is a `tasq-core` function; the CLI is thin. In-process plugins (Rust traits behind cargo features, WASM) can target the library.
- Every CLI command supports `--json` with a versioned schema, and `tasq` accepts task edits via JSON on stdin (`tasq apply`). Out-of-process plugins (`tasq-foo` executables, scripts) can target the binary.
- Adapters are registered through a `Registry` built at startup from config, so the registration mechanism can later be fed by dynamic discovery without changing the traits.

The decision is captured in T-901 with three options and a recommendation. Note that the GPL
license already tilts third-party plugins toward the out-of-process model: an executable that
talks JSON to `tasq` is not a derived work, while a Rust crate linked into the binary is.

### 4.4 Markdown format (nb-compatible)

Normative spec in `docs/file-format.md`. Day one keeps the exact shape the script writes:

```
# [ ] Title

## Description
...
## Project
/abs/path
## Due
2026-10-10
## Related
- https://...
### Merge requests
- [title](url)
## Tags
#gitlab #A #ready
## Progress
- 2026-10-04 10:15: note
## Worktrees
- /path (`branch`)
## Sessions
- 2026-10-04 10:15: `session-id` — desc
```

Additions, all optional and ignored by nb and the old script: a `## Source` section
(`gitlab: https://.../merge_requests/123`) for reconciliation, and HTML comments for
metadata the model needs but humans should not see (`<!-- tasq: {...} -->`). Unknown
sections are preserved verbatim.

### 4.5 Relationship with nb: hybrid

nb (AGPLv3, bash) already solves bookkeeping we do not want to re-implement: the `.index`
id mapping, filename generation rules, git auto-commit (`nb git checkpoint`) and remote sync
(`nb sync`, `auto_sync`). nb is also slow to invoke (a bash startup per command), which is
exactly what makes the current script sluggish on listing.

Decision (ADR 0007):

- **Read natively.** `list`, `view`, `next` parse `.index` and the markdown files directly.
  No nb process is spawned on the hot path.
- **Write files natively.** Edits are atomic rewrites of the single task file; the format
  layer guarantees nb can still read it.
- **Delegate bookkeeping to nb when present.** After `create`, run `nb index add <file>`;
  after any write, run `nb git checkpoint "<message>"` (which also pushes when nb's
  `auto_sync` is on). This is a `Bookkeeper` strategy (`NbCli`) so the exact index and git
  semantics are nb's, not ours.
- **Native fallback.** Without nb installed, `NativeBookkeeper` appends to `.index` and
  commits with git (or does nothing when the notebook is not a repo). This is what makes the
  tool usable for people who do not run nb.
- **Never reorder or rebuild the index** ourselves; `tasq doctor` suggests
  `nb index reconcile` when `nb index verify` fails.

Store config: `bookkeeper = "auto" | "nb" | "native"` (auto picks nb when found on PATH).

### 4.6 Configuration

`~/.config/tasq/config.toml` plus the first `.tasq.toml` found walking up from cwd (or a
`[profile]` chosen by `--profile`/`TASQ_PROFILE`). Env vars override individual keys.

```toml
[store]
kind = "nb"
notebook = "home"            # resolves ~/.nb/<notebook> or asks nb
bookkeeper = "auto"          # auto | nb | native — who maintains .index and git commits

[workflow]
statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
default_status = "ready"

[work]
default_project = "~/code/SF/silverfin_worspace/silverfin"
worktree_manager = "git"     # git | command
# worktree_command = "gwm create {new} {branch} --no-tmux -s"

[launch]
default = "claude"           # shell | claude | tmux | herdr
env = "direnv"               # inherit | direnv

[forge.gitlab]                # shared client config, used by every gitlab-* source
host = "gitlab.silverfin.com"
token_cmd = "glab auth token"

[[source]]
name = "gitlab-review-requests"
kind = "gitlab-review-requests"
forge = "gitlab"
tags = ["gitlab", "review-request"]
status = "ready"

[[source]]
name = "gitlab-work-items"
kind = "gitlab-work-items"     # issues / work items assigned to me
forge = "gitlab"
tags = ["gitlab"]
status = "later"

[[source]]
name = "inbox"
kind = "llm-bridge"
command = "claude -p --output-format json"
prompt_file = "~/.config/tasq/prompts/inbox.md"

[report.summary]
summarizer = "llm"           # raw | llm
command = "claude -p --model sonnet"
```

## 5. Tasks

### Phase 0 — Bootstrap

### T-001: Create the cargo workspace and quality gates
**Description:** Initialize the git repo (the current directory is not one yet), create the workspace with the six crates as empty libraries/binary, shared lint configuration, MSRV, and a `justfile` or `cargo xtask` for common commands.

**Acceptance Criteria:**
- [ ] `git init` done; `original/tasks` committed untouched as reference
- [ ] Workspace builds with `cargo build --workspace`; crates: `core`, `store-nb`, `sources`, `launch`, `cli`, `tui`
- [ ] `[workspace.lints]` enables `clippy::all`, `clippy::pedantic` (allow-listed exceptions documented), `unsafe_code = "forbid"`
- [ ] `rust-toolchain.toml` pins stable; `rust-version` set in workspace
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all pass
- [ ] `.gitignore`, `LICENSE` = **GPL-3.0-or-later** (nb is AGPLv3; the Affero clause only matters for network services, so plain GPLv3 is the conventional equivalent for a CLI), `CHANGELOG.md` present
- [ ] Every crate's `Cargo.toml` has `license = "GPL-3.0-or-later"`; `cargo-deny` config allows our MIT/Apache/BSD dependencies (one-way compatible with GPL)

### T-002: CI pipeline
**Description:** GitHub Actions workflow running fmt, clippy, tests on Linux and macOS, plus `cargo-deny` for licenses/advisories.

**Acceptance Criteria:**
- [ ] Workflow runs on push and PR; matrix: ubuntu-latest, macos-latest
- [ ] Jobs: fmt, clippy (deny warnings), test, cargo-deny, coverage (`cargo-llvm-cov`, uploaded as artifact/Codecov)
- [ ] Mutation job (see T-004): `cargo mutants --in-diff` against the PR base on every PR; full run on `main` nightly
- [ ] Badge in README

### T-003: Architecture decision records
**Description:** Write the first ADRs capturing the decisions already taken so contributors understand the constraints.

**Acceptance Criteria:**
- [ ] `docs/adr/0001-rust.md`, `0002-nb-compatible-markdown-store.md`, `0003-store-source-launcher-traits.md`, `0004-adapter-decides-sync-strategy.md`, `0005-cli-first-tui-second.md`
- [ ] `docs/adr/0006-plugin-mechanism.md` created as "Proposed", listing the three options (see T-901)
- [ ] `docs/adr/0007-nb-under-the-hood.md`: read natively, write files natively, delegate index and git bookkeeping to nb when installed (see T-203/T-206)
- [ ] `docs/adr/0008-license.md`: GPL-3.0-or-later and what it means for plugins (in-process plugins must be GPL-compatible; out-of-process plugins talking JSON are unaffected)
- [ ] ADR template committed

### T-004: Mutation testing with cargo-mutants
**Description:** Use `cargo-mutants` to measure test quality, not just line coverage: it rewrites each function (return defaults, flip conditions, drop statements) and expects at least one test to fail. Configure it so it is fast enough to run on PRs and strict on the crates where logic lives.

**Acceptance Criteria:**
- [ ] `.cargo/mutants.toml` committed: `examine_globs` for `crates/core`, `crates/store-nb`, `crates/sources`, `crates/launch`; `exclude_globs` for `crates/tui` view code and `crates/cli` arg plumbing (covered by snapshot tests instead); `timeout_multiplier = 3`
- [ ] Functions that only wrap I/O or `exec` (process replacement, terminal setup) are annotated `#[mutants::skip]` (plain form; the `mutants` crate is a regular dependency because cargo-mutants reads the attribute from source text and never evaluates `cfg_attr`) with a one-line reason
- [ ] `just mutants` runs locally with `--jobs` auto-detected and `--in-diff` against `main`; `just mutants-full` runs everything
- [ ] CI PR job fails when a mutant in the diff survives; nightly full run opens/updates a tracking issue listing survivors
- [ ] Target recorded in README: **zero missed mutants** in `tasq-core::format`, `query`, `reconcile`, `config`, `dates`; ≤5% missed across `core` as a whole
- [ ] Each phase's "done" review includes looking at the survivors list and either adding a test or documenting why the mutant is equivalent

### Phase 1 — Core domain and file format

### T-101: Domain model
**Description:** Implement `Task`, `TaskDraft`, `Status`, `Priority`, `Tag`, `Link`, `Worktree`, `Session`, `ProgressEntry`, `Origin`, with a `Workflow` type that owns the configurable status list.

**Acceptance Criteria:**
- [ ] Types live in `tasq-core::model`, derive `Debug, Clone, PartialEq, Serialize, Deserialize`
- [ ] `Status` is validated against a `Workflow`; `Priority::default() == B`
- [ ] Status and priority are **not** stored in `tags`; the format layer maps them to/from `#tags`
- [ ] Unit tests for `Workflow::parse_status`, `Priority::from_str` (`A`, `#A`), `Tag::new` rejects `#` prefix and whitespace
- [ ] `cargo mutants -f crates/core/src/model` reports no missed mutants

### T-102: Markdown format parser
**Description:** Parse an nb todo file into a `Task` plus a `Document` structure that keeps unknown sections and ordering so the file can be rewritten losslessly.

**Acceptance Criteria:**
- [ ] Parses every section the script writes (4.4), including `### Merge requests` nested in `## Related`
- [ ] Extracts status/priority from the `## Tags` line; remaining tags become topic tags
- [ ] Parses progress entries `- YYYY-MM-DD HH:MM: note` and legacy `- YYYY-MM-DD: note`
- [ ] Parses worktrees `- /path (`branch`)` and sessions `- date: `id` — desc`
- [ ] Unknown sections and unknown lines are kept in `Document` verbatim
- [ ] Fixture tests under `crates/core/tests/fixtures/*.md` covering: minimal, full, done (`# [x]`), no Tags section, no status tag, extra sections, CRLF line endings
- [ ] Non-todo markdown returns `Err(NotATask)` and never panics (fuzz-style test over random inputs with `proptest`)
- [ ] `cargo mutants -f crates/core/src/format` reports no missed mutants after T-103

### T-103: Markdown format writer and round trip
**Description:** Serialize a `Task` back into its `Document`, creating missing sections in canonical order (the order the script uses) and editing existing ones in place.

**Acceptance Criteria:**
- [ ] `parse(write(parse(x))) == parse(x)` for every fixture
- [ ] `write(parse(x)) == x` byte-for-byte for fixtures produced by the script (no reformatting of untouched files)
- [ ] Setting status replaces only the status tag; priority likewise; topic tags keep order
- [ ] Appending a progress note adds after the last `- ` entry of `## Progress`, creating the section if absent
- [ ] New sections are inserted before `## Progress` when it exists, else appended (matches `append_to_section`)
- [ ] `done` flips `# [ ]` to `# [x]` and removes the status tag; the `## Tags` line is dropped if it becomes empty

### T-104: Queries and grouping
**Description:** Pure functions over `&[Task]`: filter by status/tag/priority/done/text, group by status in workflow order plus `no-status`, sort by (priority, due-or-max, id), and `next()` = first of in-progress then ready.

**Acceptance Criteria:**
- [ ] `Filter` builder with `status`, `tag`, `priority`, `done`, `text`
- [ ] `group_by_status` returns groups in workflow order; empty groups omitted
- [ ] Sort order verified by test: A before B before C, earlier due first, missing due last, then id ascending
- [ ] `next(tasks)` returns `None` when no in-progress/ready task

### T-105: Clock and progress logging
**Description:** Inject time via a `Clock` trait so progress entries and session entries are deterministic in tests.

**Acceptance Criteria:**
- [ ] `Clock` trait with `SystemClock` and `FixedClock`
- [ ] `Task::log(note, &clock)` appends `ProgressEntry { at, note }`
- [ ] Timestamps serialize as `%F %H:%M` local time

### T-106: Configuration loading
**Description:** Implement layered config: defaults → `~/.config/tasq/config.toml` → nearest `.tasq.toml` up from cwd → env (`TASQ_*`) → CLI flags. Profiles select named sub-configs.

**Acceptance Criteria:**
- [ ] `Config` deserialized with `serde` + `toml`; unknown keys are errors with file and line
- [ ] Path values expand `~`
- [ ] `Config::load(cwd, env)` is pure given injected env and filesystem (use `tempfile` dirs in tests)
- [ ] `tasq config show` (T-205) prints the effective config and where each value came from
- [ ] Error for a missing notebook names the setting and the file that set it

### Phase 2 — nb-compatible store

### T-201: Notebook resolution and index reading
**Description:** Implement `NbStore::open(config)`: resolve `notebook` to `$NB_DIR/<name>` or fall back to `nb notebooks show <name> --path` (sanitizing escape sequences as the script does), read `.index`, map line numbers to `TaskId`.

**Acceptance Criteria:**
- [ ] Resolves a notebook directory without invoking `nb` when `$NB_DIR/<name>` exists
- [ ] Falls back to `nb` and strips ANSI/CR noise; unit test with a fake `nb` on PATH
- [ ] Missing `.index` → attempts `nb index reconcile`, warns that ids may have changed, errors if still missing
- [ ] `list` skips index lines that are not `*.todo.md` or whose file is gone, like `all_open`
- [ ] Integration test against a fixture notebook copied to a temp dir

### T-202: Reading and writing tasks through the store
**Description:** Implement `get`, `list`, `update`, `set_done` using the format crate; atomic writes via temp file + rename.

**Acceptance Criteria:**
- [ ] `update` rewrites only the task's file; other files untouched (test asserts mtimes)
- [ ] Writes are atomic (`tempfile::NamedTempFile::persist` in the same directory)
- [ ] `set_done(true)` produces the same file `nb todo do` would produce for the fixture (compare against a captured nb output)
- [ ] Concurrent-edit protection: `update` fails with `Conflict` when the file changed since `get` (mtime + hash)

### T-203: Creating tasks
**Description:** Implement `create`: generate an nb-style filename (`YYYYMMDDHHMMSS.todo.md`, verified against nb's own rule), write the document atomically, then hand the file to the `Bookkeeper` (T-206) to register it in `.index` and commit. Return the new id by reading the index back.

**Acceptance Criteria:**
- [ ] Filename never collides (same rule nb uses; verified by reading nb's `_add` implementation and encoded as a test)
- [ ] After `create`, `nb todos` lists the task with the same id `tasq` returned (integration test runs only when `nb` is installed, `#[ignore]` otherwise)
- [ ] `--status done` creates a `# [x]` task without a status tag
- [ ] With the native bookkeeper the result is byte-identical in `.index` to what `nb index add` produces (fixture comparison)

### T-206: `Bookkeeper` strategy (nb CLI vs native)
**Description:** Trait with `register(file)`, `checkpoint(message)`, `verify()`; `NbCli` impl shells out to `nb index add`, `nb git checkpoint`, `nb index verify` with the notebook path; `Native` impl appends to `.index` and commits via `git` when the notebook is a repo. `auto` picks `NbCli` when `nb` is on PATH.

**Acceptance Criteria:**
- [ ] nb output is sanitized (ANSI/CR) and never shown to the user unless `-v`
- [ ] `checkpoint` is skipped when nothing changed (`nb git dirty` / `git status --porcelain`)
- [ ] Commit messages follow nb's style: `[tasq] Update: <file>` so `nb history` stays readable
- [ ] `NbCli` tested with a fake `nb` script recording its argv; `Native` tested on a temp git repo
- [ ] `tasq store sync` runs `nb sync` (NbCli) or `git pull --rebase && git push` (Native) when a remote exists
- [ ] Failure of bookkeeping after a successful file write is reported as a warning with the manual fix (`nb index reconcile`), never as data loss

### T-204: Store capability reporting
**Description:** `describe()` returns location, id scheme (`stable`/`positional`) and whether ids can change on reconcile, so the CLI can warn users.

**Acceptance Criteria:**
- [ ] `tasq store info` prints notebook path, task count, id scheme
- [ ] Positional-id stores trigger a one-line warning on `tasq doctor`

### T-205: `tasq doctor` and `tasq config show`
**Description:** Diagnostics command that validates config, store, and optional tools (`glow`, `claude`, `direnv`, `gwm`, `herdr`, `glab`, `gh`).

**Acceptance Criteria:**
- [ ] Prints a check list with OK/WARN/FAIL and remediation hints
- [ ] Exit code 1 when any FAIL
- [ ] `--json` output

### Phase 3 — CLI parity

### T-301: CLI skeleton, output modes and error handling
**Description:** `clap` derive CLI with global flags `--profile`, `--json`, `--no-color`, `--no-pager`; a `Renderer` abstraction for human output; `anyhow`/`thiserror` error mapping to exit codes.

**Acceptance Criteria:**
- [ ] `tasq --help` lists all subcommands with one-line docs; `tasq help <cmd>` shows details and examples
- [ ] Color only when stdout is a TTY and `NO_COLOR` unset; pager (`$TASQ_PAGER`, default `less -RFX`) only on TTY
- [ ] All user errors print `tasq: <message>` to stderr and exit 1; internal errors exit 2
- [ ] Shell completions generated for bash/zsh/fish via `tasq completions <shell>`
- [ ] Integration test harness with `assert_cmd` + `insta` snapshots and a temp notebook fixture

### T-302: `list` (default command)
**Description:** Port `cmd_list`: grouped view, single-status view, tag filter, priority markers, due dates, tag chips.

**Acceptance Criteria:**
- [ ] `tasq` with no args prints groups in workflow order with headers `IN PROGRESS`, `READY`, ... `NO STATUS`
- [ ] `tasq <status>` prints one group; `tasq <tag>` prints the grouped view filtered by tag; `tasq list --status x --tag y --prio A` explicit form also works
- [ ] Row format matches the script: `[id] #A Title (due date) chips`; snapshot test with colors disabled
- [ ] `--json` emits `[{id, title, status, priority, due, tags, ...}]`
- [ ] Empty results print `No open todos.` / `No open todos tagged #x.`

### T-303: `create`
**Description:** Port `cmd_create` with the same flags (`--desc --status --prio --due --project --tag --related --mr --note`).

**Acceptance Criteria:**
- [ ] Creates the file with sections in the script's order; snapshot compares to a fixture produced by the script
- [ ] `--project` must exist and is canonicalized; `--due` parsed to ISO (accepts `today`, `tomorrow`, `YYYY-MM-DD`)
- [ ] `--mr` entries are added through the MR tracking path (T-307) so titles resolve
- [ ] Prints `[id] created: Title (#status #prio)`

### T-304: `set`, `log`, `done`
**Description:** Port status/priority setting with optional note, progress logging, and done with final note.

**Acceptance Criteria:**
- [ ] `tasq set <id> <status|A|B|C> [note]` works with `#A` too; unknown value lists valid statuses and priorities
- [ ] `tasq log <id> <note>` prints `[id] logged: note`
- [ ] `tasq done <id> [note]` logs then marks done and strips the status tag
- [ ] `--json` on each returns the updated task

### T-305: `view`
**Description:** Port `cmd_view`: render the task markdown through `glow` when available and on a TTY, with OSC 8 links (`TASQ_NO_OSC8` fallback to unwrapped URLs); otherwise print raw markdown.

**Acceptance Criteria:**
- [ ] The `linkify_pre`/`linkify_post`/`unwrap_urls` awk logic is reimplemented as pure Rust functions with unit tests ported from representative inputs (markdown links, bare URLs, trailing punctuation, fenced code untouched, GitLab `!123`/`#123` short refs)
- [ ] Falls back to plain markdown when `glow` is missing or stdout is not a TTY
- [ ] `tasq view <id> --raw` prints the file verbatim

### T-306: `project` and `worktree`
**Description:** Show/set the project directory; track worktrees; `--create <branch>` delegates to the configured worktree manager (`gwm` or plain `git worktree add`).

**Acceptance Criteria:**
- [ ] `tasq project <id>` prints the tracked dir or `no project tracked (default: ...)`
- [ ] `tasq worktree <id> <path>` records path and current branch, idempotent
- [ ] `WorktreeManager` trait with `Git` (default) and `Command` impls; `Command` runs the configured `work.worktree_command` template (gwm is one line of config, not a dependency) and both decide `{new}`/`-b` by checking local/remote branches, like the script
- [ ] Tests use a temporary git repo and a fake `gwm` script on PATH

### T-307: `session` and `mr`
**Description:** Track sessions and merge requests. MR title resolution is delegated to the matching `Source` (GitLab via `glab`/API, GitHub via `gh`/API) with manual title fallback.

**Acceptance Criteria:**
- [ ] `tasq session <id> <session-id> [desc]` idempotent; prints resume hint provided by the launcher kind
- [ ] `tasq mr <id> <url> [title]`: URL host matched against configured sources; title resolved; error with hint when it cannot be resolved
- [ ] MR entries appear under `## Related` → `### Merge requests`, created if missing, exactly as the script does

### T-308: `apply` (JSON edit surface)
**Description:** `tasq apply < task.json` updates a task from JSON (the output shape of `--json`). This is the stdin side of the out-of-process plugin surface.

**Acceptance Criteria:**
- [ ] JSON schema versioned (`"schema": 1`); documented in `docs/json.md`
- [ ] Validation errors name the field
- [ ] Round trip test: `tasq view --json | tasq apply` is a no-op on the file

### Phase 4 — Launchers: `next` and `pick`

### T-401: Launch context resolution
**Description:** Compute where a session starts: first existing tracked worktree → tracked project → default project; detect gone worktrees and offer to recreate them via the worktree manager (only when interactive).

**Acceptance Criteria:**
- [ ] `LaunchContext { task, workdir, in_worktree, missing_worktree: Option<(path, branch)>, env }`
- [ ] Pure resolver tested with a fake filesystem view
- [ ] Prompt text for recreation is identical in meaning to the script; non-TTY never prompts

### T-402: Environment strategy
**Description:** `EnvStrategy` with `Inherit` and `Direnv` (wrap with `direnv exec <dir>` when `.envrc` is allowed; warn when not allowed).

**Acceptance Criteria:**
- [ ] `direnv status` parsing covered by test with a fake `direnv`
- [ ] Warning text points to `direnv allow <dir>`

### T-403: Shell and tmux launchers
**Description:** `ShellLauncher` execs `$SHELL` in the workdir with `TASQ_TASK_ID` exported; `TmuxLauncher` opens a new window there.

**Acceptance Criteria:**
- [ ] Shell launcher replaces the process (`exec`) on Unix
- [ ] Tmux launcher no-ops with a clear error outside tmux
- [ ] Fake launcher used in CLI tests records the `LaunchContext`

### T-404: Claude Code launcher
**Description:** Builds the task prompt (task file, workdir, worktrees, sessions, tool instructions referring to the **new** commands) from a template and execs `claude` with the env strategy. Marks the task in-progress first.

**Acceptance Criteria:**
- [ ] Prompt template lives in `crates/launch/templates/claude.md` and is overridable via `[launch.claude] prompt_file`
- [ ] Template snapshot test; the template mentions `tasq log/set/project/worktree/session/mr/done` and the plugin's `/tasq:wrapup`
- [ ] Sets `TASQ_TASK_ID` and `TASQ_PROFILE` in the session env
- [ ] `tasq next` picks via `query::next`; `tasq pick <id>` targets a task; both accept `--launcher <name>` and `--dry-run` (prints context and prompt, launches nothing)

### T-405: herdr launcher
**Description:** Port the herdr branch: find or create the workspace holding the workdir, create a tab, start the Claude agent, paste the prompt, focus; fall back to the current pane on failure.

**Acceptance Criteria:**
- [ ] herdr JSON parsed with `serde_json` (no grep on JSON)
- [ ] Short workspace label computed like `short_label` with unit tests
- [ ] Only compiled with feature `herdr` (default on, so the author's setup works)
- [ ] Tested with a fake `herdr` script that records the calls

### Phase 5 — Sources and sync

### T-501: Source trait, SourceItem model and reconciliation engine
**Description:** Define `Source`, `SourceItem`, `Origin`; implement `reconcile(existing_tasks, items, policy) -> Vec<Change>` producing create/update/close/flag changes, applied by the core through the `Store`.

**Acceptance Criteria:**
- [ ] Matching by `Origin` (`## Source` section), falling back to a URL present in `## Related` for legacy tasks
- [ ] Policies: `create_new`, `close_when_done` (log a note and mark done when merged/closed), `flag_only` (set a tag like `#review-request`)
- [ ] Reconcile is pure and fully unit tested with table-driven cases (new item, known item unchanged, known item closed, item disappeared)
- [ ] `cargo mutants -f crates/core/src/reconcile` reports no missed mutants
- [ ] Changes are summarized before applying; `tasq sync --dry-run` prints them

### T-502: `sync` command
**Description:** `tasq sync [--source name] [--dry-run] [<id>...]`: runs `fetch` for a sweep or `check` for the given ids, reconciles, applies, prints a summary.

**Acceptance Criteria:**
- [ ] Runs configured sources in order; one failing source reports and does not abort the others
- [ ] `<id>...` limits to re-checking those tasks' origins
- [ ] Logs a progress note on every task it changes (`sync(gitlab): MR merged`)
- [ ] `--json` lists applied changes

### T-503: Shared forge client (GitLab and GitHub)
**Description:** A `forge` module with a `Forge` trait (`current_user`, `review_requests`, `assigned_work_items`, `merge_request(url)`, `work_item(url)`) and two REST clients, `GitLab` and `GitHub`. Sources are thin mappers on top of a forge; title resolution for T-307 also goes through it.

**Acceptance Criteria:**
- [ ] Auth via `token_cmd` (`glab auth token` / `gh auth token`) or `GITLAB_TOKEN` / `GITHUB_TOKEN`; tokens never logged
- [ ] URL → (forge, project, iid) parsing with unit tests for MR/PR, issue and work-item URLs on custom hosts
- [ ] Pagination, 429 and 5xx retry with backoff; tested with `wiremock`
- [ ] `[forge.<name>]` config blocks are referenced by sources via `forge = "<name>"`

### T-504: Review-request sources (`gitlab-review-requests`, `github-review-requests`)
**Description:** One `Source` per forge for MRs/PRs where I am a requested reviewer. `fetch` lists them; `check` resolves the state of tracked MRs (approved by me, merged, closed) so tasks can be closed automatically.

**Acceptance Criteria:**
- [ ] `SourceItem` carries `Origin { source, url }`, title `Review MR !123: <title>` (configurable template), configured tags/status
- [ ] `check` returns `Done` when the MR is merged/closed or I have approved it, `Open` otherwise
- [ ] Both sources share tests via a generic test suite parameterized by forge fixtures
- [ ] Reconcile policy default: `create_new` + `close_when_done`

### T-504b: Work-item sources (`gitlab-work-items`, `github-work-items`)
**Description:** One `Source` per forge for issues / work items assigned to me, created as tasks by default. `check` resolves closed/reassigned state.

**Acceptance Criteria:**
- [ ] Items map to tasks with the issue URL in `## Related` and `## Source`, title `#123: <title>` (configurable), configured tags/status
- [ ] Reassignment away from me or closure yields `Done` on `check`
- [ ] Filters configurable: labels include/exclude, projects/groups allow-list
- [ ] Own open MRs/PRs are intentionally **not** a source in v1: they are tracked explicitly via `tasq mr` and re-checked through the review-request forge client (see Open Questions)

### T-505: LLM bridge source (Slack, Gmail, anything unstructured)
**Description:** A generic source that runs a configured command (e.g. `claude -p --output-format json` with a prompt file) and expects a JSON array of `SourceItem`s back. This is how the adapter itself decides to use an LLM.

**Acceptance Criteria:**
- [ ] Contract documented in `docs/sources.md` with the JSON schema and an example prompt for Slack/Gmail triage
- [ ] Invalid JSON yields a clear error that includes the first 200 chars of output
- [ ] Items without a stable external id are deduplicated by URL, else by normalized title
- [ ] Tested with a fake command script
- [ ] Example prompt files shipped in `examples/sources/`

### Phase 6 — Reports

### T-601: `summary` with pluggable summarizer
**Description:** Port `cmd_summary`: collect progress notes for a day (default last working day), output raw or pass through a `Summarizer`.

**Acceptance Criteria:**
- [ ] `Report` trait and `summary` impl; raw output identical in structure to `summary_raw`
- [ ] `Summarizer` trait with `Raw` and `Command` (runs configured command with the prompt template + notes on stdin)
- [ ] Date parsing accepts `today`, `yesterday`, ISO dates, weekday names; `last_working_day` skips weekends (unit tested with `FixedClock`)
- [ ] Rendered through the same markdown renderer as `view`
- [ ] `--raw` never calls the summarizer

### T-602: Date range resolver
**Description:** Port `resolve_range` as a reusable core function for future reports and plugins (e.g. the external time-logs plugin).

**Acceptance Criteria:**
- [ ] Supports `this week`, `last week`, `this month`, `last month`, `last N days`, single dates, date pairs
- [ ] Weeks are Mon–Fri; ranges never extend past today; table-driven tests with a fixed clock
- [ ] Exposed as `tasq dates <spec> --json` for scripts

### Phase 7 — Claude Code plugin

### T-701: `plugins/claude` plugin scaffold
**Description:** A Claude Code plugin (`.claude-plugin/plugin.json`) that ships the skills the agent needs when working on a `tasq` task. Namespaced so it never collides with the existing `/wrapup` and `/update-tasks`.

**Acceptance Criteria:**
- [ ] Plugin installs via `claude plugin install` from the repo path; skills appear as `/tasq:wrapup`, `/tasq:sync`
- [ ] `tasq:wrapup` reads `TASQ_TASK_ID`, appends a progress summary with `tasq log`, sets status with `tasq set`
- [ ] `tasq:sync` guides an LLM-bridge sweep and ends with `tasq sync`
- [ ] Existing `~/.claude` skills and `original/tasks` are untouched (checked in review)
- [ ] Optional statusline snippet documented showing `[task id] title`

### Phase 8 — TUI

### T-801: TUI foundation
**Description:** ratatui + crossterm app in `tasq-tui`, launched by `tasq ui`, depending only on `tasq-core` (store/launchers injected). Elm-style architecture: `Model`, `Msg`, `update`, `view`.

**Acceptance Criteria:**
- [ ] Grouped task list with the same ordering as the CLI; `j/k` move, `/` filter, `g/G`, `q`
- [ ] Right pane shows the selected task rendered (title, status, due, tags, last progress notes)
- [ ] `update()` is pure and unit tested with `Msg` sequences; rendering tested with `ratatui::backend::TestBackend` snapshots
- [ ] Verify manually in a terminal (record a short asciinema/VHS gif for the README)

### T-802: TUI editing actions
**Description:** Change status (`s`), priority (`p`), log a note (`l`), mark done (`d`), open in `$EDITOR` (`e`), launch session (`enter`), sync (`S`).

**Acceptance Criteria:**
- [ ] Each action goes through the same core functions the CLI uses (no duplicated logic)
- [ ] Status and priority pickers show the configured workflow
- [ ] Note input supports multi-line paste; cancel with `Esc`
- [ ] Launching suspends the TUI, runs the launcher, and restores the terminal on return
- [ ] Help overlay on `?`

### T-803: TUI theming and config
**Description:** Colors per status configurable under `[ui.colors]`; respects `NO_COLOR`; works on 80x24.

**Acceptance Criteria:**
- [ ] Default theme matches the CLI colors (blue/green/yellow/red/magenta/dim)
- [ ] Layout adapts: single pane below 100 columns
- [ ] Snapshot tests for both layouts

### Phase 9 — Plugin mechanism and release

### T-901: Decide and implement the plugin mechanism
**Description:** With the CLI and TUI in place, evaluate and decide (ADR 0006) between: (A) external executables `tasq-<name>` discovered on PATH, talking JSON via `--json`/`apply` and receiving hooks (`post-create`, `post-done`, `pre-launch`); (B) in-process Rust adapters behind cargo features; (C) WASM via extism. Recommendation to validate at that point: **A for user plugins, B for built-in adapters**, with C revisited only if sandboxing becomes a requirement. Implement the chosen mechanism.

**Acceptance Criteria:**
- [ ] ADR 0006 moves to "Accepted" with the evaluation
- [ ] `tasq <unknown>` dispatches to `tasq-<unknown>` on PATH (if A) with `TASQ_PROFILE`, `TASQ_CONFIG` env set
- [ ] Hook points documented in `docs/plugins.md` with a worked example (`tasq-tlogs`, the author's private time-log plugin, as the reference external plugin)
- [ ] `tasq plugins list` shows discovered plugins

### T-902: Documentation and examples
**Description:** README (install, quick start, concepts, config reference, screenshots/gif), `docs/file-format.md`, `docs/sources.md`, `docs/plugins.md`, `docs/json.md`, `CONTRIBUTING.md`, and a `examples/` directory with configs for a plain-markdown user and for the author's setup.

**Acceptance Criteria:**
- [ ] A new user can go from `cargo install tasq` to listing tasks in an existing nb notebook by following only the README
- [ ] Every config key appears in the config reference with its default
- [ ] `cargo doc --no-deps` has no warnings; public items in `tasq-core` documented

### T-903: Release pipeline
**Description:** `cargo-dist` (or `release-plz`) producing Linux/macOS binaries, a Homebrew tap formula, and crates.io publication of `tasq-core` and `tasq`.

**Acceptance Criteria:**
- [ ] Tagging `v0.1.0` builds and attaches binaries to a GitHub release
- [ ] `cargo install tasq` works from crates.io
- [ ] `CHANGELOG.md` generated from conventional commits

### T-904: Migration guide and side-by-side period
**Description:** Document how to run `tasq` alongside the old script on the same notebook (they share files, ids and format), what `tasq` writes that the script ignores (`## Source`, comments), and the switch-over checklist (point herdr/Claude at `tasq`, install the plugin, retire old skills).

**Acceptance Criteria:**
- [ ] `docs/migration.md` with the checklist
- [ ] Verified on the author's real notebook: run `tasks` and `tasq` alternately for one week with no corrupted file (manual acceptance)

## 6. Functional Requirements

- FR-1: The tool must read and write existing nb todo files without altering their layout except for the edited section.
- FR-2: Task ids shown by the tool must equal nb's ids for the same notebook.
- FR-3: Status is one of the configured workflow statuses or absent; priority is A/B/C with default B.
- FR-4: Every command must support `--json` with a versioned schema; human output must be color-free when not on a TTY.
- FR-5: Configuration must be resolvable from a global file, a per-project file discovered from cwd, environment variables and flags, in that precedence order (later wins).
- FR-6: `sync` must call every configured source, reconcile deterministically, and show a dry run on request.
- FR-7: A source adapter must be able to be deterministic, LLM-driven or hybrid without the core knowing which.
- FR-8: `next`/`pick` must resolve the working directory as: first existing tracked worktree, else tracked project, else default project.
- FR-9: Launchers are selectable per invocation and per config; a `--dry-run` must print what would happen.
- FR-10: The TUI must perform every edit through the same core functions as the CLI.
- FR-11: Nothing in this project modifies `original/tasks`, the user's nb notebooks' content beyond normal task edits, or the existing Claude skills.
- FR-12: All adapters that talk to external processes or networks must be testable with fakes/mocks and have no network access in `cargo test`.

## 7. Non-Goals

- No Freshdesk adapter in v1 (the user did not select it; it fits the Source trait later).
- No time-log (`tlogs`) feature in the main repo; it becomes the reference external plugin.
- No sync server, mobile app, or multi-user features. Remote sync of the notebook stays nb's/git's job.
- No rewriting of nb itself or replacing nb's git sync; the nb notebook stays the source of truth in v1.
- No SQLite or other stores in v1 (the trait exists; implementations come later).
- No Windows support in v1 (`exec`, direnv, tmux semantics); should compile but is untested.
- No backwards-compatible `tasks` command names: this is a clean slate under a new binary name.

## 8. Design Considerations

- **Human output parity first**: the grouped list, chips and colors are kept because they work; the renderer is a separate module so the TUI and CLI share color semantics.
- **OSC 8 links** and `glow` rendering stay as the markdown rendering path; consider replacing `glow` with an in-process renderer (`termimad`) once the TUI exists, to drop the external dependency.
- **Prompts are data**: every LLM prompt (launcher, summarizer, LLM-bridge) is a template file users can override, not a string in Rust.
- **Errors teach**: every failure message says what was expected and the config key or command to fix it.

## 9. Technical Considerations

- Toolchain present: cargo/rustc 1.98.1, nb 7.25.4, glow, claude, herdr, gwm, glab, direnv.
- Test tooling: `cargo-mutants` (mutation testing), `cargo-llvm-cov` (coverage), `insta` (snapshots), `proptest` (round trips), `wiremock` (HTTP), `assert_cmd` (CLI). Mutation testing is why the plan keeps logic in pure functions with injected `Clock`, filesystem views and fake processes: mutants only reveal weak tests when the code under test is cheap to run thousands of times.
- Key crates: `clap` (derive), `serde`/`toml`/`serde_json`, `thiserror`/`anyhow`, `chrono`, `ratatui`/`crossterm`, `reqwest` (blocking, rustls) or `ureq`, `wiremock`, `assert_cmd`, `insta`, `proptest`, `tempfile`, `tracing`.
- Keep `tasq-core` free of `reqwest`, `ratatui` and process spawning; adapters own those dependencies.
- nb specifics to verify early (T-201/T-203/T-206): `.index` is one filename per line, id = line number; `nb index add` appends; `nb index reconcile` fixes drift; `nb git checkpoint` commits and honors `auto_sync`; `nb todo do` edits only the title line. nb says using `index` manually "will probably corrupt the index": we only ever call `add` and `verify`, never `rebuild`.
- nb is AGPLv3 and is invoked as a separate program, never linked or vendored, so our GPL-3.0-or-later license is independent of it.
- Positional ids (nb) can shift after deletions; `Store::describe` exposes this so the UI can warn, and future stores can offer stable ids.
- Performance target: `tasq` list under 50 ms on a 500-task notebook; TUI redraw under 16 ms.

## 10. Success Metrics

- The author uses `tasq` daily instead of `tasks` within the side-by-side week, with zero file corruption.
- `cargo test --workspace` runs offline in under 60 s with >80% line coverage on `tasq-core` (cargo-llvm-cov).
- `cargo mutants` reports zero missed mutants in the pure core modules (format, query, reconcile, config, dates) and ≤5% missed across `tasq-core`; the full run finishes in under 20 minutes on CI.
- A GitHub user with no nb installation can point `tasq` at a directory of markdown files (via the nb-format store with a plain index) and start in under 10 minutes.
- Adding a new source or launcher requires touching only a new module plus config, verified by adding GitHub after GitLab (T-504) without changing core.

## 11. Naming

Checked on crates.io on 2026-10-04: `tasq`, `taskdeck`, `workdesk` are free; `docket`, `tsk`, `tally`, `slate` are taken.

- **tasq** (recommended): short, typeable, free on crates.io, reads as "task" + queue; no popular Rust repo of that name (only Go/Python job queues).
- **taskdeck**: evokes the TUI "deck" view; longer to type (alias `td` would collide with common aliases).
- **workdesk**: emphasizes the work-context idea (projects, worktrees, sessions); less obviously about tasks.

Rename is a find-and-replace on crate names before T-903; nothing in the plan depends on the final name.

## 12. Open Questions

1. For non-nb users, is a "plain directory of markdown files + our own index" (the `Native` bookkeeper) enough for v1, or is that already the SQLite store?
2. Should the standup `summary` default summarizer be `raw` (no LLM dependency for new users) with `llm` opt-in? Plan assumes yes.
3. Should there be a third forge source for *my own* open MRs/PRs (`gitlab-merge-requests`), or is explicit tracking via `tasq mr` plus automatic `check` enough? Plan assumes the latter for v1.
4. Work items: should closed-by-someone-else issues mark the task done, or only flag it for review? Plan default: mark done with a progress note; configurable per source.

## 13. Decisions log (from planning conversation, 2026-10-04)

- Language: Rust. Store: nb-compatible markdown first, `Store` trait so other stores can follow.
- Plugin mechanism: deferred to T-901; architecture keeps both in-process and out-of-process open.
- Sources decide their own strategy (deterministic / LLM / hybrid); core only reconciles.
- Work context (project, worktrees, sessions) is a core concept; launchers are pluggable.
- Delivery: core library → CLI parity → sources/launchers → TUI → plugins.
- Clean slate: new binary name, new Claude plugin; the old script and skills are never modified.
- `summary` in core with pluggable summarizer; time logs as an external plugin.
- License: GPL-3.0-or-later (nb is AGPLv3; CLI has no network-service clause need).
- nb relationship: hybrid — native reads and file writes, nb for index/git bookkeeping when installed, native fallback otherwise.
- Forge sources split per concern: `<forge>-review-requests` and `<forge>-work-items`; work items create tasks by default.
- (2026-10-04, during Phase 3) No gwm adapter in tasq: worktree provisioning is a separate, per-repo concern. `work.worktree_manager` is `git` by default or `command` with a user template; gwm users set `worktree_command = "gwm create {new} {branch} --no-tmux -s"`.
