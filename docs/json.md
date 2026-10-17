# JSON surface

Every `tasq` command accepts `--json` and then prints exactly one JSON object with a `schema`
field. The current schema is **1**. Scripts and out-of-process plugins should check it and
refuse anything else; a future shape change bumps the number.

| Command | Payload next to `"schema": 1` |
|---|---|
| `tasq [list ...]` | `"tasks": [Task, ...]` in display order (group order, then rank, priority, due, id); done tasks only with `--all` (after the open ones) or `--done` |
| `tasq create`, `log`, `done`, `reopen`, `desc` without a value (it only prints), `view`, `project`, `worktree`, `worktree up`, `worktree release`, `session`, `mr`, `apply` | `"task": Task` as stored after the command |
| `tasq set`, `rename`, `due`, `tag`, `desc` with TEXT, `-` or `--edit` | `"task": Task` as stored after the command, `"changed": [string]`: the fields written (`"title"`, `"description"`, `"status"`, `"priority"`, `"due"`, `"tags"`), `[]` when the task already had the value and nothing was written |
| `tasq move` | `"task": Task` as stored after the command, `"changed": ["rank"]` (`[]` when the task's own rank did not change: it already sat where it was moved to, or only the tasks above it needed a rank), `"ranked": [id]`: the other tasks of the group whose rank was written for the move, in write order (`[]` for `--top`, `--unrank` and a `--before`/`--after` between ranked neighbours, which write only the moved task; the tasks above the destination when a move lands in the unranked part of the group) |
| `tasq worktree ls` | `"worktrees": [{task, title, status, done, path, branch, exists, changes}]`: `task` is the id, `status` the task's (`"done"` for a closed task, `null` without one), `branch` `null` for a directory tracked without one, `exists` whether the directory is there, `changes` the changed paths git reports (files provisioning wrote left out; `[]` when clean), `null` when it does not exist or git cannot say |
| `tasq worktree prune` | `"dry_run": bool`, `"forgotten": [{repo, lines}]` (per repository, the lines `git worktree prune --verbose` printed), `"released": [{task, lines}]` (per done task, what was removed and deleted, or would be); repositories and tasks with nothing to report are left out, and what was kept goes to stderr as warnings |
| `tasq worktree switch`, `tasq worktree path` | `"path"`: the directory (`switch` with `--json` neither prints a hint nor writes `$TASQ_CD_FILE`) |
| `tasq worktree shell-init` | `"script"`: the shell function |
| `tasq worktree trust` | `"file"`: the repository's `.tasq.toml` (its main checkout when it has none), `"approved": [{kind, line}]`: each hook line now approved, `kind` one of `"pre-create"`, `"post-create"`, `"pre-remove"`, `"post-remove"` (in that order, then the lines in file order); `[]` when the file sets no hooks |
| `tasq project <id>` (show) | `"task": Task`, `"default_project": path or null` |
| `tasq next`, `tasq pick` with `--dry-run` | `"task": Task`, `"workdir"`, `"in_worktree": bool` (only with worktrees enabled), `"launcher"`, `"env": {name: value}`, `"steps": [string]` |
| `tasq store info` | `"store": {name, location, task_count, id_scheme, ids_may_change_on_reconcile}`, `"bookkeeper": "native" \| "none"` |
| `tasq store reconcile` | `"changed"` (bool), `"blanked"` (index lines emptied because the file is gone), `"appended"` (files added to the index) |
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
  "closed_at": null,
  "rank": 1024
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
- `worktrees` is present only when worktrees are enabled (`worktree.enabled`, ADR 0022), in
  every document that carries a task, hook documents included. It may be left out on input,
  which leaves the stored worktrees unchanged.
- `rank` is the task's manual place within its status group (ADR 0025), an integer, smaller
  first; ranked tasks come before unranked ones. It is left out when the task has none. On
  input a missing `rank` leaves the stored rank unchanged and `"rank": null` clears it.

## `tasq apply`

`tasq apply` reads `{"schema": 1, "task": Task}` from stdin (or a file) and writes the task
through the store, which applies the differences it can express: title, description, status,
priority, due, project, topic tags, done, `closed_at`, `rank`, and appended progress entries, worktrees, sessions,
related links and merge requests. A task without a `worktrees` key keeps its stored worktrees, and one
without a `rank` key keeps its stored rank. Anything else (a dropped progress entry, a removed link) is
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
