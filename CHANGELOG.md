# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
