# ADR-0006: Plugin mechanism

- **Status:** Proposed (to be decided in T-901, after the CLI and TUI exist)
- **Date:** 2026-10-04

## Context

The tool must be extensible by people other than the author: new stores,
sources, launchers, reports and personal workflows (the author's time-log
command `tlogs` is explicitly out of the main repo and becomes the reference
external plugin). Deciding the mechanism now, before any command exists, risks
designing for imagined needs. The plan defers the decision to T-901 and asks the
architecture to keep every option open in the meantime.

## Options

**A. External executables.** `tasq-<name>` programs discovered on `PATH`.
`tasq <unknown>` dispatches to `tasq-<unknown>` with `TASQ_PROFILE` and
`TASQ_CONFIG` set. Plugins read state with `tasq ... --json`, write it with
`tasq apply < task.json`, and receive hooks (`post-create`, `post-done`,
`pre-launch`). Any language. Process boundary; no sandbox beyond the OS.

**B. In-process Rust traits behind cargo features.** Implementations of
`Store`, `Source`, `Launcher` compiled into the binary and enabled with
features (as `herdr` already is). Fastest, type-checked, no serialisation.
Requires Rust, a rebuild, and GPL-compatible licensing (ADR-0008).

**C. WASM via extism.** Plugins compiled to WebAssembly and loaded at runtime
through extism's host SDK. Sandboxed and language-agnostic, but adds a runtime
dependency, a host-function API to design and maintain, and limits on what a
plugin can touch (processes, terminal) without explicit host functions.

## What the architecture does to keep all three open

- **Library-first core.** Everything the CLI does is a `tasq-core` function;
  the CLI is thin. In-process plugins (B) and WASM hosts (C) target the library.
- **`--json` everywhere.** Every CLI command emits JSON with a versioned schema
  (`"schema": 1`, documented in `docs/json.md`).
- **`tasq apply`.** Task edits are accepted as JSON on stdin, the stdin side of
  the out-of-process surface (A).
- **Registry.** Adapters are registered through a `Registry` built at startup
  from config, so registration can later be fed by PATH discovery or a WASM
  loader without changing the traits.

## License tilt

The project is GPL-3.0-or-later (ADR-0008). An executable that talks JSON to
`tasq` over pipes is a separate program, not a derived work, and may carry any
license. A Rust crate linked into the binary is part of the combined work and
must be GPL-compatible. This tilts **third-party** plugins toward option A
regardless of technical merit; option B remains natural for adapters maintained
in this repository.

## Recommendation to validate in T-901

**A for user plugins, B for built-in adapters.** Revisit C only if sandboxing
untrusted plugins becomes a requirement. The evaluation in T-901 should:

- list the commands and hooks real plugins needed during the CLI/TUI phases;
- implement `tasq-tlogs` (the author's time-log tool) as the reference external
  plugin against option A and record what the JSON surface lacked;
- measure whether any hot path (listing, TUI redraw) would need an in-process
  plugin to meet the performance targets.

When accepted, this ADR moves to **Accepted** with the evaluation, and
`docs/plugins.md` documents the hook points and a worked example.

## Consequences (of deferring)

### Positive

- The decision is made with evidence from a working CLI and TUI.
- No plugin API has to be kept stable before v1.

### Negative

- Early adapters are built in-process; if A wins, nothing is lost, but if a
  built-in adapter later wants to move out of process, its JSON contract is new
  work.
- Until T-901, `tasq <unknown>` is simply an error.

## References

- Plan sections 2, 4.3 and task T-901.
- ADR-0003 (traits and Registry), ADR-0008 (license).
