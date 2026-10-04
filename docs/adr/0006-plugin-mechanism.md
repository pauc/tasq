# ADR-0006: Plugin mechanism

- **Status:** Accepted (decided in T-901, 2026-10-04, after the CLI and TUI existed)
- **Date:** 2026-10-04 (proposed and accepted the same day; the proposal is kept below)

## Context

The tool must be extensible by people other than the author: new stores,
sources, launchers, reports and personal workflows (the author's time-log
command `tlogs` is explicitly out of the main repo and becomes the reference
external plugin). Deciding the mechanism before any command existed risked
designing for imagined needs, so the plan deferred the decision to T-901 and
asked the architecture to keep every option open in the meantime.

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

## Decision

**A for user plugins, B for built-in adapters. C is not adopted.**

- `tasq <name> [args...]` runs the executable `tasq-<name>` found on `PATH`
  when `<name>` is not a built-in command, with the remaining arguments
  verbatim. Built-in commands always win; a plugin wins over the bare
  `tasq <word>` filter view, which stays reachable as `tasq list <word>`.
  The plugin inherits the environment plus `TASQ_BIN` (the running binary),
  and `TASQ_PROFILE`, `TASQ_CONFIG` and `TASQ_SET` when a profile, a config
  file or `--set` overrides are in effect, so `$TASQ_BIN ... --json` inside
  the plugin sees exactly the configuration of the `tasq` that ran it.
- Hooks are command lines under `[hooks]` in the configuration:
  `post-create`, `post-done` and `pre-launch`. Each gets
  `{"schema": 1, "hook": <name>, "task": {...}}` on stdin (`pre-launch` adds
  `workdir`, `in_worktree`, `launcher`) and `TASQ_HOOK`, `TASQ_TASK_ID`,
  `TASQ_BIN` in its environment. A failing `post-*` hook is a warning (its
  event already happened); a failing `pre-launch` hook aborts the launch.
  `--dry-run` lists the hooks and runs none.
- `tasq plugins list` shows the discovered executables and the configured
  hooks, with `--json`.
- Stores, sources and launchers maintained in this repository stay
  in-process Rust (`tasq-store-nb`, `tasq-sources`, `tasq-launch`), gated by
  cargo features where optional (`herdr`). Nothing in the plugin surface
  depends on them: `tasq_cli::plugins` is built on the `--json` documents,
  `apply`, and process spawning only.

## Evaluation (T-901)

**Commands real plugins and skills needed** during the CLI and TUI phases:
`list/view/summary/dates/config show --json`, `create`, `set`, `log`, `done`,
`mr`, `session`, `worktree`, `apply`. The Claude Code plugin (T-701) and the
TUI's host actions (ADR-0009) already drive the tool exactly this way, so the
out-of-process surface had been exercised by two consumers before this
decision.

**Reference external plugin.** `examples/plugins/tasq-tlogs` (bash + jq)
builds a per-day time-log proposal from `tasq dates --json` and
`tasq summary --json --raw <day>`, and is run end to end by a CLI integration
test through the dispatcher. What the JSON surface lacked, and what was done:

| Gap | Resolution |
|---|---|
| No way for a plugin to call back into the same binary | `TASQ_BIN` |
| `--set` overrides had no environment form, so a plugin's `tasq` calls lost them | `TASQ_SET` (newline-separated `key=value`, same layer as `TASQ_*`, below `--set`) |
| No event surface at all | `[hooks]` with three events and a stdin document |
| `tasq list --json` never includes done tasks, so a plugin cannot list closed work except per day through `summary` | Recorded as a follow-up (`list --all`); `tlogs` only needs `summary` |

**Hot paths.** Listing a generated 500-task notebook with a debug build takes
about 15 ms (30 ms with `--json`), under the plan's 50 ms target without any
plugin in the path; the TUI reads through `tasq-core` directly. No hot path
needs an in-process plugin, which is what would have argued for B or C for
user code.

**Why not C.** Nothing in the evaluation needed a sandbox: every plugin so
far is the user's own script running with the user's own rights, and the
things plugins want (processes, the terminal, the network) are exactly what a
WASM host would have to re-expose by hand.

## Consequences

### Positive

- Plugins can be written in any language and carry any license; an
  executable talking JSON over pipes is not a derived work of a GPL program
  (ADR-0008), while the in-process adapters in this repository stay GPL.
- The versioned `--json` documents (`docs/json.md`) are the plugin API; there
  is no second API to keep stable.
- Discovery is `PATH`, which users already know how to manage.

### Negative

- A plugin pays a process start and a config load per `tasq` call; fine for
  reports and hooks, not for anything per-keystroke. If that is ever needed,
  option B is still open for a built-in adapter.
- Hooks run in the CLI only. The TUI's `d` key edits through `tasq_core::edit`
  and fires no `post-done`; its `Enter` runs `tasq pick`, so `pre-launch`
  does. Routing TUI edits through hooks would need a `Host` method (ADR-0009).
- `tasq <word>` is ambiguous between a plugin and a filter; the rule (plugin
  wins, `tasq list <word>` always filters) is documented and tested.
- `tasq apply` setting `done` does not fire `post-done`; only `tasq done`
  does.

## Alternatives considered

- **B for user plugins** — requires Rust, a rebuild and GPL-compatible
  licensing of the plugin; the `--json` surface already existed and had two
  consumers.
- **C (WASM)** — not adopted; see the evaluation.

## References

- Plan sections 2, 4.3 and task T-901; ADR-0003 (traits and Registry),
  ADR-0008 (license), ADR-0009 (TUI host actions through the CLI).
- `crates/cli/src/plugins.rs`, `crates/cli/src/commands/plugins.rs`,
  `crates/core/src/config/mod.rs` (`HooksConfig`),
  `crates/core/src/config/load.rs` (`ENV_SET`), `docs/plugins.md`,
  `examples/plugins/`.
