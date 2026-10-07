# JSON surface

Every `tasq` command accepts `--json` and then prints exactly one JSON object with a `schema`
field. The current schema is **1**. Scripts and out-of-process plugins should check it and
refuse anything else; a future shape change bumps the number.

| Command | Payload next to `"schema": 1` |
|---|---|
| `tasq [list ...]` | `"tasks": [Task, ...]` in display order (group order, then priority, due, id); done tasks only with `--all` (after the open ones) or `--done` |
| `tasq create`, `set`, `log`, `done`, `view`, `project`, `worktree`, `session`, `mr`, `apply` | `"task": Task` as stored after the command |
| `tasq project <id>` (show) | `"task": Task`, `"default_project": path or null` |
| `tasq next`, `tasq pick` with `--dry-run` | `"task": Task`, `"workdir"`, `"in_worktree": bool`, `"launcher"`, `"env": {name: value}`, `"steps": [string]` |
| `tasq store info` | `"store": {name, location, task_count, id_scheme, ids_may_change_on_reconcile}`, `"bookkeeper": "nb" \| "native" \| "none"` |
| `tasq store sync` | `"synced": bool`, `"detail": string` |
| `tasq doctor` | `"checks": [{name, status: "ok" \| "warn" \| "fail", detail, fix}]`, `"ok": bool` |
| `tasq sync` | `"dry_run": bool`, `"ok": bool`, `"sources": [{name, changes: [string], applied: [{id, description}], error}]` |
| `tasq summary` | `"day"`, `"header"` (`Friday 2026-10-02`), `"summarizer": "raw" \| "llm"`, `"tasks": [{id, title, done, notes: [string]}]`, `"notes"` (the raw text), `"summary"` (the distilled text, `null` when raw) |
| `tasq dates` | `"spec"`, `"from"`, `"to"` (`YYYY-MM-DD`), `"days": [...]`, `"working_days": [...]` (Monday to Friday only) |
| `tasq config show` | `"config": the effective config`, `"profile"`, `"profiles"`, `"layers": [{origin, keys}]` |
| `tasq plugins list` | `"plugins": [{name, path}]` (executables `tasq-<name>` found on `PATH`), `"hooks": {"post-create": [string], "post-done": [string], "pre-launch": [string]}` |

`tasq <plugin> [args...]` is not covered by this table: its output is the plugin's own, `--json`
included, and `tasq` passes the arguments through verbatim (see `docs/plugins.md`).

## Task

The `Task` object is the core model (`tasq_core::model::Task`) serialised with serde:

```json
{
  "id": "12",
  "title": "Rewrite the tasks script in Rust",
  "done": false,
  "status": "in-progress",
  "priority": "A",
  "due": "2026-10-10",
  "description": "Port the bash script to a Rust workspace.",
  "project": "/home/me/code/tasks",
  "tags": ["gitlab"],
  "related": [{"url": "https://gitlab.example.com/g/p/-/issues/42", "label": null}],
  "merge_requests": [{"url": "https://gitlab.example.com/g/p/-/merge_requests/123", "label": "Add parser"}],
  "worktrees": [{"path": "/home/me/code/tasks-wt/feature-a", "branch": "feature-a"}],
  "sessions": [{"at": "2026-10-04 10:20", "id": "abc-123", "launcher": null, "description": "first session"}],
  "progress": [
    {"at": "2025-03-01", "note": "legacy note without time"},
    {"at": "2026-10-04 10:15", "note": "created via tasks create"}
  ],
  "origin": null,
  "closed_at": null
}
```

- `id` is a string (nb ids are index line numbers, other stores may use slugs).
- `status` is `null` for done tasks and for open tasks without a status tag; `priority` is
  always `A`, `B` or `C`.
- Timestamps are local time: `YYYY-MM-DD HH:MM` everywhere, except that a legacy progress
  entry may carry only `YYYY-MM-DD`.
- `tags` holds topic tags only; status and priority are fields, never tags.
- `origin` is `{"source", "external_id", "url"}` for tasks created by `tasq sync`.
- `closed_at` is when a done task was closed (`YYYY-MM-DD HH:MM`), `null` on open tasks and
  on tasks closed outside tasq (ADR 0020). It may be left out on input.

## `tasq apply`

`tasq apply` reads `{"schema": 1, "task": Task}` from stdin (or a file) and writes the task
through the store, which applies the differences it can express: title, description, status,
priority, due, project, topic tags, done, `closed_at`, and appended progress entries, worktrees, sessions,
related links and merge requests. Anything else (a dropped progress entry, a removed link) is
refused with an error naming the fields, and nothing is written. Piping `tasq view --json <id>` straight back into
`tasq apply` changes nothing.

Validation errors name the field: `apply: missing field \`task\``, `apply: unsupported schema
2 (expected 1)`, `apply: invalid task: missing field \`title\``.

## Hook documents

Commands configured under `[hooks]` (`docs/config.md`) receive one JSON object on stdin, with
the same `schema` as everything else:

| Hook | Document |
|---|---|
| `post-create` | `{"schema": 1, "hook": "post-create", "task": Task}` |
| `post-done` | `{"schema": 1, "hook": "post-done", "task": Task}` |
| `pre-launch` | `{"schema": 1, "hook": "pre-launch", "task": Task, "workdir": path, "launcher": name}` |

`task` is the task as stored when the hook runs (after the create or done write; before the
launch). The hook's environment carries `TASQ_HOOK`, `TASQ_TASK_ID` and `TASQ_BIN`, so a hook
that needs more can run `$TASQ_BIN view --json $TASQ_TASK_ID` or `$TASQ_BIN apply`. Hook stdout
is not parsed; a `post-*` hook that exits non-zero is a warning, a `pre-launch` one aborts the
launch.
