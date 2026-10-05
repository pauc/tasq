# ADR-0014: Choosing which sources a sync runs: `auto`, repeatable `--source`, a picker in the TUI

- **Status:** Accepted
- **Date:** 2026-10-05

## Context

`tasq sync` ran every enabled `[[source]]`, `--source NAME` narrowed it to
one, and the TUI's `S` key was a bare `tasq sync`. That was fine while every
source was a cheap, deterministic forge query. The LLM bridge
(`examples/sources/`, verified headless on 2026-10-05) is neither: one run
is a full Claude Code session, about 90 s and $2.3, and it reads the user's
Slack and Gmail. Nobody wants that on every `tasq sync`, yet `enabled =
false` made the source impossible to run at all (`--source` refused it), so
the only way to run it on purpose was editing the config twice.

The TUI side had the same all-or-nothing shape, and its default keys were
in the way of the layout the author wants: `s` for "sync what runs by
default", `S` for "choose", and `e` free for a future in-TUI edit form
(ADR-0011 deferred that form; `t` and `p` stay as the quick status and
priority pickers).

## Decision

- **`source[].auto`, default `true`.** A bare `tasq sync` runs the enabled
  sources with `auto = true`. `auto = false` means "only when named":
  skipped by a bare sync and by the TUI's sync-all, run by `--source` or the
  TUI's picker. When every enabled source is `auto = false`, a bare sync is
  an error naming the sources, not a silent no-op. `enabled` keeps its
  meaning, "can run at all"; naming a disabled source is still an error.
- **`--source NAME` is repeatable and explicit.** The named sources run in
  config order, `auto` or not; the first unknown or disabled name is the
  error, listing the enabled sources. An explicit list always wins over
  `auto`.
- **The TUI gets a source picker.** `S` opens `Mode::Sources`: one row per
  enabled source (`[x] 1 name  kind`), the `auto` ones checked at start.
  `Space` and the row digit toggle, `Enter` runs the checked ones as
  `Cmd::Sync(names)`, which the CLI host turns into `tasq sync --source
  <name>...`; `Enter` with nothing checked says so and stays open. The
  checked set lives in `Model::checked`, outside the mode, so closing and
  reopening the picker keeps the choice for the session. `s` is the bare
  `tasq sync` (`Cmd::Sync(vec![])`). The TUI learns the sources as
  `SourceChoice { name, kind, auto }` from the CLI (`Model::with_sources`),
  keeping the core-only dependency of ADR-0005.
- **Three default keys move.** `sync` `S` -> `s`, new `sources` on `S`,
  `status` `s` -> `t`, `edit` `e` -> `E`. `[ui.keys]` (ADR-0013) restores
  any old layout in four lines. The picker's toggles (`Space`, digits) are
  not actions, like typing: they cannot be rebound. The status-bar hint
  shows the two sync keys as one entry, `s/S sync`, so the full hint line
  still fits 100 columns.

## Consequences

### Positive

- Expensive sources are configured once and run on purpose, from the shell
  or the TUI, with no config edits in between.
- `tasq sync --source a --source b` replaces N invocations.
- The picker reuses the existing picker rendering and key handling; the
  new state is one `Mode` variant, two `Model` fields and a `Vec<String>`
  on `Cmd::Sync`.

### Negative

- Two booleans per source (`enabled`, `auto`) instead of one; the docs
  state the difference in one sentence each ("can run" vs "runs by
  default").
- Every `Host` implementation takes the source list (three in the
  workspace).
- Users of the previous defaults relearn three keys; the README gif shows
  the old hint line until it is re-recorded.
- `/tasq:sync` (the Claude plugin) must run an `auto = false` bridge by
  name, since a bare `tasq sync` no longer covers the inbox for it.

## Alternatives considered

- **`--skip NAME`** — covers "everything but the bridge" per invocation
  but not "never by accident"; `auto = false` plus repeatable `--source`
  covers both directions with one concept.
- **Keep `enabled = false` as the switch** — it also blocks `--source`,
  so running the bridge meant editing the config before and after.
- **Picker only, on `S`, with `Enter` running the defaults** — one key
  fewer to move, one keystroke more on every ordinary sync; the author
  preferred a one-key sync-all.
- **A `tasq sync --all` for the `auto = false` sources** — a third way to
  say the same thing as naming them; rejected for surface area.
- **Configurable toggle keys in the picker** — the toggles are typing-like
  and tied to the row digits already shown; making them actions would
  grow `[ui.keys]` for no requested use.

## References

- Todo 12 (`sync sources on demand`), todo 13 (edit form on `e`).
- `docs/sources.md` ("Which sources run", "Headless Claude Code"),
  `docs/config.md` ("Key bindings").
- ADR-0005, ADR-0009, ADR-0011, ADR-0013.
