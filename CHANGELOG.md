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
- nb-compatible store (`tasq-store-nb`) with nb and native bookkeepers.
- Core domain model, lossless markdown format, queries and layered TOML config.
- Cargo workspace with the `tasq-core`, `tasq-store-nb`, `tasq-sources`,
  `tasq-launch`, `tasq` and `tasq-tui` crates, shared lints and quality gates.
