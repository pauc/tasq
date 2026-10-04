# ADR-0005: CLI first, TUI second

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

The rewrite has several deliverables: a core library, a CLI with parity to the
script, external source and launcher adapters, a ratatui TUI, a Claude Code
plugin and a plugin mechanism. The order they are built in affects how clean
the core/UI boundary ends up and how early the author can start using the tool.
The question asked during planning was which order yields the cleanest, most
maintainable code.

## Decision

Phases are delivered in this order. Phases are sequential; tasks inside a phase
can be parallelised.

1. **Core library first**: model, markdown format, `Store` trait, nb store,
   fully tested with fixtures. Correctness lives here, and a bad model would be
   the most expensive thing to fix later.
2. **CLI as the first consumer**, reaching parity with the script
   (`list/create/set/log/done/view/project/worktree/session/mr/next/pick/sync/summary`),
   every command with `--json`. Writing the CLI against the library immediately
   exposes leaks in the core API, and parity means the author can dogfood early.
3. **Sources and launchers** as adapters, each landing with tests (HTTP mocks,
   fake launchers).
4. **TUI as the second consumer.** A second consumer is the best test of the
   boundary; building it last means it never drives the model. The TUI depends
   on `tasq-core` only and performs every edit through the same core functions
   as the CLI (FR-10).
5. **Plugin mechanism** decided once we know which commands plugins actually
   need (ADR-0006, T-901).

## Consequences

### Positive

- Early dogfooding: a usable `tasq` exists after phase 2, well before any UI
  work.
- The core API is shaped by a real consumer before a second one arrives, so
  the TUI finds a stable library rather than co-evolving with it.
- `--json` on every CLI command from the start gives scripts and the later
  plugin mechanism a surface to target.
- Adapter tests (phase 3) land against an API that already has a user.

### Negative

- The TUI, which motivated choosing ratatui, arrives late. Accepted: the CLI
  covers every workflow in the meantime.
- Some core design (for example rendering colours shared by CLI and TUI) has to
  anticipate the TUI without it existing yet. Mitigated by keeping the renderer
  a separate module.
- Deferring the plugin decision means early adapters are built in-process by
  default; ADR-0006 explains how the architecture keeps the alternative open.

## Alternatives considered

- **TUI first** — attractive as the visible deliverable, but the UI would drive
  the model and the author could not replace the script until much later.
- **CLI and TUI in parallel** — two consumers shaping the core at once, with
  more coordination and a less settled API for both.
- **Big-bang parity including sources and launchers before any UI** — delays
  the first usable binary without improving the boundary.

## References

- Plan section 3 and section 13 ("Delivery: core library → CLI parity →
  sources/launchers → TUI → plugins").
- ADR-0003, ADR-0006.
