---
name: sync
description: Refresh the tasq task list - run `tasq sync` for the configured sources (merge requests to review, assigned issues, an LLM inbox bridge), triage Slack and Gmail into tasks when no bridge is configured, and print a short briefing of what is new, what got closed and what needs attention. Use when the user asks to update, refresh or sync their tasks, or wants to know what came in.
allowed-tools: Bash(tasq *)
---

# Sync tasq tasks

`tasq sync` does the deterministic work: every enabled `[[source]]` in the config is fetched,
reconciled with the notebook and applied (new items become tasks with a `## Source` line,
merged / closed / reassigned items mark their task done). Your job is to run it, fill the gap
it cannot cover on its own, and report.

## 1. Run the configured sources

```sh
tasq sync --json
```

- The document is `{"schema": 1, "dry_run": false, "ok": bool, "sources": [{name, changes,
  applied: [{id, description}], error}]}`. Keep the `applied` lines per source: they are the
  briefing.
- A source with an `error` is reported, not retried blindly. Authentication errors name the
  token command or environment variable to fix (`forge.<name>.token_cmd`, `GITLAB_TOKEN`,
  `GITHUB_TOKEN`); say so and continue.
- `tasq: no [[source]] is configured` means nothing is set up yet. Point the user at
  `docs/config.md` and `examples/sources/config.toml` in the tasq repository, then continue
  with step 2 so the run is still useful.

## 2. Triage the inbox when no bridge does it

Check `tasq config show --json` for an enabled source of `kind = "llm-bridge"`. When there is
one, `tasq sync` already covered the inbox; skip this step unless the user asked for a manual
sweep.

Otherwise, with the connectors available in this session (Slack, Gmail), read the unread
mentions, direct messages and threads addressed to the user from the last two working days.
For each one that needs an action from the user:

1. Deduplicate against `tasq list --json --tag slack`, `tasq list --json --tag gmail` and the
   full `tasq list --json`: same permalink under `related`, or the same request already
   titled, means skip.
2. Create the task, always with the permalink, so a later bridge run or `tasq sync` matches
   it by URL instead of creating a duplicate:

   ```sh
   tasq create "<imperative title, under 60 characters, who and what>" \
     --desc "<one or two sentences of context>" \
     --related <permalink> --tag slack --status ready --prio B
   ```

   Use `--tag gmail` for mail, `--prio A` when someone else is blocked on the answer, and
   `--due YYYY-MM-DD` when the message names a date.

Skip FYI messages, notifications and anything already answered. When in doubt about whether
something is actionable, list it under "unsure" in the briefing instead of creating a task.

## 3. Brief the user

End with `tasq` (the grouped list) and, above it, a briefing of at most ten lines:

- **New**: one line per created task, `[id] title`, with the source.
- **Closed**: tasks marked done by a source, with the reason (`MR merged`, `issue closed`).
- **Needs attention**: tasks a source flagged, and the "unsure" inbox items.
- **Failed**: sources that errored, with the fix.

Then suggest the next action in one line: `tasq next` when something is in progress or ready,
otherwise what to pick.
