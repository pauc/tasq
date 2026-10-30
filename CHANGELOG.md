# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/pauc/tasq/releases/tag/v0.1.0) - 2026-10-30

First release: a terminal task manager over plain markdown task files.

### Added

**Tasks and the store**

- One task per `<ULID>.md` file in a store directory (default
  `~/.local/share/tasq/stores/default`): frontmatter (`id`, `status`, `priority`, `due`,
  `tags`, `rank`), then title, description, links, project, worktrees, sessions and a dated
  progress log ([`docs/file-format.md`](docs/file-format.md)). Edits keep every byte they do
  not understand, so hand edits coexist.
- Integer ids, highest plus one, never renumbered. Writes take `.tasq/lock` and fail on a file
  changed since it was read. Duplicate ids are reported by `tasq doctor` with the fix.
- Configurable statuses (`[workflow]`, default `in-progress`, `ready`, `waiting`, `blocked`,
  `later`), priorities `A`/`B`/`C`, due dates (`today`, `tomorrow`, ISO), topic tags, manual
  rank within a status group.

**Commands**

- `list` (`ls`) with status, tag, priority and text filters, `--all`, `--done`.
- `create`, `set`, `log`, `done`, `reopen`, `rename`, `due`, `tag`, `desc` (`-`, `""`,
  `--edit`), `move` (`--before`, `--after`, `--top`, `--unrank`), `project`, `session`, `mr`,
  `apply` (JSON in). No-op edits write nothing; `done` refuses a done task.
- `view` (glow with OSC 8 links on a terminal, markdown otherwise), `summary [DAY]` (a day's
  notes per task, distilled by `report.summary.command` or `--raw`), `dates [SPEC]`.
- `store info`, `store lock -- CMD`, `doctor`, `config show`, `plugins list`,
  `completions SHELL`.
- `--json` on every command, `"schema": 1` ([`docs/json.md`](docs/json.md)). Exit 0, 1
  (fixable error), 2 (usage or internal). `--profile`, `--config-dir`, `--set`, `--color`,
  `--no-pager`, `-v`; `TASQ_NOW` fixes the clock.

**Work sessions**

- `next` (first in-progress task, else first ready) and `pick ID` start a session in the
  task's worktree, else its project, else `work.default_project`.
- Launchers `claude` (templated prompt, `direnv exec` when allowed), `shell`, `tmux`,
  `herdr`, `auto`; `--window`/`--no-focus` for a new window; `--dry-run`; `-v` and
  `launch.log` trace the launcher commands.

**Worktrees** (opt-in: `worktrees_enabled = true`;
[ADR 0022](docs/adr/0022-worktrees-opt-in-core-feature.md))

- Declared from `[worktree]` `path`/`branch` templates (`worktree set`, `create --worktree`,
  the TUI) or tracked from a path; created at launch or `worktree up` with provisioning
  (`copies`, `symlinks`, `templates`, `ports`, `variables`) and `[worktree.hooks]`.
- Released at close: worktrees removed, branches deleted when integrated or pushed, anything
  with changes or in use kept with a warning. `worktree ls`, `switch` (`shell-init`), `path`,
  `prune`, `config`.
- Per-repository settings in `repos/*.toml`.

**Sources** (`tasq sync`; [`docs/sources.md`](docs/sources.md))

- GitLab and GitHub review requests and work items: a task per item, closed when merged,
  approved, closed or unassigned (`done_when_*`). `drafts`, `exclude_authors`, tokens from a
  command.
- `llm-bridge`: a command's JSON becomes tasks.
- `--source` (repeatable), `auto` sources, `--dry-run`, `sync ID...` re-checks,
  `--interactive` for the Claude Code briefing.

**Terminal UI** (`tasq ui`; [`docs/config.md`](docs/config.md#terminal-ui))

- Grouped list with a detail pane (side, bottom or full), filter, status and priority
  pickers, note and done prompts, create and edit views with a calendar picker, editor,
  `view`, sessions (`Enter`, new window with `Ctrl+Enter`/`Shift+Enter`), sync and source
  picker, folded groups, DONE group, Today view, `Ctrl+K`/`Ctrl+J` to move a task, `?` help.
- Reloads when the store changes on disk. Colour presets, `[ui.theme.colors]`, `NO_COLOR`,
  every key rebindable in `[ui.keys]`.

**Configuration** ([`docs/config.md`](docs/config.md);
[ADR 0031](docs/adr/0031-config-only-in-the-config-directory.md))

- Config directory only (`--config-dir`, `TASQ_CONFIG_DIR`, `~/.config/tasq`): `config.toml`,
  `profiles/<name>.toml`, `repos/*.toml`, then `TASQ_*` and `--set`. Errors carry
  file:line:column; `config show` names the layer of each key. Examples in
  [`examples/config/`](examples/config/).

**Plugins and hooks** ([`docs/plugins.md`](docs/plugins.md);
[ADR 0030](docs/adr/0030-store-sync-as-a-plugin.md))

- `tasq NAME` runs `tasq-NAME` from `PATH` with the same configuration.
- `[hooks]` `post-create`, `post-done`, `pre-launch` with the task as JSON on stdin.
- `[plugins] enabled` plugins get `store-open`, `post-write` and `doctor` events, keep
  lasting warnings shown by every command, and rewrite files under `tasq store lock`.
  `[plugin.<name>]` for their settings. Examples in `examples/plugins/`.

**Store sync** (`tasq-git`; [`docs/git-sync.md`](docs/git-sync.md))

- Second binary, enabled with `[plugins] enabled = ["git"]`. `tasq git init [URL]`,
  `tasq git clone URL`, `tasq git sync`; every command's writes are committed in a background
  job that fetches, rebases and pushes on an `interval`.
- Conflicting rebases are aborted with local commits kept and a warning naming the tasks;
  duplicate ids from a pull warn until fixed. `doctor` checks identity, remote, push, fetch
  and rebase state.

**Claude Code plugin** (`plugins/claude`)

- `/tasq:wrapup`, `/tasq:sync`, a status-line snippet; the repository is its own marketplace.

**Distribution**

- Tarballs for Linux x86_64/aarch64 and macOS x86_64/arm64 (`tasq`, `tasq-git`, `.sha256`);
  `cargo install tasq`; eight crates on crates.io. GPL-3.0-or-later, MSRV 1.90. No Windows.
