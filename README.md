# tasq

[![CI](https://github.com/pauc/tasq/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pauc/tasq/actions/workflows/ci.yml)
[![Mutants (nightly)](https://github.com/pauc/tasq/actions/workflows/mutants-nightly.yml/badge.svg)](https://github.com/pauc/tasq/actions/workflows/mutants-nightly.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

Terminal task manager over plain markdown task files.

Each task is one markdown file in a store directory: a small `key: value` block for the id,
status, priority and the other fields, then the title, description, links and a dated progress
log. `tasq` edits the files in place and keeps every byte it does not understand, so hand edits
and `tasq` edits coexist. On top of the files it adds a status workflow, priorities, due dates,
progress notes, a standup summary, work sessions (Claude Code, tmux, herdr, a shell) started
in the task's worktree, external sources (GitLab and GitHub review requests and issues, an LLM
inbox) that create and close tasks, a full-screen terminal UI, `--json` on every command, and
out-of-process plugins.

![tasq listing tasks, setting a status and printing a standup summary, then tasq ui with the status picker and the help overlay](docs/demo/tasq.gif)

The recording is [`docs/demo/demo.tape`](docs/demo/demo.tape) against a throwaway store;
`scripts/demo-gif` re-renders it with [VHS](https://github.com/charmbracelet/vhs) in docker.

## Install

| Method | Command |
|---|---|
| crates.io (once published) | `cargo install tasq` |
| Prebuilt binary | Download `tasq-vX.Y.Z-<target>.tar.gz` for your platform from the [GitHub releases](https://github.com/pauc/tasq/releases) (Linux x86_64 and aarch64, macOS x86_64 and arm64; a `.sha256` sits next to each tarball), unpack it and put `tasq` on your `PATH` |
| Homebrew | `brew install pauc/tasq/tasq` (the tap is `pauc/homebrew-tasq`) <!-- TODO: the tap repository does not exist yet; the first release with HOMEBREW_TAP_TOKEN set creates the formula --> |
| From source | `git clone https://github.com/pauc/tasq && cd tasq && cargo install --path crates/cli` |

The binary is called `tasq`. Rust 1.90 or newer builds it from source. Nothing else is
required: git, glow, claude, direnv, tmux, herdr, glab and gh are all optional and
`tasq doctor` tells you which ones it found and what each one would add.

Shell completions:

```sh
tasq completions bash > ~/.local/share/bash-completion/completions/tasq
tasq completions zsh  > ~/.zfunc/_tasq        # with ~/.zfunc in fpath
tasq completions fish > ~/.config/fish/completions/tasq.fish
```

How releases are built and published is in [`docs/release.md`](docs/release.md). Coming from
the original `tasks` script: [`docs/migration.md`](docs/migration.md).

## Quick start

1. Check the environment:

   ```sh
   tasq doctor
   ```

   One line per check (`OK`, `WARN`, `FAIL`) with a fix for anything that is not OK: the
   config files, the store directory and its task files, and the optional tools. Exit code 1
   when a check failed.

2. List the open tasks of the default store, grouped by status:

   ```sh
   tasq
   ```

   The store is `~/.local/share/tasq/stores/default` (`$XDG_DATA_HOME/tasq/stores/default`
   when set); it is created by the first `tasq create`. Another store, for one command or for
   the shell session:

   ```sh
   tasq --set store.name=work
   TASQ_STORE_NAME=work tasq
   ```

   Permanently, in `~/.config/tasq/config.toml` (or a `.tasq.toml` in a project):

   ```toml
   [store]
   name = "work"
   ```

   `tasq list ready`, `tasq ls gitlab` and `tasq ls A` print one status group, the tasks carrying
   a tag, and the tasks of a priority (`ls` is an alias of `list`); `tasq list --status waiting --tag support --text export`
   combines filters.

3. Create and edit tasks:

   ```sh
   tasq create "Fix the build" --prio A --due tomorrow --tag ci --project ~/code/app
   tasq set 12 in-progress          # a status of the workflow, or A/B/C for priority
   tasq log 12 "found the cause"    # dated progress note
   tasq view 12                     # rendered with glow on a terminal, plain markdown otherwise
   tasq done 12 "merged"            # status: done, closed time and final note written
   ```

   Ids are the numbers in brackets in the list. A task keeps its id for life.

4. Work on a task:

   ```sh
   tasq next --dry-run              # what would happen: directory, commands, prompt
   tasq next                        # first in-progress task, else first ready one
   tasq pick 12 --launcher shell    # a specific task, in a plain shell
   ```

   The task is set to in-progress and a session opens in the task's project, else
   `work.default_project`. The default launcher is Claude Code with a prompt built from the
   task; `shell`, `tmux`, `herdr` and `auto` are the others (`launch.default`). With
   worktrees enabled (`[worktree] enabled = true`), the session opens in the task's worktree
   instead, creating it first when it does not exist; `tasq create ... --worktree [BRANCH]` and
   `tasq worktree 12 [BRANCH]` declare one, `tasq worktree up 12` creates it without launching.

5. Pull in work from outside: declare `[forge.<name>]` and `[[source]]` blocks (see
   [`docs/sources.md`](docs/sources.md) and [`examples/sources/`](examples/sources/)), then

   ```sh
   tasq sync --dry-run
   tasq sync
   tasq sync --interactive    # the morning briefing: a Claude session running /tasq:sync
   ```

6. Yesterday's standup, from the progress notes:

   ```sh
   tasq summary                     # last working day, distilled by `claude -p`
   tasq summary --raw               # the notes themselves, no LLM
   tasq summary last friday
   ```

7. The same tasks full screen:

   ```sh
   tasq ui
   ```

   `j`/`k` move, `Ctrl+K`/`Ctrl+J` move the selected task up or down within its status group
   (`tasq move`), `z` or `Space` folds the group to its header and back, `/` filters, `c` creates a task in the edit view, `t`/`p` set status and
   priority, `e` opens the edit view (every field and the description, `Enter` on the due
   date opens a calendar, `Ctrl+S` saves), `l`
   logs a note, `d` marks done, `E` opens the file in `$EDITOR`, `v` shows it rendered
   (`tasq view`), `J`/`K` scroll the detail (`Tab` or `Right` shows it), `Enter` starts
   a session here, `Ctrl+Enter` starts it in a new herdr or tmux window and switches to it,
   `Shift+Enter` does the same without leaving the list, `s` runs the sources that run by
   default, `S` picks which sources to run, `?` lists every key. Every key can be rebound under `[ui.keys]` in the config: Ghostty on Linux keeps
   `Ctrl+Enter` for fullscreen, so `launch-detached = "alt+enter"` moves the action there.

## Concepts

**A task is one markdown file.** `<ULID>.md` in the store directory, by default
`~/.local/share/tasq/stores/default` (`data_dir` and `store.name` choose another). The file
starts with a frontmatter block, then the body:

```
---
id: 12
status: ready
priority: A
due: 2026-10-10
tags: [gitlab]
---
# Fix the build

## Progress

- 2026-10-04 10:15: created via tasq create
```

The body has `## Description`, `## Related` (with `### Merge requests`), `## Progress`,
`## Worktrees` and `## Sessions`. `tasq` edits only the lines it changes and keeps everything
else byte for byte: unknown keys, their order, unknown sections, blank lines, line endings.
A done task says `status: done`. Normative reference, including the exact frontmatter subset:
[`docs/file-format.md`](docs/file-format.md).

**Status, priority and tags are fields.** `tags` holds topic tags only, `status` is one of the
workflow's statuses (`in-progress`, `ready`, `waiting`, `blocked`, `later` by default;
configurable with `[workflow]`) or none, `priority` is always set (`A`, `B` or `C`, `B` when
absent).

**Ids are stable.** A task's id is the `id:` in its file: the highest id in the store plus one
when it is created, never changed afterwards. The file name is a ULID whose timestamp is the
creation time (`created` in `tasq view --json`), so two creates in the same second, or in
parallel, both succeed, and `ls` sorts the store chronologically.

**Files, a lock, no index and no git.** Reads never spawn a process and never lock: every
command reads the frontmatter of the store's files. A write takes `.tasq/lock` in the store
directory (a crashed holder's lock is broken after two minutes), rewrites one file atomically,
and refuses with a conflict when the file changed since it was read. Nothing is committed;
syncing a store with git comes back as an opt-in feature
([ADR-0026](docs/adr/0026-store-v2-frontmatter-tasq-directory-ulid-files.md)).

**Layered configuration.** TOML, later layers winning key by key: built-in defaults,
`~/.config/tasq/config.toml`, the nearest `.tasq.toml` walking up from the current directory,
a `[profile.<name>]` selected with `--profile`, `TASQ_*` environment variables, `--set
key=value`. Unknown keys are errors with file, line and column. `tasq config show` prints the
effective config and which layer set each key. Reference: [`docs/config.md`](docs/config.md).

**Sources** turn things outside the store into tasks: merge requests waiting for your
review, issues assigned to you (GitLab and GitHub), and whatever a command prints as JSON
(`llm-bridge`, typically `claude -p` over Slack or Gmail). `tasq sync` creates tasks for new
items, marks them done when the item is merged, closed, approved or reassigned, and re-checks
tracked tasks the sweep no longer lists. Each task remembers its origin in its `source`,
`source_id` and `source_url` keys.
[`docs/sources.md`](docs/sources.md).

**Launchers** open a work session on a task: `claude` (Claude Code with a prompt from a
template, through `direnv exec` when the directory has an allowed `.envrc`), `shell`, `tmux`
(new window), `herdr` (workspace with a Claude agent) and `auto` (herdr inside herdr, else
claude). The session gets `TASQ_TASK_ID`, `TASQ_DATA_DIR` and `TASQ_STORE_NAME` in its
environment.

**Plugins and hooks.** `tasq <name> [args...]` runs an executable `tasq-<name>` found on
`PATH` when `<name>` is not a built-in command, passing the remaining arguments verbatim and
`TASQ_BIN` (plus `TASQ_PROFILE`, `TASQ_CONFIG`, `TASQ_SET` when in effect) so the plugin can
call back into `tasq --json` and `tasq apply` with the same configuration. Built-ins and
their aliases always win. `[hooks]` runs command lines after `create`
and `done` and before a session starts, with the task as JSON on stdin. `tasq plugins list`
shows what was found. [`docs/plugins.md`](docs/plugins.md); reference plugin under
[`examples/plugins/`](examples/plugins/).

**Claude Code plugin.** `plugins/claude` ships two skills for sessions started by `tasq next`:
`/tasq:wrapup` records progress, merge requests, worktrees and the final status on the task;
`/tasq:sync` runs the sources and briefs you. `claude --plugin-dir plugins/claude` loads it for
one session; the repository is also a one-plugin marketplace. A status-line snippet shows the
current task. [`plugins/claude/README.md`](plugins/claude/README.md).

## Commands

| Command | Purpose |
|---|---|
| `tasq`, `tasq list [WORD] [--status S] [--tag T]... [--prio P] [--text TEXT] [--all \| --done]` | Open tasks grouped by status (`ls` is an alias of `list`); `WORD` is a status, a tag or a priority; `--all` adds a DONE group, `--done` shows only closed tasks |
| `tasq create TITLE [--desc] [--status] [--prio] [--due] [--project] [--tag]... [--related]... [--mr]... [--note]` | Create a task |
| `tasq set ID VALUE [NOTE]` | Set the status or the priority, optionally logging a note; a value the task already has writes nothing |
| `tasq log ID NOTE` | Append a dated progress note |
| `tasq done ID [NOTE]` | Mark done (`status: done` and the closing time), optional final note; a done task is refused |
| `tasq rename ID TITLE [--note TEXT]` | Replace the title |
| `tasq due ID DATE\|none [--note TEXT]` | Set the due date (`create --due` words) or remove it |
| `tasq tag ID TAG... [--note TEXT]` | `+name` or `name` adds a topic tag, `-name` removes one; the other tags stay |
| `tasq desc ID [TEXT\|-\|--edit] [--note TEXT]` | Print the description, replace it (`-` reads stdin, `""` clears) or open it in `$VISUAL`/`$EDITOR` |
| `tasq move ID --before ID\|--after ID\|--top\|--unrank` | Place the task next to another task of its status group, first in the group (above every priority), or drop its rank back to the priority, due, id order; lists, the UI and `next` follow |
| `tasq next [--launcher NAME] [--dry-run]` | Open a session on the first in-progress task, else the first ready one |
| `tasq pick ID [--launcher NAME] [--dry-run]` | Open a session on a specific task |
| `tasq view ID [--raw]` | Show a task (glow on a terminal, plain markdown otherwise; `--raw` is verbatim) |
| `tasq project ID [PATH]` | Show or set the task's project directory |
| `tasq worktree set ID [BRANCH]` / `tasq worktree set ID PATH` | Declare a worktree for the task (no git, nothing on disk), or track an existing one (with `worktree.enabled`) |
| `tasq session ID SESSION_ID [DESC] [--launcher NAME]` | Track an agent session |
| `tasq mr ID URL [TITLE]` | Track a merge request |
| `tasq sync [--source NAME] [--dry-run] [ID]...` | Refresh tasks from the configured sources, or re-check the given tasks |
| `tasq sync --interactive [--dry-run]` | Open the Claude Code briefing session (`/tasq:sync`) in `work.default_project` |
| `tasq apply [FILE]` | Update a task from `{"schema":1,"task":{...}}` on stdin or in a file |
| `tasq summary [DAY] [--raw]` | Standup summary of a day's progress notes |
| `tasq dates [SPEC]...` | Resolve `today`, `last week`, `last 7 days`, ... to `FROM TO` |
| `tasq ui` | Full-screen terminal UI |
| `tasq store info` | Store directory, task count, how ids behave |
| `tasq doctor` | Check config, the store and optional tools |
| `tasq config show` | Effective configuration with the layer that set each key |
| `tasq plugins list` | Discovered `tasq-*` plugins and configured hooks |
| `tasq completions SHELL` | Completion script for bash, zsh, fish and others |
| `tasq NAME [ARGS]...` | Run the plugin `tasq-NAME` from `PATH` |

Global flags, accepted before or after the subcommand: `--profile NAME`, `--config FILE`,
`--set KEY=VALUE` (repeatable), `--json`, `--color auto|always|never`, `--no-color`,
`--no-pager`, `-v`/`-vv`. `tasq --help` and `tasq <command> --help` carry the details.

## Exit codes and `--json`

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | An error you can fix, printed as `tasq: <message>` on stderr; also `doctor` with a failed check and `sync` with a failed source |
| 2 | Usage error (clap prints the usage) or an internal failure (`tasq: internal error: ...`) |

Every command accepts `--json` and prints exactly one object carrying `"schema": 1`.
Payloads per command and the `Task` object: [`docs/json.md`](docs/json.md). Human output is
colour-free when stdout is not a terminal or `NO_COLOR` is set; the pager (`ui.pager`) is used
only on a terminal.

## Configuration

Reference with every key and its default: [`docs/config.md`](docs/config.md). Complete
examples: [`examples/config/plain-markdown.toml`](examples/config/plain-markdown.toml) for a
plain setup, [`examples/config/author.toml`](examples/config/author.toml) for the full
GitLab, worktree, launcher and hook setup.

## Status

Pre-release, version 0.1.0. The only store is the directory of task files described above;
SQLite or other stores are behind the `Store` trait but not implemented. Linux and
macOS; Windows should compile but is untested and unsupported (`exec`, direnv and tmux
semantics). The plan and the decisions behind the design are in
[`ruli/features/rust-rewrite/PLAN.md`](ruli/features/rust-rewrite/PLAN.md) and
[`docs/adr/`](docs/adr/README.md).

## Development

A stable Rust toolchain (`rust-toolchain.toml`; MSRV 1.90) and
[`just`](https://github.com/casey/just). `just check` runs formatting, clippy with warnings
denied and the tests; `just --list` shows the rest. Every recipe runs cargo through
`scripts/guard`, a memory-capped systemd scope, and only one build should run at a time on a
machine: the reasons and the rules are in [`CONTRIBUTING.md`](CONTRIBUTING.md). The crates,
what each owns and how they depend on each other: [`docs/architecture.md`](docs/architecture.md).
Test conventions, the fixture store and mutation testing: [`docs/testing.md`](docs/testing.md).

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
