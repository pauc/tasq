# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Review-request sources skip draft merge requests unless `drafts = true`, and
  `exclude_authors = ["dependabot"]` drops merge requests by those usernames (exact), as the
  `/update-tasks` skill did; without it a first `tasq sync` against a busy GitLab creates a
  task per dependency bump. Only listing is filtered: a tracked task still closes when its MR
  merges. `tasq config show` prints both keys. The docs and examples now spell the GitLab
  token command as `glab config get token --host <host>`: glab 1.118 has no `auth token`
  subcommand and prints help with exit 0, which was sent as the token.
- Manual order within a status group: `tasq move <id> --before <id>` / `--after <id>` places
  a task next to another task of its group, `--top` puts it first (above every priority) and
  `--unrank` hands it back to the priority, due, id order; `tasq list`, `tasq ui` and
  `tasq next` follow. In `tasq ui`, `Ctrl+K`/`Ctrl+J` (`move-up`, `move-down`) move the
  selected task one slot, next to its visible neighbour. The rank lives in a `## Rank` section
  of the task file, is never shown, and is kept across status changes and close/reopen;
  `rank` is in every task JSON (`tasq move --json` adds `changed` and `ranked`) and
  `tasq apply` keeps it unless the document says `"rank": null`. The common move rewrites one
  file; a move that ranks or renumbers neighbours goes out as one commit (ADR 0025).
- `tasq ui` detail pane: `J`/`Ctrl+E` and `K`/`Ctrl+Y` scroll it (`detail-down`, `detail-up`),
  `PgUp`/`PgDn` page it when it replaces the list, a scrollbar shows the position, and every
  progress note is listed (no longer the last 8). Related and merge request entries show
  their label or a short reference (`!7`, `#42`, `example.com/doc`) as OSC 8 hyperlinks
  (`ui.no_osc8` turns them off); Project and Worktrees paths show the home directory as `~`.
  `v` (`view`) runs `tasq view` on the task; with less as `ui.pager` it stays open full screen
  even on a short task and `q` returns to the list.
- Launch trace for `tasq pick|next`: `-v` prints every `herdr`/`tmux` command with its result
  and output on stderr, and `launch.log` (`TASQ_LAUNCH_LOG`) appends it to a file with a header
  and the outcome. The herdr launcher now names herdr's error when it cannot open a pane, warns
  when a focus step fails, and reports the error of both `agent start` attempts.
- `tasq rename`, `tasq due`, `tasq tag` and `tasq desc` (TEXT, `-` for stdin, no TEXT prints,
  `""` clears, `--edit` opens `$VISUAL`/`$EDITOR`), each with `--note` and `--json`; `--note`
  on `set`, `done` and `reopen` as an alias of the positional note. An edit that changes
  nothing writes nothing and prints `[id] <field> unchanged` (also in the TUI's status line;
  with `--json`, `"changed": []` next to the task, which otherwise names the fields written);
  with a note it fails (exit 1), since the note is not logged either. `desc -` and `--edit`
  drop blank lines at either end and CRLF; an editor that exits non-zero aborts `--edit`.
  `tasq done` refuses a task that is already done (exit 1), so it no longer retries a failed
  worktree release: `tasq worktree release` does that.
- `[worktree]` configuration for worktrees per task (ADR 0022): `enabled` (default `false`),
  `path` and `branch` templates, `base`, `on_create`, `on_done`, `delete_branch`, `symlinks`,
  `copies`, `copy_ignored`, `templates`, `variables`, `ports` and `[worktree.hooks]`, with
  `TASQ_WORKTREE_*` variables (`TASQ_WORKTREE_ENABLED` for `enabled`). The keys are validated;
  the lifecycle that uses them is still being built.
- Worktrees are invisible unless `worktree.enabled = true`: `tasq worktree` is hidden from
  help and completions and refused, task JSON and hook documents leave out `worktrees` (and
  `pick --dry-run --json` `in_worktree`), the TUI detail and the Claude prompt say nothing about
  them, and sessions start in the project. `tasq apply` keeps the stored worktrees when the
  document has no `worktrees` key.
- Worktree template variables (`id`, `title`, `slug`, `project`, `project_name`,
  `project_parent`, `repo`, `branch`, `branch_safe`, `branch_db`, `port`, `worktree`,
  `worktree_name` and `[worktree.variables]`), checked per template at load time. The `{{name}}`
  engine moved to `tasq_core::template`; `tasq-launch` no longer depends on `regex`.
- Every task created with `tasq create` or the TUI's `c` key tracks a project: `--project`
  when given, else the directory `tasq` runs in. `tasq pick` therefore always has somewhere
  to start; `tasq project <id> <path>` changes it. The original script left the section out
  unless `--project` was passed.
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
  with OSC 8 links on a terminal), `project`, `worktree` (track, or `--create` with
  `git worktree add`), `session`, `mr`, `apply` (JSON in), `store info`, `store sync`, `doctor`,
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

### Changed

- **Breaking:** no positional argument shares a slot with subcommands any more (ADR-0028),
  so the generated shell completions reach every subcommand (`tasq worktree rel<TAB>`,
  `tasq store <TAB>`, ...).
  - The list filter is `tasq list [WORD]`, with the new alias `tasq ls [WORD]`; the bare
    `tasq <WORD>` shorthand is gone (`tasq ready` is now an unknown command, exit 2, unless a
    `tasq-ready` plugin exists). A bare `tasq` still lists open tasks.
  - Declaring or tracking a worktree is `tasq worktree set <ID> [BRANCH|PATH]` (and
    `tasq worktree set <ID> --create <BRANCH>`); the `tasq worktree <ID> ...` form is gone. The
    session prompt and the `/tasq:wrapup` skill use the new form; a bare `tasq worktree` prints
    the subcommand list.

### Fixed

- The herdr launcher no longer races the new pane's shell: `herdr agent start` was sent a
  few milliseconds after `workspace create`, before zsh (rc files, direnv, gitstatus) reached
  its prompt, and herdr answered `agent_pane_busy`; the name-collision retry 100 ms later
  only masked it. A busy pane is now retried every 100 ms for up to 5 s. `launch.log` and
  `-v` keep one `agent start` record per name plus a `waited N ms for the pane's shell`
  note; a pane still busy after that fails with herdr's message, the wait and the pane's
  foreground process from `herdr pane process-info` (`gitstatusd (pid 42)`).

### Removed

- `work.worktree_manager`, `work.worktree_command` and their `TASQ_WORKTREE_MANAGER` and
  `TASQ_WORKTREE_COMMAND` variables: `tasq worktree --create` always runs `git worktree add`,
  and worktree provisioning moves to `[worktree]`. A config that still sets them fails with the
  file, line and column and a pointer to `[worktree]` (ADR 0022).
