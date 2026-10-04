# tasq

<!-- TODO: replace OWNER/tasq with the real GitHub repository once it exists. -->
[![CI](https://github.com/OWNER/tasq/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/OWNER/tasq/actions/workflows/ci.yml)
[![Mutants (nightly)](https://github.com/OWNER/tasq/actions/workflows/mutants-nightly.yml/badge.svg)](https://github.com/OWNER/tasq/actions/workflows/mutants-nightly.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

A task manager for nb-style markdown todos, rewritten in Rust.

`original/tasks` is a ~1200-line bash script that manages todos stored as nb
markdown files. It mixes three concerns in one file: a task store (nb todo
files with status and priority encoded as tags), a terminal UI (status-grouped
list, glow rendering, OSC 8 links, paging, standup summary), and personal
integrations (Claude Code sessions, herdr workspaces, gwm worktrees, direnv,
GitLab/GitHub refreshes). The rewrite separates these into a core library with
a small, well-tested domain model and three trait-based extension points
(Store, Source, Launcher), consumed by a CLI first and a ratatui TUI second.
The result should be a tool other people can install and extend, while keeping
every workflow the script offers today.

## Status

Pre-alpha. See [`ruli/features/rust-rewrite/PLAN.md`](ruli/features/rust-rewrite/PLAN.md)
for the implementation plan.

## Terminal UI

`tasq ui` shows the same grouped list full screen with the selected task beside it: `j`/`k`
move, `/` filters, `s`/`p` set the status or priority, `l` logs a note, `d` closes the task,
`e` opens the file in your editor, `Enter` starts a work session and `S` runs `tasq sync`.
Every edit is the same operation as the matching CLI command. Colours follow `[ui.colors]`
and `NO_COLOR` (see [`docs/config.md`](docs/config.md)).

## Claude Code plugin

`plugins/claude` is a Claude Code plugin (namespace `tasq`) with two skills: `/tasq:wrapup`
records a session's progress and final status on its task, `/tasq:sync` refreshes the task list
from the configured sources and briefs you. Load it for one session with
`claude --plugin-dir plugins/claude`, or install it: the repository is a one-plugin marketplace
(`claude plugin marketplace add <repo>`, then `claude plugin install tasq@tasq`). See
[`plugins/claude/README.md`](plugins/claude/README.md), which also has a status-line snippet
showing the current task.

## Development

Requires a stable Rust toolchain (see `rust-toolchain.toml`) and
[`just`](https://github.com/casey/just). Run `just check` before committing.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
