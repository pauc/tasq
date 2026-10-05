# tasq

<!-- TODO: replace OWNER/tasq with the real GitHub repository once it exists. -->
[![CI](https://github.com/OWNER/tasq/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/OWNER/tasq/actions/workflows/ci.yml)
[![Mutants (nightly)](https://github.com/OWNER/tasq/actions/workflows/mutants-nightly.yml/badge.svg)](https://github.com/OWNER/tasq/actions/workflows/mutants-nightly.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

Terminal task manager over [nb](https://github.com/xwmx/nb) markdown todos.

Tasks are the `*.todo.md` files of an nb notebook. `tasq` reads and writes them directly,
keeps every byte it does not understand, and leaves index registration and git commits to nb
when nb is installed. On top of the files it adds a status workflow, priorities, due dates,
progress notes, a standup summary, work sessions (Claude Code, tmux, herdr, a shell) started
in the task's worktree, external sources (GitLab and GitHub review requests and issues, an LLM
inbox) that create and close tasks, a full-screen terminal UI, `--json` on every command, and
out-of-process plugins. nb itself keeps working on the same notebook.

![tasq listing tasks, setting a status and printing a standup summary, then tasq ui with the status picker and the help overlay](docs/demo/tasq.gif)

The recording is [`docs/demo/demo.tape`](docs/demo/demo.tape) against a throwaway notebook;
`scripts/demo-gif` re-renders it with [VHS](https://github.com/charmbracelet/vhs) in docker.

## Install

| Method | Command |
|---|---|
| crates.io (once published) | `cargo install tasq` |
| Prebuilt binary | Download `tasq-vX.Y.Z-<target>.tar.gz` for your platform from the [GitHub releases](https://github.com/OWNER/tasq/releases) (Linux x86_64 and aarch64, macOS x86_64 and arm64; a `.sha256` sits next to each tarball), unpack it and put `tasq` on your `PATH` |
| Homebrew | `brew install OWNER/tasq/tasq` (the tap is `OWNER/homebrew-tasq`) <!-- TODO: OWNER --> |
| From source | `git clone https://github.com/OWNER/tasq && cd tasq && cargo install --path crates/cli` |

The binary is called `tasq`. Rust 1.90 or newer builds it from source. Nothing else is
required: nb, git, glow, claude, direnv, tmux, herdr, glab and gh are all optional and
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
   config files, the notebook and its `.index`, nb, git identity and the optional tools.
   Exit code 1 when a check failed.

2. List the open tasks of the `home` notebook, grouped by status:

   ```sh
   tasq
   ```

   The notebook is nb's `home` (`~/.nb/home`, or `$NB_DIR/home`). Another notebook, for one
   command or for the shell session:

   ```sh
   tasq --set store.notebook=work
   TASQ_NOTEBOOK=work tasq
   ```

   Permanently, in `~/.config/tasq/config.toml`:

   ```toml
   [store]
   notebook = "work"
   ```

   `tasq ready`, `tasq gitlab` and `tasq A` print one status group, the tasks carrying a tag,
   and the tasks of a priority; `tasq list --status waiting --tag support --text export`
   combines filters.

3. Create and edit tasks:

   ```sh
   tasq create "Fix the build" --prio A --due tomorrow --tag ci --project ~/code/app
   tasq set 12 in-progress          # a status of the workflow, or A/B/C for priority
   tasq log 12 "found the cause"    # dated progress note
   tasq view 12                     # rendered with glow on a terminal, plain markdown otherwise
   tasq done 12 "merged"            # # [x], status tag removed, final note logged
   ```

   Ids are the numbers in brackets in the list, the same ids nb shows.

4. Work on a task:

   ```sh
   tasq next --dry-run              # what would happen: directory, commands, prompt
   tasq next                        # first in-progress task, else first ready one
   tasq pick 12 --launcher shell    # a specific task, in a plain shell
   ```

   The task is set to in-progress and a session opens in the first tracked worktree that
   exists, else the task's project, else `work.default_project`. The default launcher is
   Claude Code with a prompt built from the task; `shell`, `tmux`, `herdr` and `auto` are the
   others (`launch.default`). `tasq worktree 12 --create my-branch` makes and tracks a git
   worktree first.

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

   `j`/`k` move, `/` filters, `c` creates a task from a title, `t`/`p` set status and
   priority, `e` opens the edit view (every field and the description, `Ctrl+S` saves), `l`
   logs a note, `d` marks done, `E` opens the file in `$EDITOR`, `Enter` starts
   a session here, `Ctrl+Enter` starts it in a new herdr or tmux window and switches to it,
   `Shift+Enter` does the same without leaving the list, `s` runs the sources that run by
   default, `S` picks which sources to run, `?` lists every key. Every key can be rebound under `[ui.keys]` in the config: Ghostty on Linux keeps
   `Ctrl+Enter` for fullscreen, so `launch-detached = "alt+enter"` moves the action there.

## Concepts

**A task is one nb todo file.** `<stamp>.todo.md` in the notebook directory, starting with
`# [ ] Title` (`# [x]` when done) and `## Section` headings: Description, Project, Due,
Related (with `### Merge requests`), Tags, Progress, Worktrees, Sessions. The format is the
one the original `tasks` script wrote and nb reads; `tasq` edits only the section it changes
and keeps everything else byte for byte. Normative reference:
[`docs/file-format.md`](docs/file-format.md).

**Status and priority are tags in the file, fields in the model.** The `## Tags` line holds
`#gitlab #A #ready`: topic tags, the priority (`A`, `B` or `C`, `B` when absent) and the
status. In `tasq` they are three different things: `tags` holds topic tags only, `status` is
one of the workflow's statuses (`in-progress`, `ready`, `waiting`, `blocked`, `later` by
default; configurable with `[workflow]`) or none, `priority` is always set. The format layer
maps between the two; nothing else asks whether a tag is a status.

**Ids are nb's.** A task's id is the line number of its file in the notebook's `.index`, so
`tasq` and `nb` show the same numbers. Ids are positional: deleting a file or running
`nb index reconcile` can renumber them. `tasq store info` says so for the store in use, and
`tasq doctor` warns when the index is inconsistent.

**Hybrid nb bookkeeping.** Reads never spawn a process: `.index` and the markdown files are
parsed directly. Writes are atomic rewrites of one file. Afterwards a bookkeeper registers new
files in `.index` and commits: nb itself (`nb index add`, `nb git checkpoint`) when it is on
`PATH`, so the index and git semantics are nb's; a native fallback (append to `.index`, run
`git`) otherwise, which is what makes `tasq` usable without nb. `store.bookkeeper` chooses
(`auto`, `nb`, `native`). The index is only ever appended to, never rebuilt. A bookkeeping
failure after a successful write is a warning with the manual fix, never data loss.

**Layered configuration.** TOML, later layers winning key by key: built-in defaults,
`~/.config/tasq/config.toml`, the nearest `.tasq.toml` walking up from the current directory,
a `[profile.<name>]` selected with `--profile`, `TASQ_*` environment variables, `--set
key=value`. Unknown keys are errors with file, line and column. `tasq config show` prints the
effective config and which layer set each key. Reference: [`docs/config.md`](docs/config.md).

**Sources** turn things outside the notebook into tasks: merge requests waiting for your
review, issues assigned to you (GitLab and GitHub), and whatever a command prints as JSON
(`llm-bridge`, typically `claude -p` over Slack or Gmail). `tasq sync` creates tasks for new
items, marks them done when the item is merged, closed, approved or reassigned, and re-checks
tracked tasks the sweep no longer lists. Each task remembers its origin in a `## Source` line.
[`docs/sources.md`](docs/sources.md).

**Launchers** open a work session on a task: `claude` (Claude Code with a prompt from a
template, through `direnv exec` when the directory has an allowed `.envrc`), `shell`, `tmux`
(new window), `herdr` (workspace with a Claude agent) and `auto` (herdr inside herdr, else
claude). The session gets `TASQ_TASK_ID` and `TASQ_NOTEBOOK` in its environment.

**Plugins and hooks.** `tasq <name> [args...]` runs an executable `tasq-<name>` found on
`PATH` when `<name>` is not a built-in command, passing the remaining arguments verbatim and
`TASQ_BIN` (plus `TASQ_PROFILE`, `TASQ_CONFIG`, `TASQ_SET` when in effect) so the plugin can
call back into `tasq --json` and `tasq apply` with the same configuration. Built-ins always
win; a plugin wins over the bare `tasq <word>` filter, which stays available as
`tasq list <word>`. `[hooks]` runs command lines after `create`
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
| `tasq [WORD]`, `tasq list [WORD] [--status S] [--tag T]... [--prio P] [--text TEXT] [--all \| --done]` | Open tasks grouped by status; `WORD` is a status, a tag or a priority; `--all` adds a DONE group, `--done` shows only closed tasks |
| `tasq create TITLE [--desc] [--status] [--prio] [--due] [--project] [--tag]... [--related]... [--mr]... [--note]` | Create a task |
| `tasq set ID VALUE [NOTE]` | Set the status or the priority, optionally logging a note |
| `tasq log ID NOTE` | Append a dated progress note |
| `tasq done ID [NOTE]` | Mark done, status tag removed, optional final note |
| `tasq next [--launcher NAME] [--dry-run]` | Open a session on the first in-progress task, else the first ready one |
| `tasq pick ID [--launcher NAME] [--dry-run]` | Open a session on a specific task |
| `tasq view ID [--raw]` | Show a task (glow on a terminal, plain markdown otherwise; `--raw` is verbatim) |
| `tasq project ID [PATH]` | Show or set the task's project directory |
| `tasq worktree ID PATH` / `tasq worktree ID --create BRANCH` | Track an existing git worktree, or create one and track it |
| `tasq session ID SESSION_ID [DESC] [--launcher NAME]` | Track an agent session |
| `tasq mr ID URL [TITLE]` | Track a merge request |
| `tasq sync [--source NAME] [--dry-run] [ID]...` | Refresh tasks from the configured sources, or re-check the given tasks |
| `tasq sync --interactive [--dry-run]` | Open the Claude Code briefing session (`/tasq:sync`) in `work.default_project` |
| `tasq apply [FILE]` | Update a task from `{"schema":1,"task":{...}}` on stdin or in a file |
| `tasq summary [DAY] [--raw]` | Standup summary of a day's progress notes |
| `tasq dates [SPEC]...` | Resolve `today`, `last week`, `last 7 days`, ... to `FROM TO` |
| `tasq ui` | Full-screen terminal UI |
| `tasq store info` | Notebook path, task count, how ids behave |
| `tasq store sync` | `nb sync`, or `git pull --rebase && git push` without nb |
| `tasq doctor` | Check config, notebook, nb and optional tools |
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
setup without nb, [`examples/config/author.toml`](examples/config/author.toml) for the full
GitLab, worktree, launcher and hook setup.

## Status

Pre-release, version 0.1.0. The only store is the nb-compatible notebook (markdown files plus
`.index`); SQLite or other stores are behind the `Store` trait but not implemented. Linux and
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
Test conventions, the nb fixture notebook and mutation testing: [`docs/testing.md`](docs/testing.md).

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE). nb is AGPLv3 and is run as a separate program,
never linked.
