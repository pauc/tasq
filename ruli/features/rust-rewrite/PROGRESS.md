# Progress: tasq rewrite — Phases 0 and 1

Plan: [PLAN.md](PLAN.md). Started 2026-10-04. Agents work per task; this file is the single
place for status, learnings, blockers and deviations from the plan.

## Status

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| T-001 | Workspace and quality gates | done | commits cc023aa, 01e2ead; edition 2024, MSRV 1.90 |
| T-002 | CI pipeline | done | commit ab28fc1; cargo-deny 0.20.2 passes locally |
| T-003 | ADRs | done | commit f79c210; 8 ADRs + docs/file-format.md draft |
| T-004 | cargo-mutants setup | done | commit 07d3203; cargo-mutants 27.1.0 |
| T-101 | Domain model | done | commit f9e606c; 59 unit tests, clippy clean |
| T-105 | Clock and progress logging | done | commit f9e606c; `When` enum keeps date-only entries lossless |
| T-102 | Markdown parser | in progress | wave 4 (with T-103) |
| T-103 | Markdown writer and round trip | in progress | wave 4 |
| T-104 | Queries and grouping | in progress | wave 4 |
| T-106 | Configuration loading | in progress | wave 4 |

## Wave plan

1. T-001 alone: everything else needs the workspace. Pre-creates the `justfile` and stub
   modules so later parallel agents own disjoint files.
2. T-002, T-003, T-004 in parallel (disjoint files: `.github/`, `docs/adr/`, `.cargo/mutants.toml` + justfile recipes).
3. T-101 + T-105: the model everything in Phase 1 depends on. Declares `format`, `query`, `config` modules as stubs.
4. T-102+T-103 (one agent), T-104, T-106 in parallel, each owning one module directory.

## Learnings

- **`ruli/` is ignored by the user's global git excludes** (`~/.config/git/ignore`). The repo
  `.gitignore` adds `!/ruli/` so the plan and this file are tracked. Anyone cloning is unaffected.
- `rust-toolchain.toml` pinning `stable` made rustup install 1.99.0 on first use (1.98.1 was the
  default toolchain, not the latest). MSRV 1.90 set with a comment; edition 2024.
- `just` is not installed on this machine; the justfile recipes mirror the exact cargo commands
  run by hand. Install with `cargo install just` or use cargo directly.
- Clippy `all`/`pedantic` are enabled with `priority = -1` so individual allows win. Allowed:
  `module_name_repetitions`, `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`.
- **CI msrv job must call `cargo +<version>`**: `rust-toolchain.toml` pins stable and otherwise wins silently.
- `cargo mutants --in-diff` on PRs needs `fetch-depth: 0` and an explicit fetch of the base branch.
- `deny.toml` with `version = 2` sections avoids deprecated-key warnings on cargo-deny 0.20.x.
- **Skip attribute is plain `#[mutants::skip]`**, with the `mutants` crate as a regular dependency.
  cargo-mutants finds the attribute by scanning source text, so `#[cfg_attr(test, mutants::skip)]`
  (what the plan said) works only by accident and is misleading. Plan corrected.
- `cargo mutants --emit-schema config` prints the supported config keys for the installed version.
- The nightly mutants job never fails the run; the tracking issue "Surviving mutants (nightly)" is the signal. The PR diff job does fail on survivors.

### Findings from reading the script (relevant to T-102/T-103)

- Section insertion rules in the script are **not uniform**: `append_to_section` (before `## Progress`),
  `set_project` (after `## Description` or first), `append_mr_entry` (`### Merge requests` at end
  of `## Related`), `cmd_set` (new `## Tags` appended at end of file). Test each separately.
- Idempotence checks differ per list: worktrees match the path prefix, sessions match the backticked
  id anywhere in the file, MRs match `(url)` inside the subsection.
- The Sessions separator is a real EM DASH (U+2014) with spaces, not a hyphen.
- `### Merge requests` ends at any line starting with `##`, so another `###` also closes it.
- `--due` is stored verbatim by the script; existing non-ISO values must still parse (keep raw string
  on failure or treat as TBD; decided in T-102).
- Open points are listed at the end of `docs/file-format.md`.

## Blockers

(none open)

## Deviations from the plan

- T-001: repository URL in `Cargo.toml` is a placeholder (`https://example.invalid/tasq`) until a
  GitHub repo exists. Extra just recipes `default` and `fmt-check`.

## Open questions raised during implementation

(none yet)
