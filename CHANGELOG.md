# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `tasq` CLI: `list` (default, with status/tag/priority/text filters), `store info`,
  `store sync`, `doctor`, `config show` and `completions`; `--json` on every command,
  `--color`/`--no-color`/`--no-pager`, `--profile`/`--config`/`--set`.
- nb-compatible store (`tasq-store-nb`) with nb and native bookkeepers.
- Core domain model, lossless markdown format, queries and layered TOML config.
- Cargo workspace with the `tasq-core`, `tasq-store-nb`, `tasq-sources`,
  `tasq-launch`, `tasq` and `tasq-tui` crates, shared lints and quality gates.
