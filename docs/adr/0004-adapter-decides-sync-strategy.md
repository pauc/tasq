# ADR-0004: Each Source adapter decides its sync strategy

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

Today new tasks and status changes arrive through Claude slash commands
(`/update-tasks`, `/update-support-tasks`) that read GitLab, Slack, Gmail and
Freshdesk and edit todo files directly. Some of that data is structured (MRs
where I am a requested reviewer, issues assigned to me) and can be fetched
deterministically through an API. Some is unstructured (a Slack thread asking
for something) and needs an LLM to turn it into a task. The core must not have
to know which is which.

## Decision

Each `Source` adapter decides for itself whether it is **deterministic**,
**LLM-driven** or **hybrid**. The trait contract is the same for all of them:
return `SourceItem`s from `fetch` (a full sweep) or `SourceItemState`s from
`check` (re-checking known origins). The core only **reconciles** those items
into tasks, with a pure, unit-tested `reconcile(existing, items, policy) ->
Vec<Change>` producing create / update / close / flag changes that are then
applied through the `Store`.

Consequences for the adapters that ship:

- **Forge sources are deterministic** and split per concern so each has one
  clear policy: `<forge>-review-requests` (MRs/PRs where I am a requested
  reviewer; default policy `create_new` + `close_when_done`) and
  `<forge>-work-items` (issues assigned to me; create tasks by default, close
  on closure or reassignment). `<forge>` is `gitlab` or `github`; both sit on a
  shared forge client configured under `[forge.<name>]`.
- **The LLM bridge source** (`kind = "llm-bridge"`) is how an adapter chooses
  to use an LLM: it runs a configured command (for example `claude -p
  --output-format json` with a prompt file) and expects a JSON array of
  `SourceItem`s back. Slack, Gmail and any unstructured inbox go through it.
  The prompt is a user-editable file, not a string in Rust.
- A hybrid adapter can fetch deterministically and use an LLM only to
  classify or title items; the core cannot tell and does not need to.

Matching uses the `Origin` recorded in `## Source`, falling back to a URL in
`## Related` for tasks created before this tool existed.

## Consequences

### Positive

- The core's sync logic is pure and fully testable with table-driven cases;
  mutation testing targets zero missed mutants in `reconcile`.
- Users without an LLM can run the forge sources alone; users without a forge
  can run only the bridge. Neither path depends on the other.
- `tasq sync --dry-run` shows the same change list regardless of how items
  were produced.
- Adding a source for a new system is a mapper onto `SourceItem`, nothing in
  the core changes (FR-7).

### Negative

- Deduplication of LLM-produced items is weaker: without a stable external id
  the bridge deduplicates by URL, then by normalised title.
- Every source carries its own auth, pagination and error handling; shared
  behaviour lives in the forge client, not in the trait.
- Reconcile policies must be configurable per source (for example whether a
  work item closed by someone else marks the task done or only flags it, plan
  open question 4).

## Alternatives considered

- **Core-driven LLM pass over all sources** — one place to run the model, but
  forces an LLM dependency on deterministic sources and hides where
  non-determinism enters.
- **One `gitlab` source doing everything** — simpler config, but review
  requests and work items want different default status, tags and close
  policies; splitting keeps each policy obvious.
- **A source for my own open MRs/PRs** — deliberately not in v1; those are
  tracked explicitly with `tasq mr` and re-checked through the forge client
  (plan open question 3).

## References

- Plan sections 2, 4.6; tasks T-501 to T-505; open questions 3 and 4.
- ADR-0003.
