# ADR-0003: Store, Source and Launcher extension traits

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

The bash script mixes three concerns: where tasks are stored (nb files), where
new tasks and state changes come from (GitLab, Slack, Gmail via Claude slash
commands), and how a work session on a task is started (Claude Code, herdr,
gwm worktrees, direnv). Each is personal to the author today. For the tool to be
usable by others, each concern needs a seam where a different implementation can
be plugged in without touching the rest.

## Decision

`tasq-core` defines three extension traits. The core owns the domain model and
all logic; adapters implement the traits in separate crates.

```rust
pub trait Store {
    fn list(&self, filter: &Filter) -> Result<Vec<Task>>;
    fn get(&self, id: &TaskId) -> Result<Task>;
    fn create(&mut self, draft: TaskDraft) -> Result<Task>;
    fn update(&mut self, task: &Task) -> Result<()>;   // whole-task write; adapters diff as needed
    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<()>;
    fn describe(&self) -> StoreInfo;                    // name, location, capabilities
}

pub trait Source {
    fn name(&self) -> &str;
    fn fetch(&self, ctx: &SyncContext) -> Result<Vec<SourceItem>>;        // full sweep
    fn check(&self, items: &[Origin]) -> Result<Vec<SourceItemState>>;    // re-check specific tasks
}
// SourceItem: external id, url, title, suggested status/tags/priority, body,
// state (open/merged/closed/needs-attention)

pub trait Launcher {
    fn name(&self) -> &str;
    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome>;        // workdir, env, prompt, task
}
```

- **Store** persists tasks. First implementation: nb-compatible markdown
  (ADR-0002). `update` takes the whole task; the adapter decides how to diff it
  into the file.
- **Source** produces `SourceItem`s from somewhere external. The core reconciles
  them into tasks (ADR-0004). Implementations: `gitlab-review-requests`,
  `gitlab-work-items`, their GitHub twins, and the generic `llm-bridge`.
- **Launcher** starts a session for a task given a resolved `LaunchContext`.
  Implementations: shell, Claude Code, tmux, herdr.

### Work context is a core concept

A task's **work context** (project directory, tracked worktrees, tracked
sessions) is part of the `Task` model and of the core, not of any launcher.
The core resolves where a session starts (first existing tracked worktree, else
tracked project, else default project, FR-8), detects gone worktrees and builds
a `LaunchContext`. Launchers only decide *how* to open a session there. This
keeps `tasq next --dry-run` meaningful without any launcher and lets a new
launcher reuse all the resolution logic.

Adapters are registered through a `Registry` built from config at startup, so
the registration mechanism can later be fed by dynamic discovery (ADR-0006)
without changing the traits.

## Consequences

### Positive

- The CLI and TUI depend only on the traits; adding GitHub after GitLab touches
  one new module plus config (success metric in plan section 10).
- Each adapter is testable in isolation with fakes: fake `nb`, `wiremock`
  servers, fake launchers recording the context.
- `tasq-core` stays free of `reqwest`, `ratatui` and process spawning.

### Negative

- Three traits are more surface than one script. Trait signatures become a
  compatibility concern once third parties implement them.
- `Store::update` as a whole-task write pushes diffing into every store
  adapter. Accepted because it keeps the trait small and the core unaware of
  file layout.
- Some operations cut across traits (MR title resolution uses a forge client
  from the sources crate inside a CLI command). The plan routes these through
  the matching `Source`'s forge client rather than adding a fourth trait.

## Alternatives considered

- **One `Backend` trait** covering storage, sync and launching — simpler to
  register but forces every implementation to care about all three concerns.
- **Launchers own work context** — each launcher would re-implement worktree
  resolution and the "worktree is gone" prompt; dry runs would differ per
  launcher.
- **Function pointers / closures instead of traits** — less discoverable, no
  natural place for `name()`/`describe()` metadata.

## References

- Plan sections 4.2, 4.3; tasks T-101, T-401 to T-405, T-501 to T-505.
- ADR-0002, ADR-0004, ADR-0006.
