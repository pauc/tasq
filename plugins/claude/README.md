# tasq Claude Code plugin

Skills for working on tasq tasks from inside Claude Code. Namespaced under `tasq:` so they never
collide with other skills called `wrapup` or `sync`.

| Skill | What it does |
|---|---|
| `/tasq:wrapup [task-id]` | Records the session on the task: progress notes with `tasq log`, merge requests / worktree / session tracking, final status with `tasq set` or `tasq done`. Defaults to `$TASQ_TASK_ID`, which `tasq next` and `tasq pick` set. |
| `/tasq:sync` | Runs `tasq sync --json` for every configured source, triages Slack and Gmail into tasks when no LLM bridge is configured (always with the permalink under `--related`, so later syncs match by URL), and prints a briefing. |

Everything the skills do goes through the `tasq` CLI (`--json` in, `tasq create/log/set/done`
out), so they work with any store and any notebook the session points at.

## Install

For one session, from a checkout of this repository:

```sh
claude --plugin-dir /path/to/tasq/plugins/claude
```

Permanently, the repository is also a one-plugin marketplace:

```sh
claude plugin marketplace add /path/to/tasq     # or the git URL once published
claude plugin install tasq@tasq
```

Check the layout any time with `claude plugin validate --strict plugins/claude`.

## Status line

`statusline/tasq-statusline.sh` shows `[id] title` in Claude Code's status line while working in
a session started by `tasq next` / `tasq pick`, and the current directory otherwise. Enable it
in `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/path/to/tasq/plugins/claude/statusline/tasq-statusline.sh"
  }
}
```

It needs `tasq` on `PATH`; `jq` is optional (used for the directory fallback).

## What it does not do

The original `tasks` script's `/wrapup`, `/update-tasks`, `/update-support-tasks` and
`/time-logs` skills are untouched and keep working against the script. Freshdesk and time logs
are out of scope for tasq v1 (plan Non-Goals); time logs are meant to become an external plugin
on top of `tasq dates` and `tasq summary --json`.
