# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Work sessions in a new window: `tasq pick|next --detached [--no-focus]` opens the session
  with `launch.detached` (`herdr`, `tmux`, or `auto` for whichever the terminal runs in)
  instead of the current terminal, in the background with `--no-focus`. In `tasq ui`,
  `Ctrl+Enter` opens the selected task in a new window and switches to it, `Shift+Enter`
  opens it without leaving the list; plain `Enter` is unchanged. `launch.herdr.placement`
  (`auto` | `workspace` | `tab`) says what a herdr window is. The two chords need a
  terminal with the kitty keyboard protocol (herdr has it). ADR 0012.
- `tasq list --all` (done tasks in a `DONE` group after the open ones, also in `--json`) and
  `tasq list --done` (only closed tasks); the other filters apply to both. `[ui.colors] done`
  colours the group.
- Plugins (ADR 0006): `tasq <name> [args...]` runs an executable `tasq-<name>` from `PATH`
  when `<name>` is not a built-in command, with `TASQ_BIN`, `TASQ_PROFILE`, `TASQ_CONFIG` and
  `TASQ_SET` forwarded so the plugin sees the same configuration; `[hooks]` config
  (`post-create`, `post-done`, `pre-launch`) runs command lines with the task as JSON on
  stdin, a failing `pre-launch` hook aborting the launch; `tasq plugins list`; `TASQ_SET`
  environment variable (newline-separated `key=value` overrides). Reference plugin
  `examples/plugins/tasq-tlogs` and hook `examples/plugins/hooks/log-event.sh`;
  `docs/plugins.md`.
- Documentation for a new user: README (install, quick start, concepts, commands),
  `CONTRIBUTING.md`, `docs/config.md` with every key and its default, `docs/migration.md`
  (running alongside the original `tasks` script), `docs/release.md`, `examples/config/`.
- Release pipeline: `.github/workflows/release.yml` builds Linux and macOS tarballs on a
  `v*` tag, creates the GitHub release with git-cliff notes (`cliff.toml`), publishes the
  crates in dependency order and renders a Homebrew formula from `homebrew/tasq.rb.template`.

- `tasq ui`: a ratatui terminal UI over the same tasks. Grouped list with the CLI's ordering,
  the selected task's detail beside it (one pane below 100 columns, `Tab` to switch), `/`
  filter, status and priority pickers from the configured workflow, note and done prompts,
  `c` to create a task from a title (with `workflow.default_status`; the `post-create` hooks
  fire as for `tasq create`), `e` for `$EDITOR`, `Enter` for a work session and `S` for
  `tasq sync` (both run as child `tasq` processes while the terminal is released), `?` help.
  Colours follow `[ui.colors]`
  and `NO_COLOR`. The edit operations behind `tasq set/log/done` moved to `tasq_core::edit`
  so both front ends share them; `tasq_core::store::MemoryStore` is the in-memory store for
  tests and `Store::file_of` tells a UI which file to open.

- `tasq` CLI with every editing command of the original script: `list` (default, with
  status/tag/priority/text filters), `create`, `set`, `log`, `done`, `view` (glow rendering
  with OSC 8 links on a terminal), `project`, `worktree` (track, or `--create` through gwm or
  git), `session`, `mr`, `apply` (JSON in), `store info`, `store sync`, `doctor`,
  `config show` and `completions`; `--json` on every command (`docs/json.md`),
  `--color`/`--no-color`/`--no-pager`, `--profile`/`--config`/`--set`, `TASQ_NOW` for
  reproducible timestamps.
- `tasq next` and `tasq pick <id>`: open a work session in the first tracked worktree that
  exists, else the project, else `work.default_project`, with `--launcher` and `--dry-run`.
  Launchers: `claude` (prompt from a template, `direnv exec` when the `.envrc` is allowed),
  `shell`, `tmux`, `herdr` (workspace or tab, Claude agent, prompt pasted in) and `auto`.
- `tasq sync`: GitLab and GitHub review-request and work-item sources (create tasks, close
  them when merged, approved, closed or reassigned), an LLM bridge for anything unstructured,
  `--source`, `--dry-run`, `--json` and per-task re-checks. `tasq mr` resolves titles through
  the configured forge.
- `tasq summary [DAY] [--raw]`: the day's progress notes grouped per task (default: the last
  working day; weekday names and `last <weekday>` look back), distilled by
  `report.summary.command` reading a prompt template on stdin, or printed raw. `tasq dates
  [SPEC]`: `this|last week`, `this|last month`, `last N days`, days and day pairs resolved to
  `FROM TO`, with `--json` for scripts and plugins.
- Claude Code plugin `plugins/claude` (namespace `tasq`): `/tasq:wrapup` (progress notes,
  tracked MRs/worktree/session, final status through the CLI) and `/tasq:sync` (`tasq sync`,
  inbox triage when no LLM bridge is configured, a briefing); the repository doubles as a
  one-plugin marketplace; a status-line snippet shows `[id] title`.
- nb-compatible store (`tasq-store-nb`) with nb and native bookkeepers.
- Core domain model, lossless markdown format, queries and layered TOML config.
- Cargo workspace with the `tasq-core`, `tasq-store-nb`, `tasq-sources`,
  `tasq-launch`, `tasq` and `tasq-tui` crates, shared lints and quality gates.
