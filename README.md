# tasq

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

## Development

Requires a stable Rust toolchain (see `rust-toolchain.toml`) and
[`just`](https://github.com/casey/just). Run `just check` before committing.

## License

GPL-3.0-or-later. See [`LICENSE`](LICENSE).
