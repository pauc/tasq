# Sources and `tasq sync`

A source turns something outside the notebook into tasks: merge requests waiting for your
review, issues assigned to you, or whatever an LLM finds in your inbox. Sources are declared as
`[[source]]` blocks (see `docs/config.md`); `tasq sync` runs the enabled ones in order.

## What a sweep does

For each source:

1. `fetch` lists everything currently relevant. Each item has a stable external id
   (`group/project!123`, `owner/repo#42`, a ticket id), usually a URL, a title and a state.
2. Items are matched against existing tasks by the task's `## Source` line
   (`<source>: <external id> <url>`), or, for tasks that predate `tasq sync`, by the URL in
   `## Related` or `### Merge requests`.
3. A new open item becomes a task (`create_new`, default on): the source's `tags` and `status`
   (else `workflow.default_status`), the URL under `## Related`, the first progress note
   `sync(<source>): created`.
4. A matched open task whose item is done gets a note (`sync(<source>): merged`) and is marked
   done (`close_when_done`, default on). With `flag = "<tag>"`, matched open tasks missing that
   tag get it. Done tasks are never touched, so a merged MR that shows up again is not re-created.
5. Tracked open tasks the sweep no longer lists are re-checked one by one with `check`, which
   says whether each is still open, done (and why) or gone.

`tasq sync <id>...` skips the sweep and only re-checks those tasks. `--dry-run` prints the
changes without writing. `--source <name>` runs one source. One failing source is reported
and the others still run; the exit code is 1 when any failed.

## Forge sources

`gitlab-review-requests` / `github-review-requests`: open merge requests where you are a
requested reviewer. `check` reports done when the MR is merged, closed, approved by you, or
gone. Default title `Review MR !{iid}: {title}` (`Review PR #{iid}: {title}` on GitHub).

`gitlab-work-items` / `github-work-items`: open issues assigned to you, filtered by `labels`
(include), `exclude_labels` and `projects` (exact `group/project`, or a group prefix ending in
`/`). `check` reports done when the issue is closed, no longer assigned to you, or gone. Default
title `#{iid}: {title}`.

Both need a `[forge.<name>]` block with `host` and a token: `token_cmd` (`glab auth token`,
`gh auth token`) or `GITLAB_TOKEN` / `GITHUB_TOKEN`. Tokens are never printed. Requests retry on
429 and 5xx with backoff, follow `Link` pagination, and never run during `cargo test`.

## The LLM bridge

`kind = "llm-bridge"` runs `command` (split like a shell command line, no shell) with
`prompt_file` on its stdin and the environment variable `TASQ_SYNC_KNOWN` holding a JSON array
of the external ids already tracked for this source. The command must print JSON on stdout:

```json
[
  {
    "external_id": "slack:C123/p1696400000",
    "url": "https://workspace.slack.com/archives/C123/p1696400000",
    "title": "Reply to Ana about the export format",
    "body": "Thread in #support, Ana asked yesterday.",
    "state": "open",
    "status": "ready",
    "priority": "B",
    "tags": ["slack"],
    "due": "2026-10-10",
    "note": null
  }
]
```

Only `external_id` and `title` are required. `state` is `open` (default), `done` or
`needs-attention`. `status` must be a workflow status, `priority` `A`/`B`/`C`, `tags` plain
words without `#`, `due` `YYYY-MM-DD`. Accepted shapes: a bare array; an object with
`"items": [...]`; or the envelope `claude -p --output-format json` prints, whose `"result"`
string holds the array (a ```` ```json ```` fence around it is fine). Anything else is an
error quoting the first 200 characters of the output.

Repeated items are dropped: same external id, else same URL, else same title ignoring case and
whitespace. The bridge cannot look items up again, so it never closes tasks; close them with
`tasq done` or let another source do it.

Example configuration and prompt: `examples/sources/`.
