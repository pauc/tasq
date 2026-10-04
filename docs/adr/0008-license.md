# ADR-0008: GPL-3.0-or-later license

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

The project is meant to be published and extended by others. It works on
notebooks maintained by nb, which is licensed under the AGPLv3, and it will have
a plugin mechanism (ADR-0006) whose shape interacts with the license. The
license must be chosen before the first public commit so every crate and
contribution is covered.

## Decision

The whole workspace is licensed **GPL-3.0-or-later**. `LICENSE` holds the GPLv3
text and every crate's `Cargo.toml` sets `license = "GPL-3.0-or-later"`.

### Why not AGPL

The AGPL adds one clause to the GPL: users who interact with a modified version
over a network must be offered its source. `tasq` is a local CLI and TUI; it is
not a network service, so the clause would never apply. Plain GPLv3 is the
conventional equivalent for a command-line tool and avoids the extra friction
AGPL causes for packagers and corporate users.

### Relation to nb's AGPLv3

nb is invoked only as a separate program through its command line (`nb index
add`, `nb git checkpoint`, `nb index verify`, `nb notebooks show`, see
ADR-0007). It is never linked, vendored or copied into this repository.
Running an AGPL program as a subprocess does not make the caller a derived
work, so this project's license is independent of nb's. The `Native`
bookkeeper reproduces nb's filename and index *behaviour* from observation and
tests, not nb's code.

### Consequences for plugins

- **In-process plugins** (Rust crates compiled into the binary, WASM modules
  loaded into the process) are combined with GPL code and must be distributed
  under a GPL-compatible license.
- **Out-of-process plugins** (`tasq-<name>` executables talking JSON over
  pipes) are separate programs and are unaffected: they may be proprietary or
  under any license.

This is why ADR-0006 tilts third-party plugins toward the out-of-process model.

### Dependency compatibility

Our dependencies are MIT, Apache-2.0 or BSD licensed. Those licenses are
one-way compatible with the GPL: GPL code may link to them, while they cannot
absorb GPL code. `cargo-deny` is configured to allow exactly that set and to
fail on licenses that are not GPL-compatible.

## Consequences

### Positive

- Contributions and forks stay open; improvements to the tool come back under
  the same terms.
- Clear rule for plugin authors: pipe JSON and you are free; link and you are
  GPL.
- "or later" lets the project move to a future GPL version without relicensing.

### Negative

- Some companies forbid GPL tooling; a permissive license would reach more
  users. Accepted as the cost of copyleft; the out-of-process plugin path
  removes the concern for integrations.
- Every new dependency must be checked for GPL compatibility (enforced by
  `cargo-deny` in CI).
- `tasq-core` cannot be used as a library by non-GPL programs. If that becomes
  a need, a separate relicensing decision would be required.

## Alternatives considered

- **AGPL-3.0-or-later** (match nb) — no benefit for a local tool; adds friction.
- **MIT or Apache-2.0** — maximum reach, but no copyleft; the planning
  conversation settled on a GPL-family license.
- **Dual license core (MIT) / GPL binary** — more flexible for library users
  but complicates contributions and the plugin story; not needed in v1.

## References

- Plan tasks T-001 (LICENSE, `cargo-deny`), section 9, section 13.
- ADR-0006, ADR-0007.
