# Progress: tasq rewrite

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

**Phase 9 (plugin mechanism and release) complete (2026-10-04), pending two manual acceptances.**
ADR 0006 is Accepted: external executables for user plugins, in-process Rust for the adapters
in this repository, no WASM. `tasq <name> [args]` execs `tasq-<name>` from `PATH` (built-ins
win; a plugin wins over the bare `tasq <word>` filter) with `TASQ_BIN`, `TASQ_PROFILE`,
`TASQ_CONFIG` and `TASQ_SET` forwarded; `[hooks]` (`post-create`, `post-done`, `pre-launch`)
run command lines with a JSON document on stdin; `tasq plugins list` shows both. The
reference plugin `examples/plugins/tasq-tlogs` runs end to end in a CLI test. Docs: README
rewritten for a new user, `CONTRIBUTING.md`, `docs/plugins.md`, `docs/release.md`,
`docs/migration.md`, `examples/config/`, config reference audited against the structs.
Release: hand-written `.github/workflows/release.yml` (tag `v*`: four native builds, GitHub
release with git-cliff notes, crates.io publish of all six crates in dependency order,
optional Homebrew tap from `homebrew/tasq.rb.template`), `cliff.toml`, crate metadata for
publishing. 701 workspace tests; config module 107 mutants, 0 missed; `cargo doc` clean.
**Not verifiable here:** the release pipeline has never run (no GitHub repository yet, `OWNER`
placeholders), `cargo install tasq` is not possible until the crates are published, and
T-904's one-week side-by-side run on the real notebook is the author's manual acceptance
(procedure in `docs/migration.md`). Phase 9 housekeeping 2026-10-05: `cargo deny check` green
locally after allowing `CDLA-Permissive-2.0` (`webpki-roots`, the CA bundle behind `ureq`;
it was the only rejection, so CI's `deny` job would have failed on first run); full mutants
run over the examined crates, 1454 mutants in 8 min, 1212 caught, 242 unviable, 0 missed,
0 timeouts; PLAN section 9 tooling corrected (`ureq`, no `wiremock`/`reqwest`). Follow-up
`tasq list --all` / `--done` done the same day. Commits ae29142, b5a649d, ef2bcce,
b272893, f2f0fad, 50cb267 and the manifest follow-up. Follow-up 2026-10-04: the TUI's `d` key
now fires `post-done` through a fourth `Host` method (ADR 0010); commit sha below under
"Plugin and release decisions".

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-901 | Plugin mechanism | done | ADR 0006 Accepted; `crates/cli/src/plugins.rs` (dispatch, discovery, hooks), `commands/plugins.rs`; core `HooksConfig` + `TASQ_SET`; `docs/plugins.md`; `examples/plugins/{tasq-tlogs,hooks/log-event.sh}`; 6 integration + 6 unit tests |
| T-902 | Documentation and examples | done | README, `CONTRIBUTING.md`, `docs/architecture.md` (crates, modules, dependency and sequence diagrams; added 2026-10-05), `docs/config.md` (every key with default), `docs/json.md`, `docs/testing.md` (wiremock claim removed), `examples/config/{plain-markdown,author}.toml`, `examples/README.md`; gif recorded 2026-10-05 (`docs/demo/`, VHS in docker, commit fddd704) |
| T-903 | Release pipeline | done, unrun | `.github/workflows/release.yml`, `cliff.toml`, `homebrew/tasq.rb.template`, `docs/release.md`; manifests carry `version` on path deps, `homepage`/`keywords`/`categories`; needs a GitHub repo and `CARGO_REGISTRY_TOKEN` to run |
| T-904 | Migration guide | docs done, acceptance pending | `docs/migration.md` (command and env mapping from `original/tasks`, switch-over checklist, daily verification); the one-week run is manual |

### Phase 8 status

**Phase 8 (TUI) complete (2026-10-04).** `tasq ui` is a ratatui UI in `crates/tui`
(`tasq-tui`, depends on `tasq-core` and ratatui only): the grouped list with the CLI's ordering
and the selected task's detail beside it (one pane below 100 columns, `Tab` switches), `/`
filter, `s`/`p` pickers from the configured workflow, `l`/`d` note prompts, `e` editor, `Enter`
session, `S` sync, `?` help, `[ui.colors]` and `NO_COLOR`. Elm shape: pure `update`, `view` on
a `Frame`, `dispatch` running `Cmd`s against the injected `Store`, `Clock` and a `Host`. The
edit logic moved into `tasq_core::edit` so the CLI and the TUI share it (ADR 0009);
`tasq_core::theme` holds the colour semantics; `tasq_core::store::MemoryStore` is the test
double and `Store::file_of` tells a UI which file to open. 686 workspace tests. Mutants:
core `theme` + `edit` + `store` and store-nb `store.rs` + tui `model`/`update`/`keys`/`msg`/
`runtime`: 276 tested, 0 missed after fixes (first pass 5 missed: two tests added, two code
shapes changed, one terminal `Drop` skipped); tui alone 162 tested, 134 caught, 28 unviable.
Verified in a pseudo-terminal (`script` + `stty`): draws, moves, opens help and the picker,
quits and restores the screen. The README gif was recorded on 2026-10-05 with VHS in docker
(`docs/demo/`, `scripts/demo-gif`). Not done: `cargo deny` locally (not installed; CI runs it; every new dependency is
MIT/Apache/Zlib). Next: Phase 9 (T-901 plugin mechanism, T-902 docs, T-903 release, T-904
migration).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-801 | TUI foundation | done | `crates/tui/src/{model,msg,keys,update,view,runtime}.rs`; `tasq ui` in `crates/cli/src/commands/ui.rs`; 18 `TestBackend` snapshots in `crates/tui/tests/render.rs`; gif recorded 2026-10-05 (`docs/demo/demo.tape`) |
| T-802 | TUI editing actions | done | `s p l d` through `tasq_core::edit`; `c` creates a task (title only, `workflow.default_status`) through `Store::create` and fires `post-create` through `Host::after_create` (ADR 0011); `Ctrl+Enter`/`Shift+Enter` open the session in a new window, focused or not (`tasq pick --detached [--no-focus]`, ADR 0012); `e`/`Enter`/`S` through the `Host` trait, run as `$EDITOR`, `tasq pick`, `tasq sync` child processes with the terminal released; paste collapses to one line; `?` help overlay |
| T-803 | Theming and config | done | `tasq_core::theme::{Color, Theme}` shared with the CLI; `NO_COLOR`/`--color never` monochrome; two-pane from 100 columns, one pane below; both layouts snapshotted; `[ui.keys]` rebinds every key but `Ctrl+C` (ADR 0013); follow-up 2026-10-06: named colour themes, `theme::{Role, Preset}`, `ui.theme.preset` (dark/light/solarized/gruvbox/mono) + `[ui.theme.colors]` under `[ui.colors]` (ADR 0018, commit 51ed314) |

### Phase 7 status

**Phase 7 (Claude Code plugin) complete (2026-10-04).** `plugins/claude` is a Claude Code plugin
named `tasq` (so its skills are `/tasq:wrapup` and `/tasq:sync`) plus a status-line snippet;
the repository root is a one-plugin marketplace (`.claude-plugin/marketplace.json`), so
`claude plugin marketplace add <repo>` + `claude plugin install tasq@tasq` installs it and
`claude --plugin-dir plugins/claude` loads it for one session. `claude plugin validate --strict`
passes for both manifests and the skills directory; `crates/cli/tests/plugin.rs` pins the layout
and checks that the launch prompt names only shipped skills. The existing `~/.claude` skills and
`original/tasks` were not touched (nor read: they sit outside the working directory, so the
skills were written from the plan, the script and the launcher prompt). Next: Phase 8 (TUI,
T-801 to T-803).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-701 | `plugins/claude` plugin scaffold | done | `.claude-plugin/plugin.json` (name `tasq`), `skills/{wrapup,sync}/SKILL.md` with `allowed-tools: Bash(tasq *)`, `statusline/tasq-statusline.sh`, `plugins/claude/README.md`; root `.claude-plugin/marketplace.json` |

### Phase 6 status

**Phase 6 (reports) complete (2026-10-04).** `tasq summary [DAY] [--raw]` collects the day's
progress notes (open and done tasks, id order) and distils them through `report.summary.command`
(default `claude -p`; the prompt template with the notes on stdin) or prints them raw;
`tasq dates [SPEC]` resolves `this|last week`, `this|last month`, `last N days`, days and day
pairs to `FROM TO` (`--json` adds `days` and `working_days`). Mutants: `core::dates` +
`core::report` + `launch::summarizer` 105 tested, 88 caught, 17 unviable, 0 missed
(`process::run_with_input` skipped). Next: Phase 7 (Claude Code plugin, T-701).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-601 | `summary` with pluggable summarizer | done | `tasq_core::report::{DaySummary, TaskNotes, Summarizer, RawSummarizer}`; `tasq_launch::summarizer::CommandSummarizer` + `templates/summary.md`; `report.summary.prompt_file`, `TASQ_SUMMARY_COMMAND`, `TASQ_SUMMARY_PROMPT_FILE`; rendered through `view::show_markdown` |
| T-602 | Date range resolver | done | `tasq_core::dates::{resolve_range, DateRange, parse_past_day, last_working_day}`; `tasq dates <spec> [--json]` |

### Phase 5 status

**Phase 5 (sources and sync) complete (2026-10-04).** `tasq sync` runs the configured sources
(GitLab/GitHub review requests and work items, the LLM bridge), reconciles and applies; `tasq mr`
and `create --mr` resolve titles through the configured forge. The script's `update` is now
`sync` with deterministic forge sources plus an LLM bridge for Slack/Gmail; `update-support`
(Freshdesk) is out of scope for v1 (plan Non-Goals). Mutants: core `source` 32/0 missed;
sources crate 240 tested, 0 missed (`UreqTransport::get`, `auth::run`, `run_command` skipped).
Next: Phase 6 (reports, T-601/T-602).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-501 | Source trait, items, reconcile | done | `tasq_core::source`: `reconcile` + `apply`, matched by origin or legacy URL; `Policy { create_new, close_when_done, flag }` |
| T-502 | `sync` command | done | sweep + per-item `check` of tracked tasks the sweep dropped; `<id>...` re-check; `--source`, `--dry-run`, `--json`; exit 1 if any source failed; `--interactive` opens the `/tasq:sync` Claude session (2026-10-05, replaces `tasks update`) |
| T-503 | Forge client | done | `tasq_sources::{http, auth, url, forge, gitlab, github}`; injectable `Transport`, retry/backoff, `Link` pagination; `forge.<name>.url` override |
| T-504 | Review-request sources | done | one `ReviewRequests` over any `Forge`; check: merged/closed/approved by you/gone |
| T-504b | Work-item sources | done | `WorkItems` with label/project filters; check: closed/reassigned/gone |
| T-505 | LLM bridge | done | command + prompt on stdin, `TASQ_SYNC_KNOWN`, claude result envelope, dedupe; `docs/sources.md`, `examples/sources/` |

### Phase 4 status

**Phase 4 (launchers) complete (2026-10-04).** `tasq next` and `tasq pick` open sessions through
the `claude`, `shell`, `tmux` and `herdr` launchers (`auto` picks herdr inside herdr). 511
workspace tests at the time. Mutants: core `launch` 12/0 missed; launch crate 192 tested, 0 missed
(`process::run`, `process::exec` and the feature-off `herdr` stub skipped).

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-401 | Launch context resolution | done | `tasq_core::launch::resolve_workdir` (pure, fs as a closure); recreate prompt only on a TTY |
| T-402 | Environment strategy | done | `tasq_launch::env`: `direnv status` parsed (`Found RC allowed`), `direnv exec` wrap, `direnv allow` warning |
| T-403 | Shell and tmux launchers | done | `exec $SHELL`; `tmux new-window -c -n -e`, error outside tmux |
| T-404 | Claude launcher, `next`/`pick` | done | template `crates/launch/templates/claude.md` (+ `prompt_file`), `--launcher`, `--dry-run` (text or JSON) |
| T-405 | herdr launcher | done | feature `herdr` (default on); serde_json over herdr's output; `short_label` in core; fake herdr tests |

### Phase 3 status

**Phase 3 (CLI parity) complete (2026-10-04).** 468 workspace tests at the time (cli 49 unit + 66
integration). Mutants: core `work` 9/0 missed, `clock` 22/0, `dates` 8/0, model 0 missed.

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-301 | CLI skeleton, output modes, errors | done | 27 unit + 37 integration tests (`crates/cli/tests/cli.rs`, insta snapshots) |
| T-302 | `list` (default command) | done | grouped/single-status/tag/priority views, `--json` |
| T-205 | `doctor` / `config show` commands | done | config failures are reported as a FAIL check, not a crash |
| T-204 | `store info` | done | plus `store sync` from T-206 |
| T-303 | `create` | done | `--due` words via `tasq_core::dates::parse_day` (mutants 8 tested, 0 missed); `--mr` falls back to a `group/project!123` label until T-307 |
| T-304 | `set`, `log`, `done` | done | whole-task `Store::update`; `TASQ_NOW` fixes timestamps in tests |
| T-305 | `view` | done | `linkify_pre/post`, `unwrap_urls`, `shortref` ported as pure functions with unit tests; `--raw`; glow only on a TTY |
| T-306 | `project`, `worktree` | done | `tasq_core::work` (trait + pure helpers), `tasq_launch::worktree` (`GitManager` default, `CommandManager` for gwm and friends; real git + fake tool tests) |
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

### Plugin and release decisions (T-901 to T-904)

- **`tasq-tlogs` with parity to `tasks tlogs` (follow-up 2026-10-06, task 8).** The
  reference plugin now does what the script did: `tasq dates --json` for the range, then
  `exec claude "/time-logs <from> <to>"` from `work.default_project` (read from
  `tasq config show --json`, `~` expanded), wrapped in `direnv exec <dir>` when `launch.env`
  is `direnv`, direnv is on `PATH` and `direnv status` says `Found RC allowed true|0`
  (a refused `.envrc` is a stderr warning and a bare `claude`, as in the script). It exports
  `TASKS_NB_NOTEBOOK=<store.notebook>` unless already set, because the personal
  `/time-logs` skill still reads the notebook through that variable. The even split from
  progress notes moved behind `--propose` (`--json` as before); `--dry-run` prints
  `cd <dir> && TASKS_NB_NOTEBOOK=... <cmd>` with `printf %q`, which is what the CLI test
  asserts (no direnv on the test `PATH`, `--set work.default_project=<home>`), plus exit 1
  with "work.default_project is not set" when there is no directory. The skill itself stays
  private (`~/code/SF/.claude/skills/time-logs`); `docs/plugins.md` says so and
  `docs/migration.md` maps `TASKS_DEFAULT_WORKTREE` to `work.default_project` for it.
- **Sessions in a new window from the TUI (follow-up 2026-10-05, ADR 0012).** `Enter` stays
  "here"; `Ctrl+Enter` / `Shift+Enter` are `Cmd::Launch(id, LaunchTarget::Detached { focus })`
  and `Host::launch(id, target)`; the CLI host runs `tasq pick <id> --detached [--no-focus]`
  with `Command::output()` so the UI keeps the screen and shows the child's last line. New
  config: `launch.detached` (`auto` = herdr, else tmux, else an error before any write) and
  `launch.herdr.placement` (`auto` | `workspace` | `tab`); `LaunchContext.focus`; tmux gets
  `-d`, herdr skips `agent/tab/workspace focus`. The runtime pushes
  `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES` when `supports_keyboard_enhancement()`
  and pops it on restore (a static `AtomicBool` guards the pop, so a terminal that never got
  the push is not sent `CSI < u`). Probed from a herdr 0.9.3 pane: `CSI ? u` answers
  `CSI ? 0 u`, so herdr speaks the kitty protocol and the chords arrive as such. Mutants on
  `launch/{registry,herdr,tmux}.rs` + `core/launch.rs`: 108 tested, 0 missed. Commit d7b39e3.
  Follow-up 8be3253: the herdr launcher focuses the workspace right after creating the pane,
  before `agent start` waits for Claude to be ready; the switch felt slow when it came last.

- **Relative, coloured due dates (follow-up 2026-10-06, ADR 0019, todo 26).** Due dates
  were dim whether three days past or three weeks away. `dates::Due::of(due, today)`
  (`Overdue(n)`/`Today`/`Tomorrow`/`Later(n)`, a `..=-1` range arm so no `<`/`<=` mutant
  is equivalent) and `dates::due_label` render `overdue 3d`, `due today`, `due tomorrow`,
  `due in 4d`; `ui.due_format = relative | iso | both` (`TASQ_DUE_FORMAT`) picks the row
  form. New roles `overdue` (bold added by the front ends) and `due-soon`, one shade per
  preset (each preset's `blocked`/`waiting` colour, `mono` plain); `Role::of_due` maps a
  `Due` to `overdue`/`due-soon`/`dim`. CLI: `list::Look` bundles theme, style, today and
  format for `row`; a done task keeps its ISO date, dim. TUI: `view::due_span` for list rows
  (`model.due_format`) and the detail pane (always `both`); the TUI lists open tasks only, so
  it has no done case. `--json` unchanged. The CLI test harness now pins `TASQ_NOW` to
  2026-10-06 09:00 for every command and the TUI render fixture pins `Model::today`, so
  snapshots do not move with the calendar. Colouring past days in the calendar picker was
  dropped from scope (the saved row already shows overdue). Mutants: `dates.rs` +
  `theme.rs` 118 tested, 0 missed. Known limit: a long-running `tasq ui` keeps the `today`
  it started with. Commits baf8006, e5e47bf, b460d55, 36bb278.
- **Readline keys and scrolling in the status-bar prompts (follow-up 2026-10-06, step 1 of
  3).** The filter, log, done and create prompts hold a single-line `form::Text` (the edit
  view's editor) instead of a `String`. `keys::prompt` (was `text`) adds `Left`/`Right`,
  `Home`/`End`, `Delete` and `Ctrl+A`/`Ctrl+E`; the edit view's `form` falls through to it.
  `update::edit_line(&mut Text, &Msg)` is the one editing helper for the three handlers
  (paste still through `one_line`); the filter is re-applied after every message.
  `view::prompt(model, width) -> Option<Prompt { label, shown, cursor }>` windows the input
  after the label (the label capped at `width - 1`, so the cursor never leaves the bar) and
  `render_status_bar` puts the real terminal cursor there; the fake `▁` is gone, so five
  prompt snapshots lost it and `long_note_scrolled` (80 columns) was added. `Up`/`Down` stay
  unbound in prompts (history comes later). Mutants on the diff (`--no-config --in-diff`):
  54 tested, 1 missed on the first pass (`- -> +` in the cursor column, hidden by a
  `min(width - 1)` clamp that was always active when scrolled), 0 after capping the label
  instead (56 tested, 48 caught, 8 unviable). Commit f983b82.
  Step 2 of 3: readline's kills, as `Text` methods so the prompts and the edit view's text
  rows both get them (`Form` forwards to the focused row, choice rows ignore them):
  `Ctrl+W` `delete_word` (unix-word-rubout: whitespace before the cursor, then back to the
  previous whitespace, by `char::is_whitespace`, so punctuation is part of the word),
  `Ctrl+U` `kill_to_start`, `Ctrl+K` `kill_to_end`; `Msg::DeleteWord`/`KillToStart`/
  `KillToEnd`. All three stay on the current line: `Ctrl+W` at column 0 and `Ctrl+K` at the
  end of a line do nothing (no join, unlike `Backspace`/`Delete`). The list's `Ctrl+U`
  (page-up) is a keymap binding and never reaches prompt or form keys. Mutants on the diff:
  46 tested, 41 caught, 2 timeouts (`-=` to `/=` in the `delete_word` loops, which never
  terminate), 3 unviable, 0 missed. Commit 1d1a874.
- **Calendar picker for the Due box (follow-up 2026-10-06, ADR 0017, todo 14).** `Enter` on
  the Due box opens `Mode::Calendar { form, calendar }`: the edit view stays underneath, one
  month is drawn centred over it. `calendar::Calendar` is the day under the cursor plus the
  arithmetic: `Up`/`Down` a week, `Left`/`Right` a day, `PageUp`/`PageDown` a month
  (`checked_add_months` clamps 31 January to 28 February), `t` today, `Enter` writes ISO into
  the box (`Text::single`, cursor at the end) and returns to the view, `Esc` returns
  unchanged; every other key is `None` in `keys::calendar`, `Ctrl+C` quits. Opens on the
  box's day when `parse_day` takes it, else on today. The grid is the crate's own
  (`calendar::month_grid`: title, ` Mo Tu ...` header, weeks of `Option<NaiveDate>`), after
  ratatui's `Monthly` widget, which the first cut used and the author rejected the same day:
  its weeks are Sunday-based with no Monday option, and it works on `time::Date`, so chrono
  crossed a crate boundary at every call. Owning the layout dropped the `widget-calendar`
  feature and the `time` dependency and added `ui.week_start` (`WeekStart`, a weekday name,
  `monday` by default, `TASQ_WEEK_START`; `Model::with_week_start` from the CLI). Styles in
  `view::day_style`: cursor reversed bold, today bold, weekends dim; cyan border and ` Due `
  title like the focused box; the terminal cursor is hidden while the picker is open
  (`render_form` now returns the cursor position and `view` sets it). `form_hints` became
  `key_bar(hints, width)` with `CALENDAR_HINTS` (64 columns with labels). chrono fact:
  `NaiveDate::iter_days` never yields `NaiveDate::MAX` (it stops where `succ_opt` fails), so
  the grid walks with `succ_opt` itself. Snapshots `calendar_picker` (Monday),
  `calendar_picker_sunday`, `calendar_picker_next_month`, `calendar_picker_narrow` plus
  buffer-style assertions (border, title, header, the three day styles, the gutters,
  `--color never`); grid unit tests for four-, five- and six-row months, three week starts
  and both ends of chrono's range. Mutants on the diff (tui `calendar/update/keys/view/msg/
  model.rs`, core `config/{mod,load}.rs`): first pass with the widget 80 tested, 2 missed
  (the calendar's status-bar message guard, untested; `REVERSED | BOLD` equivalent under
  `^`), 0 after a `Msg::Failed`-while-open assertion and two `add_modifier` calls; with the
  owned grid and the config key 103 tested, 0 missed (10 unviable). Commits c90ffcd (core),
  8054d8a (tui, docs). Docs: `docs/config.md` (key, env, defaults, TUI
  section), README. Pre-existing and unrelated: `cargo doc` warns about a redundant link
  target in `view.rs:5`; `render_with_glow_runs_the_program_and_reports_failures` (cli)
  failed once in a full workspace run and passed alone.
- **Boxed edit view on wide terminals (follow-up 2026-10-06, ADR 0016 addendum).** After
  reading the ratatui `user_input` example, `tui-textarea` and the TUI design guides, the
  wide layout (>= 100 columns, `TWO_PANE_MIN_WIDTH`) became a header line plus a bordered box
  per field (`field_block`: label as title, cyan border when focused, red when the save
  refused it, dim otherwise, one column of padding), status and priority side by side with
  every option visible (`choice_box`: chosen bold, reversed when focused, others dim), due,
  project and tags on one row, the description box taking the rest; narrow terminals keep the
  compact rows (`render_form_compact`). `Ctrl+A`/`Ctrl+E` added as Home/End. Mutants on
  `view.rs`: 199 tested, 9 missed on the first pass (styles and scrolling the text snapshots
  cannot see: covered with buffer-style assertions, a scrolled due box, a compact-layout test
  and a 100-column boundary snapshot), 0 missed after one reshaped modifier expression.
- **Detail on demand and wrapped list rows (follow-up 2026-10-06).** The detail pane is no
  longer always there: `show_detail` now applies to both layouts and starts off, so `tasq ui`
  opens on the list alone; `Right` (`show-detail`) shows the selected task's detail beside the
  list from 100 columns or in its place below that, `Left` (`hide-detail`) hides it, `Tab`
  still toggles and `Esc` still closes. The one-pane detail's bottom hint is rendered from the
  `hide-detail` binding. List rows wrap at the pane width (`view::task_lines`): the title's
  words, the due date and each chip are placed in turn, continuation lines indented to where
  the title starts; `list_lines` returns the lines plus the selected task's line range and
  `scroll_offset` takes that range (its last line scrolls into view, the first when the row is
  taller than the pane). Snapshots: `two_pane_list`/`two_pane_detail`, `wrapped_rows`,
  `wrapped_rows_scrolled`. Mutants on `task_lines`/`list_lines`/`scroll_offset`: 2 missed on
  the first pass (two `>` boundaries), 0 after an exact-width wrap test and reshaping
  `scroll_offset` with `saturating_sub`.
- **Full-screen edit view with the description (follow-up 2026-10-06, ADR 0016, todo 13).**
  The popup of ADR 0015 is gone: `e` now replaces the list and the detail with one bordered
  panel, the six single-line rows, a `Description` rule and the description below it; the
  focused label is cyan bold, the terminal cursor sits in the focused text
  (`Frame::set_cursor_position`, asserted with `Terminal::get_cursor_position` on the
  `TestBackend`), a focused choice row shows `‹ value ›`, long single-line values scroll
  under the cursor (`view::window`), the description wraps by character and scrolls
  vertically (`view::wrapped`, `view::wrapped_cursor`, `scroll_offset`). `form::Text` is the
  editor: lines plus a (line, char) cursor with insert, newline, backspace, delete, arrows,
  Home/End and paste; single-line rows never get a newline. Keys: `Tab`/`Shift+Tab` rows,
  `Up`/`Down` cursor in the description else rows, `Left`/`Right` cursor or cycle,
  `Enter` newline in the description else next row, **`Ctrl+S` saves**, `Esc` cancels; the
  status bar is a key bar (`view::form_hints`, keys bold, labels dim, labels dropped under 68
  columns). Store: `ops::set_description` (first section rewritten, duplicates removed,
  missing one inserted after the title) and `clear_description`, 9 fixture pairs, `diff.rs`
  wiring, `edit::Fields.description`; `tasq apply` can change a description. Mutants: core
  `ops.rs` + `edit.rs`, store-nb `diff.rs`, tui `form/update/keys/view/msg` 719 tested, 11
  missed on the first pass (an equivalent guard reshaped, a join case, the `Ctrl+S` guard,
  scrolling and style assertions, unit tests for the wrapping helpers), 0 missed after
  (525 on the rerun of the four files). ADR 0015's view section is superseded; its store and
  core parts stand.
- **Edit form in the TUI (follow-up 2026-10-05, ADR 0015, todo 13).** `e` opens
  `Mode::Form(Box<Form>)` (`crates/tui/src/form.rs`): six rows, Title, Status, Priority,
  Due, Project, Tags; `Up`/`Down`/`Tab`/`Shift+Tab` move the focus, text rows take typing
  and paste, `Left`/`Right` cycle the status (workflow statuses then `none`) and the
  priority, `Enter` validates (`Form::fields`: trimmed non-empty title, `dates::parse_day`
  against `Model::today`, `Tag::from_str`, empty due/project clear) and sends
  `Cmd::Revise(id, Box<Fields>)`; a bad row keeps the form open with the focus on it.
  `dispatch` runs `edit::revise` and reports `[id] updated: title, due` or `[id] unchanged`
  (no write). Prerequisite the todo did not mention: the store could not write a title,
  a due date or a tag set (no awk pass to mirror), so `format::ops` gained `set_title`,
  `set_due`, `clear_due`, `clear_project`, `set_tags` with `tasq`'s own rules
  (`docs/file-format.md`, "Edits the script never made", 29 fixture pairs), and
  `diff.rs` applies them; `tasq apply` benefits (`docs/json.md`). Keys: action `edit` is
  now the form on `e`, the external editor is the new action `editor` on `E`
  (`docs/config.md`, README). `Model::with_today` is set by the CLI from its clock.
  Mutants: core `ops.rs` + `edit.rs` + store-nb `diff.rs` 173 tested, 0 missed; tui
  `form/update/keys/view/model/msg/runtime` 419 tested, 9 missed on the first pass (all in
  the new code: two unreachable fallbacks reshaped, the form key translation tested, the
  title term of the popup width dropped), 0 missed after. Commits 10f8212 (core),
  81d1e1b (store-nb), 86558e6 (ADR), af861ea (tui), 48500f2 (docs).
- **Sync sources on demand (follow-up 2026-10-05, ADR 0014, todo 12).** `source[].auto`
  (default `true`): a bare `tasq sync` and the TUI's sync-all run the `auto` sources only;
  `--source NAME` is now repeatable and runs exactly the named enabled sources, `auto` or not;
  `enabled = false` keeps meaning "cannot run" (naming it is an error). Motivation: the inbox
  bridge costs a full Claude session (~$2.3) per run. The TUI got a source picker: `S` opens
  `Mode::Sources` over `Model::sources` (the enabled `[[source]]` blocks, passed by the CLI as
  `SourceChoice { name, kind, auto }`), the checked set lives in `Model::checked` so it
  survives closing the picker, `Space` and digits toggle, `Enter` sends `Cmd::Sync(names)`,
  `Host::sync(&[String])` turns the names into `--source` flags. Three default keys moved:
  sync-all `S` -> `s`, status `s` -> `t`, editor `e` -> `E` (freeing `e` for the in-TUI edit
  form, todo 13; `t` and `p` stay as quick pickers). The status-bar hint shows `s/S sync` so
  the full line still fits 100 columns (99). Commits: d2e4f13 (core), 42528dd (tui), 96c14a0 (cli), docs
  follow-up.
- **Configurable key bindings (follow-up 2026-10-05, ADR 0013).** `[ui.keys]` maps action
  names to one key or a list (`[]` unbinds); core stores the strings
  (`UiConfig.keys: BTreeMap<String, KeySpec>`, `serde(untagged)` string-or-list, a `<name>`
  wildcard in the `--set` template so `--set ui.keys.quit=q,x` coerces to a list) and
  `tasq-tui` parses them: `keys::{Action, Key, Chord, KeyMap, KeyError}`. `KeyMap::default()`
  is the old `match`; `from_config` overlays the table and rejects an unknown action, a bad
  spec or one chord on two actions of the same mode (`NORMAL`, `PICKER` action sets). The
  `?` overlay (`view::HELP` is now `(&[Action], &str)` rows rendered by `help_rows`) and the
  status-bar hints (`hints(&KeyMap, width)`, unbound actions dropped) come from the map.
  Fixed on purpose: `Ctrl+C` quits everywhere, typing modes, help closes on any key.
  Deviation: modifiers now match exactly, so `Ctrl+Shift+Enter` is no longer
  `launch-detached` (was an accident of the match order). `tasq ui` checks the map before the
  tty check so the error (`<file>: ui.keys.<action>: ...`) is testable without a terminal.
  Mutants on `tui/{keys,view}.rs`: 181 tested, 8 missed, all pre-existing style/guard
  mutants in render code not touched here (`bold`, `chip`, `detail_lines`, `status_bar`
  colour, `render_picker` cursor), invisible to the text snapshots; `keys.rs` and the new
  `help_rows`/`hints` have 0 missed. Commits 195b2b4 (core), 3be8a64 (tui), 115fbee (cli),
  e5ba776 (docs).

- **`post-done` from the TUI (follow-up, ADR 0010).** `Host::after_done(&Task)` is called by
  the TUI's `dispatch` after `edit::done` succeeded; the CLI's `CliHost` (now holding `&App`)
  runs the `post-done` hooks in-process with the same document as `tasq done`. The hook
  runner became `run_hooks_with(.., report: &mut dyn FnMut(HookEvent))` so the TUI can
  collect warnings for the status bar instead of having them printed over the alternate
  screen; `run_hooks` is the printing wrapper and CLI behaviour is unchanged. `CliHost`'s
  unit tests build an `App` from a `.tasq.toml` in a temp dir and use `/bin/sh -c` hooks,
  no fake tools needed. Message on a hook failure: `[1] done: A (post-done hook "x" failed:
  ...)` in the error style; the task is closed either way. Commit: 278e576.

- **Dispatch happens before clap and before the config is loaded.** `tasq_cli::plugins::
  External::parse` scans argv for the first positional, skipping the global flags (the four
  that take a value: `--profile`, `--config`, `--set`, `--color`); an unknown flag hands the
  line back to clap. A plugin runs even when the config is broken and finds out through its
  own `tasq` calls. The plugin replaces the process (`exec`), so its exit code and streams are
  the user's.
- **`--set` needed an environment form.** Without it a plugin's `$TASQ_BIN` calls silently
  used another notebook. `TASQ_SET` (newline-separated `key=value`) lives in the env layer
  of the config loader, above `TASQ_*` for the same key and below `--set`; errors say `env:`.
  `TASQ_CONFIG` is forwarded as an absolute path because a plugin may `cd`.
- **Hooks are CLI-only by construction.** They run from `commands::{create,edit,launch}`;
  the TUI's `d` edits through core and fires nothing, its `Enter` runs `tasq pick` so
  `pre-launch` fires. `pre-launch` runs after the in-progress transition (a refused launch
  leaves the task in progress; documented). Hook stdout is shown only with `-v` so `--json`
  output stays clean; stdin gets the same `Task` JSON as `view --json`.
- The CLI crate has no `mutants` dependency and is excluded from mutation testing, so the
  process wrappers there carry a "Not unit-tested" doc line instead of `#[mutants::skip]`.
- A clippy `too_many_lines` on `Config::load` after adding `TASQ_SET` was fixed by
  extracting `env_layer`, not by an allow. `HooksConfig::is_empty` written as three `&&`
  survived two mutants; `entries().iter().all(..)` plus a per-field test killed them.
- `cargo publish` rejects path dependencies without a `version`, and `cargo install tasq`
  needs every crate the binary depends on to be on crates.io, so all six crates are published
  (plan said core and tasq). Order: core; store-nb, sources, launch, tui; tasq, each waiting
  for the sparse index.
- The release workflow is hand-written (cargo-dist and release-plz are not installed and
  generate code to keep in sync); every target builds natively on its own runner and the job
  asserts the runner's host triple. The pipeline is untested until a GitHub repository exists.
- Listing a generated 500-task notebook takes ~15 ms with a debug build (30 ms as JSON):
  the plan's 50 ms target holds with margin, so no hot path argued for in-process plugins.
- Fixture notebook: the next created task gets id 8 (seven index lines), not 7; tests that
  create a task must use 8.

### TUI decisions (T-801 to T-803)

- **ratatui 0.30** (crossterm 0.29 through `ratatui::crossterm`, so one version of the event
  types). Bracketed paste is a default crossterm feature; `Event::Paste` carries the text and
  the TUI collapses line breaks to spaces because a progress entry is one file line.
- The dependency rule is exact: `tasq-tui` → `tasq-core` + ratatui. Outside-world actions go
  through a three-method `Host` trait (`edit(id, file)`, `launch(id)`, `sync()`); the CLI's
  `Host` runs **the `tasq` binary itself** (`current_exe()` + `pick`/`sync` with `--profile`,
  `--config`, `--set` passed on) so a session from the TUI is literally `tasq pick`, exec'd
  launchers included. `Store::file_of` (default `Ok(None)`, nb returns the path) is what `e`
  needs; `MemoryStore` returns `None` and the TUI says so.
- **Edits moved to core** (`tasq_core::edit::{Value, set, log, done}`, `EditError`): the CLI's
  `commands::edit` shrank to argument parsing and printing. Side effect: `tasq set/done` with
  an empty note now fail with "the note must not be empty" instead of logging an empty entry.
- `dispatch` always ends with a reload (`Msg::Loaded`), after failures too: a `Conflict` or an
  editor that wrote the file never leaves stale rows on screen. Results never clear the status
  message; the next key does.
- Selection is a `TaskId`, not an index, so reloads and filters keep it (`fix_selection`
  falls back to the first visible task). `with_selection` reads the *visible* task, which
  removed a guard mutant; the filter's lone-`#` guard was equivalent to its fallback and was
  removed by reshaping the match (plan rule: fix the shape, do not argue).
- Snapshots: `TestBackend`'s `Display` prints each row quoted, good enough for `insta`;
  styles are asserted on `buffer()[(x, y)]` cells (`fg`, `modifier`). The status-bar hints
  come in three lengths (`hints(width)`), so the 80-column snapshot is not just a truncation.
- `script` gives a program a pty whose size is 0x0: run `stty cols N rows M` inside the
  session or ratatui draws nothing (the smoke test looked broken until then).
- `rustdoc` flags `` [`update`] `` as ambiguous when a module and a function share the name;
  write `` [`update()`] ``.
- The crates.io registry for the pinned toolchain lives under `~/.asdf/installs/rust/<ver>/
  registry`, not `~/.cargo/registry`; `cargo-mutants` is installed there too (`which` misses
  it, `cargo mutants --version` works).

- **Create from the TUI (ADR 0011).** `c` is a one-line title prompt; Enter writes
  `Model::draft(title)` (`TaskDraft::new` plus `Model::default_status`, which `tasq ui` fills
  from `workflow.default_status`; `Workflow` itself carries no default) through
  `Cmd::Create(Box<TaskDraft>)` (boxed: clippy's `large_enum_variant`, the draft is ~270 bytes
  against 48 for the next variant). `dispatch` then emits `Msg::Select(id)` after the reload,
  handled like a result (does not clear the message) and ignored when a filter hides the new
  task. `Host::after_create` mirrors `after_done`; `CliHost::hooks(hook, task)` is the shared
  runner. Status-bar `HINTS` lost the word "move" to stay under 100 columns (97) with
  `c new` added; the `hints()` test now pins both lengths. Commit: 2b71369.

- **Named colour themes (ADR 0018, 2026-10-06, from the UX review).** Every coloured
  thing is a `tasq_core::theme::Role` (17: the eight groups, `chip-bg`, `chip-fg`, `prio-a`,
  `focus`, `selection`, `error`, `dim`, `link`, `header`) and `Theme::color(role)` layers a
  built-in `Preset` under `[ui.theme.colors]` under the old `[ui.colors]` by status name.
  `Color` gained `black` and three attribute pseudo-colours (`dim`, `reversed`, `none`), so
  "the selection is reversed" and "mono has no chip background" are table rows, not special
  cases; the TUI applies `selection` as a background when it is a colour and `chip-bg` as a
  background under `chip-fg`, and keeps the faint attribute for `dim` when colours are off so
  the light theme's grey 245 does not come out plain. `ui.theme` is a table (`preset` plus
  `colors`), not the string the task first asked for: `[ui.theme.colors]` has to live under
  it and a layer merge cannot combine a string with a table; `TASQ_THEME=light` is the short
  form. Clippy's `match_same_arms` fights a data table laid out by preset, so `Preset::color`
  carries one `#[allow]` with the reason; each of the five tables is pinned by a test listing
  all 17 values. The CLI link closer undoes exactly what the opener set (`24;39`, `24;22` for
  `dim`, `24;27` for `reversed`) so glow's own styling around a link survives. 144 mutants in
  `theme` + `config`, 0 missed. The `doctor` verdict colours are not roles (a check report,
  not task styling). Commit: 51ed314.

### Plugin decisions (T-701)

- Layout follows the current plugin docs: `skills/<name>/SKILL.md` (not the legacy flat
  `commands/`), frontmatter `name`, `description`, `argument-hint`, `arguments: [id]` (read as
  `$id`), `allowed-tools: Bash(tasq *)` so the skills' CLI calls need no prompts. The plugin
  `name` is the namespace; the repo root marketplace lists the plugin with `source =
  "./plugins/claude"`.
- `/tasq:sync` runs `tasq sync --json` **first** (the plan said it "ends with `tasq sync`"):
  items the skill finds interactively through Slack/Gmail connectors cannot be handed to the
  headless bridge, so the skill creates them itself with `tasq create --related <permalink>`,
  and the reconciler's legacy URL match dedupes them against any later bridge or sync run.
  Interactive triage only happens when no `llm-bridge` source is enabled. The skill ends with
  a briefing and the `tasq` list.
- `/tasq:wrapup` diffs against `tasq view --json` before writing (only untracked MRs,
  worktrees, sessions), never invents a session id, and asks before `tasq done` unless the user
  said the task is finished. Statuses come from `tasq config show --json`, not a hard-coded list.
- Every write in both skills goes through the CLI; the plugin test rejects a `SKILL.md` that
  mentions `.todo.md`.
- The status line reads `TASQ_TASK_ID` from the environment (Claude Code passes its own
  environment to the status-line command) and the title from `tasq view --raw` (no `jq` needed
  for the task line; `jq` only for the directory fallback).
- `claude plugin validate --strict` is the real validator but needs Claude Code, so it is run by
  hand (documented in `docs/testing.md`); the Rust test covers what can be checked offline.

### Report decisions (T-601/T-602)

- Two single-day parsers, because the same word means different days: `dates::parse_day`
  (due dates, looks forward: `tomorrow`) and `dates::parse_past_day` (reports, looks back:
  a weekday name is the most recent one on or before today, `last <weekday>` the most recent
  one strictly before today, GNU `date -d` semantics). Reports reject `tomorrow`.
- `resolve_range`: `this week` is Monday to `min(Friday, today)` (the plan says weeks are
  Mon–Fri; the script returned Monday..today, a weekend day on Saturdays). `last week` is
  Mon..Fri. Every range is clamped: `to` never passes today, a `from` after today is
  `DateError::InFuture`. The spec is lowercased and whitespace-collapsed, and the CLI joins
  its positional words, so `tasq dates last week` works unquoted.
- The summarizer command reads everything on stdin: the template rendered with `{{day}}`
  (`Friday 2026-10-02`), `{{date}}` and `{{notes}}`. The script passed the prompt as an
  argument and the notes on stdin; one stdin document is a contract any command honours
  (`claude -p`, `llm`, a script). `report.summary.model` is appended as `--model <model>`.
  The `Summarizer` trait and `RawSummarizer` live in core; `CommandSummarizer` in
  `tasq-launch` (the process-running crate), `process::run_with_input` being its only skip.
- Plan open question 2 (raw vs llm default) stays resolved as T-106 did: `llm`, what the
  script did. Without the command on `PATH`, `tasq summary` fails with a message naming
  `--raw` and `report.summary.summarizer = "raw"`.
- `summary --json` always carries the structured `tasks` and the raw `notes`; `summary` is the
  distilled text, or `null` when raw. JSON without `--raw` still runs the command.
- Raw output is the bold header (plain when colour is off) over the script's layout; the
  distilled text is `## <header>` plus the command output, shown through
  `view::show_markdown` (glow on a terminal), extracted from `view::run` for reuse.
- Test fakes under the CLI harness see a `PATH` with only `git`: use `/bin/cat` and shell
  builtins, and `while IFS= read -r line || [ -n "$line" ]` to catch a final unterminated
  line (the rendered prompt ends without a newline).

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

### Source decisions (T-501 to T-505)

- Reconciliation sees **all** tasks, done ones included (`Filter::default().any_done()`), so a
  merged MR that reappears in a sweep matches its done task and is not re-created; done tasks
  never receive changes. New items use the item's `status`/`priority` when set, else the
  source's `status`/`workflow.default_status` and the source's `tags` plus the item's.
- A sweep that no longer lists a tracked open item does not close it by itself: the sync command
  calls `check` for exactly those origins and only a `Done` state closes (merged, closed, approved
  by you, reassigned, gone = HTTP 404). Other HTTP errors fail the source (reported, others run).
- **No wiremock.** HTTP goes through `tasq_sources::http::Transport`; unit tests use
  `ScriptedTransport` (a response script plus recorded requests) and the CLI end-to-end test uses
  `tests/support::FakeHttp`, a 60-line loopback `TcpListener` server with a route table, reached
  through `forge.<name>.url`. Nothing in `cargo test` touches the network.
- `ureq` 3 with rustls is the only network dependency; `http_status_as_error(false)` so 4xx/5xx
  come back as responses and the retry logic owns them. 401/403 never retry; 429/5xx retry twice
  with 1s/2s backoff or `Retry-After`.
- GitHub review requests come from the search API (`is:pr is:open review-requested:<login>`, one
  page of 100); `/issues?filter=assigned` includes pull requests, which are skipped.
- The LLM bridge accepts a bare array, `{"items": [...]}` or claude's `{"result": "..."}`
  envelope (with a code fence) and never closes tasks; its `check` reports every origin as open
  so a configured `flag` still applies (that shape also exists because a function whose body is
  `Ok(Vec::new())` cannot be mutation-tested).
- Headless check of the bridge (2026-10-05, Claude Code 2.1.289, `claude -p --output-format
  json < examples/sources/inbox.md` from a plain shell): `claude mcp list` shows the claude.ai
  Slack and Gmail connectors `Connected`, and the run returned 3 real Slack items in 15 turns,
  89 s API time, $2.28. So `/update-tasks` parity for Slack/Gmail does not need an interactive
  session; `/tasq:sync` step 2 stays the no-bridge fallback. The example ships, with its cost
  documented (`docs/sources.md`, "Headless Claude Code"). Gmail items were not observed.
- The same run broke the parser: despite "Print only a JSON array, no prose", `"result"` was
  `All nine ... Final list:\n\n```json ...`, and `strip_fences` only dropped a fence at the
  very start. It now takes the body of the first fenced block wherever it sits and leaves text
  that already starts with `[`/`{` alone (so JSON containing a fence in a string is safe). The
  real envelope shape is pinned in `parses_arrays_objects_and_claude_envelopes`. Commit: 93ceaf5.
- `tasq mr` / `create --mr`: title from the forge whose `host` matches the URL; a failed lookup
  is a warning and the short reference (`g/p!10`) is used; a gone MR (404) silently falls back.

### Launcher decisions (T-401 to T-405)

- **A failed `exec` gets a test binary of its own (2026-10-06, PR #3 CI).** `Command::exec`
  with a custom env swaps the process-wide `environ` for a temporary array and frees it when
  exec fails, under the env *read* lock only, so a concurrent `spawn` on another test thread
  can read freed memory: insta's `cargo metadata` failed with `Bad address (os error 14)`,
  fell back to the manifest dir and reported `claude_prompt_snapshot` as a new snapshot
  (`crates/launch/crates/launch/tests/snapshots/*.snap.new`). The Claude and shell
  failed-exec checks now live in one test in `crates/launch/tests/exec_failure.rs`; never
  exec from a test that shares its process with other tests.
- `tasq_core::launch`: `LaunchContext { task, file, markdown, workdir, in_worktree, env, statuses }`,
  `resolve_workdir(task, default_project, is_dir)` (first existing worktree → project if it exists,
  else a warning and the default → `NoWorkdir`/`WorkdirMissing` errors), the `Launcher` trait
  (`describe` for `--dry-run`, `launch`, `resume_hint`) and `short_label` (ported, unit-tested).
  `Session.at`/`Worktree` line formatters are re-exported from `format` for the prompt.
- The prompt is data: `templates/claude.md` with `{{name}}` placeholders and
  `{{#name}}...{{/name}}` sections (kept only when the variable is non-empty); section markers sit
  inline so the blank lines match the script's `${wt_list:+...}` layout. Unknown placeholders are
  errors naming them. It names `tasq log/set/project/worktree/session/mr/done` and `/tasq:wrapup`.
- Launchers that replace the process (`shell`, `claude`) use `process::exec` and are tested
  through `command()`/`describe()` plus a CLI integration test with a fake `claude` on `PATH`
  (the exec'd fake records cwd, env and the prompt). `tmux` and `herdr` run subprocesses and are
  tested with fake executables; the herdr launcher takes its fallback launcher and prompt
  function as injected boxes, so the fallback path is tested with a recording launcher instead of
  exec'ing anything from a test.
- herdr port: `worktree list --cwd` is skipped when the workdir is the default project (as the
  script's `$workdir != $DEFAULT_WORKTREE`); `workspace create` or `tab create` with `--env
  TASQ_TASK_ID/TASQ_NOTEBOOK(/TASQ_PROFILE) --no-focus`; `agent start task-<id>` retried as
  `task-<id>-<pid>`; `agent prompt`; `agent focus`, `tab focus`, `workspace focus`. JSON is read
  with `serde_json`, searching every object for the key (herdr repeats ids across nested
  objects). Without a pane the fallback launcher runs in the current pane.
- `launch.default` accepts `auto` (herdr when `HERDR_ENV` is set, else claude), which is what the
  script did implicitly; the default stays `claude`. `--dry-run` never writes (the in-progress
  change is reported as skipped) and `--json` with `--dry-run` prints the context and steps.
- Launch-crate survivors fixed by shape, not by argument: a hand-written `impl std::fmt::Debug`
  is not matched by the `-E 'impl Debug'` exclude (write `impl Debug` via `use std::fmt::Debug`);
  an `in_herdr: true` override that could never differ from the derived value was removed; the
  `#[cfg(not(feature = "herdr"))]` stub is `#[mutants::skip]` because the default-feature test
  build never compiles it.

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
  `branch_slug`, `sibling_worktree`, `project_dir`) with the process-running impls in
  `tasq-launch` (`process::run` is the only `#[mutants::skip]`). `GitManager` (the default) uses
  `git worktree add` into `<project>-<branch-slug>` next to the project and reuses the directory
  when it exists. `CommandManager` runs `work.worktree_command`, a template with `{branch}`,
  `{project}`, `{new}` (`-b` when the branch exists neither locally nor on `origin`; `{new:x}`
  for another flag), inside the project; last stdout line = path, other lines shown. **tasq has
  no gwm code**: gwm is `worktree_command = "gwm create {new} {branch} --no-tmux -s"`, which is
  what the script's `GWM_SHELL_MODE=1 gwm create [-b] <branch> --no-tmux -s` becomes. Config
  validation rejects `command` without a `worktree_command`.
- `session`: the file never stores a launcher, so `Session.launcher` stays `None` on write (a
  `Some` would be `Unsupported`); the resume hint (`claude --resume <id>`, `tmux attach -t`)
  comes from `--launcher`/`launch.default` in `commands::session::resume_hint`, to move onto the
  `Launcher` trait in Phase 4. Session ids may not contain backticks (the file delimiter).
- `apply` reads the `view --json` envelope only (`schema` must be 1, `task` required); errors
  are prefixed `apply:` and name the field via serde's message. Unsupported edits surface the
  store's `unsupported: changing title of task 3 (...)` message unchanged.
- Config module mutants: replacing gwm surfaced two survivors in untouched `load.rs` code
  (`leaf_keys::walk`'s empty-table guard and `coerce`'s integer arm, which no key uses yet).
  Both now have tests (`an_empty_table_is_recorded_as_a_leaf_key`, a unit test calling `coerce`
  with a hand-built template) rather than being argued equivalent. Config: 90 mutants, 0 missed.
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
- No dirty probe before a checkpoint (2026-10-06, perf): the store only checkpoints after a write
  changed bytes, so the tree is dirty by construction. nb runs `nb git checkpoint --wait` alone
  (one bash spawn instead of two); native runs `git add -A` + `git commit` and only on commit
  exit 1 asks `git status --porcelain` whether the tree is clean (nothing to commit, an ignored
  file: `Ok(false)`) or not (a rejecting pre-commit hook, which also exits 1: a failure). nb's
  `true` now means "nb ran the checkpoint", since nb's exit status cannot say more.
  Release build, 43-file copy of the real notebook, signing off, median of 2x30 `tasq set`:
  nb 145 -> 84 ms, native 11.3 -> 9.8 ms. Both bookkeepers: 47 mutants, 38 caught, 9 unviable,
  0 missed.

### nb facts (probed 2026-10-04 with nb 7.25.4 in an isolated NB_DIR)

- **`nb git checkpoint` commits asynchronously unless `--wait`**; and any nb read command
  (`nb todos` included) commits a dirty notebook in the background as `[nb] Commit`. Checkpoint
  right after writing, with `--wait`, or nb takes the commit with its own message.
- `nb index verify` exits 1 with "Index corrupted" on stderr in this version (an earlier probe saw
  exit 0): parse the text, not just the exit code.
- `nb index add` appends `basename\n` and skips names already listed; it does not repair a missing
  trailing newline on the previous line.
- `nb git dirty` exits 1 both when clean and when the notebook is not a git repo; check `.git` first.
- `nb git checkpoint --wait` exits 0 even when it committed nothing or the commit failed:
  `_git_checkpoint` ends the commit with `|| return 0` (no auto-sync) or `|| :` (auto-sync).
  Its exit status says nothing about the commit; no longer probed with `nb git dirty` first.
- Each nb (or native) commit is GPG-signed when the user's global git config has
  `commit.gpgsign=true`: ~440 ms per commit on the author's machine, three times every other
  cost of `tasq set` combined. Benchmarks set `commit.gpgsign false` in the notebook copy.
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
- **Random CLI test failures under the address-space cap (2026-10-06).** Two or three
  `crates/cli/tests/cli.rs` tests failed per full `cargo test --workspace`, different ones
  each time, and always passed alone. With `--test-threads=48` ten runs in twelve failed and
  the panics were `failed to spawn thread` (assert_cmd's stdout/stderr readers) and `failed
  to allocate an alternative stack: Cannot allocate memory`. Cause: the 4 GiB `ulimit -v` of
  `scripts/test-runner` is an address-space cap, and glibc's malloc reserves a 64 MiB arena
  per allocating thread, up to 8 per core (256 on this machine): the test process with one
  thread per core plus two reader threads per child ran out of address space, not memory.
  Fix: the runner exports `MALLOC_ARENA_MAX=2`. Twelve stress runs at 48 threads then passed,
  as did repeated full workspace runs. The ETXTBSY probe loop in `TestEnv::fake_tool` was a
  guess at the same symptom and stays (it is correct for what it covers).
- **Every cli test failing with `NotFound` on the fixture notebook after the repo moved
  (2026-10-06).** `cargo test --workspace` had all 111 `crates/cli/tests/cli.rs` tests and
  `plan_section_4_6_example_loads_as_is` (core) panic in 0.00 s with `fixture notebook
  exists: No such file or directory`; `cargo test -p tasq` passed. Cause: the test helpers
  bake `env!("CARGO_MANIFEST_DIR")` into the binary, the checkout had moved
  (`~/code/tasks` to `~/code/tasq` to `~/code/tasq_lab/tasq`) and cargo does not refingerprint
  on a manifest-dir change, so the workspace-feature build kept binaries with the old paths
  (`strings target/debug/deps/cli-* | grep /home/pau/code` showed three). The `-p` build has
  a different feature hash and had been rebuilt after the move. Fix: `cargo clean` (6.5 GiB)
  and a cold build; the full workspace then passed. Do this after any move of the checkout.

## Deviations from the plan

- T-901: `TASQ_BIN` and `TASQ_SET` added to the plugin environment (the plan listed
  `TASQ_PROFILE` and `TASQ_CONFIG`); hooks are configured under `[hooks]` rather than
  discovered; `tasq apply` closing a task fires no `post-done` (the TUI's `d` key does since
  ADR 0010).
  The reference plugin is a sketch of the author's tool (date range + per-day notes + even
  split), not the real HiBob/GitLab poster, which stays private.
- T-902: the gif came a day late (2026-10-05): no recorder was installed, so it is a VHS tape
  run through the `ghcr.io/charmbracelet/vhs` docker image (`scripts/demo-gif`) against a
  notebook built by `docs/demo/setup.sh`. `cargo install tasq` cannot be followed yet because nothing is published.
- T-903: hand-written workflow instead of cargo-dist/release-plz; all six crates are
  published, not two; the GitHub release and crates.io steps have never run. `CHANGELOG.md`
  is prepend-only: the `0.1.0` notes are hand-written; from the first tag on, each release
  section is rendered by `git cliff --unreleased --prepend` and then edited by hand into
  user-facing notes (decided 2026-10-05, `docs/release.md`). `git-cliff` 2.14.2 is installed
  locally; the full-file regeneration (77 commit subjects) was rendered once and discarded.
- `create` (CLI and TUI, 2026-10-05): a task created without `--project` tracks the directory
  `tasq` ran in (`LoadOptions.cwd`, canonicalised; the TUI gets it as `Model::default_project`
  from the CLI). The script left `## Project` out, and `pick` then failed on
  `work.default_project` unset. `tasq sync` tasks are unchanged.
- `sync --interactive` (2026-10-05, not in the plan): the script's `tasks update` opened an
  interactive Claude session; the plan replaced it with headless `tasq sync` plus the
  `/tasq:sync` skill, which left the morning entry point as two steps. `tasq sync
  --interactive` execs `claude "/tasq:sync"` in `work.default_project` through
  `tasq_launch::command_in` (the `ClaudeLauncher` wrapping without a task: `envrc_status`,
  `wrap_command`, `envrc_warning`), with `TASQ_NOTEBOOK`/`TASQ_PROFILE` set. A flag on `sync`
  rather than a `briefing` command because the skill is already called sync; a shell alias was
  rejected because it loses `work.default_project`, `launch.env` and the `direnv allow` warning
  and has no `--dry-run`. `--interactive` conflicts with `--source` and ids (clap); the skill
  decides which sources run.
- T-904: the one-week side-by-side verification is a manual acceptance left to the author;
  `docs/migration.md` gives the daily procedure.
- T-801: the README gif was recorded with VHS in docker rather than a local `vhs`/`asciinema`
  (none is installed); the terminal path was checked with `script` first (see `docs/testing.md`). `d` asks for an optional
  final note (the `tasq done [note]` shape) rather than closing on the keypress alone.
- T-802: launching does not "run the launcher" in-process: the TUI runs `tasq pick <id>` (and
  `tasq sync`, `$EDITOR`) as a child with the terminal released, then waits for Enter after
  `pick`/`sync` so their output can be read (ADR 0009). The edit operations moved into
  `tasq_core::edit`, so "the same core functions the CLI uses" is literal; `tasq set`/`done`
  now reject an empty note.
- T-803: `[ui.colors]` keys are status names plus `no-status` (the plan left the names open);
  the same table colours the CLI's group headers (`tasq_core::theme`). Below 100 columns the
  detail pane is reached with `Tab` (the plan only said "single pane").
- T-301: usage errors exit 2 (clap convention), not 1; only domain errors exit 1 with `tasq: ...`.
- T-302: `--json` emits `{"schema":1,"tasks":[...]}` rather than a bare array (FR-4 asks for a
  versioned schema on every command).
- T-305: no nb passthrough (`tasks view <id> [args]`); `--raw` prints the file instead.
- T-306: no gwm adapter. `work.worktree_manager` is `git` (default, `<project>-<branch-slug>`)
  or `command` (user template); gwm is one line of config. Decided 2026-10-04 so tasq carries no
  dependency on the author's provisioning tool.
- T-307: the resume hint came from a CLI table until Phase 4; it is now `Launcher::resume_hint`.
- T-403: the tmux launcher opens a shell window, not a Claude session (what the plan says);
  the window is named `<id> <short label>`.
- T-404: `launch.default = "auto"` added (not in the plan's value list) to keep the script's
  "herdr when inside herdr" behaviour configurable rather than implicit.
- T-503: `wiremock` replaced by an injected `Transport` plus a loopback test server (no tokio in
  the dev-dependency tree). `forge.<name>.url` added for self-hosted layouts and tests.
- T-504b: `title`, `labels`, `exclude_labels`, `projects`, `create_new`, `close_when_done` and
  `flag` are `[[source]]` keys (the plan left their spelling open).
- T-601: the summarizer command gets the prompt and the notes on stdin (the script passed the
  prompt as an argument); `report.summary.prompt_file` added so the prompt is a user-editable
  template (plan section 8); reports reject `tomorrow`.
- T-602: `this week` ends on Friday (the script ended on today, even on a weekend); explicit
  ranges are clamped to today and a start after today is an error; `--json` adds `days` and
  `working_days` lists, not in the plan, so the time-logs plugin needs no date arithmetic.
- T-701: `claude plugin install` needs the repository registered as a marketplace first
  (`claude plugin marketplace add <repo>`), so the repo root carries `.claude-plugin/
  marketplace.json`; `claude --plugin-dir plugins/claude` is the no-install path. `/tasq:sync`
  starts with `tasq sync` rather than ending with it (see Plugin decisions).
- T-001: repository URL in `Cargo.toml` is a placeholder (`https://example.invalid/tasq`) until a
  GitHub repo exists. Extra just recipes `default` and `fmt-check`.

- T-802: `c` (create a task from the TUI) was not in the plan's key list; added as title-only
  with the configured default status, refined afterwards with `s`/`p` (ADR 0011). The
  "`post-create` still has no TUI counterpart" negative of ADR 0010 no longer holds.

- Perf, no dirty pre-check (2026-10-06): the task asked for nb's silent no-op to return
  `Ok(false)`; nb exits 0 either way, so the nb bookkeeper returns `true` once nb ran instead of
  comparing HEAD before and after. Nothing in the store reads the `bool`.

## Open questions raised during implementation

- (resolved 2026-10-04) `tasq list --json` never included done tasks, so a plugin could not
  enumerate closed work. `tasq list --all` (a `DONE` group after the open ones, last in the
  JSON array) and `tasq list --done` were added; `Scope` in `commands::list`, `[ui.colors]
  done`, `Theme::done_color`.
