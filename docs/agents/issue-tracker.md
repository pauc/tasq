# Issue tracker: tasq

This project tracks its own work with tasq. An issue is a task in the `Tasq` nb notebook
(`tasq store info`), read and written with the installed `tasq` binary. Never edit the notebook
files with nb or by hand while a `tasq` command can do it; the store does the bookkeeping.

Ids are positional (`.index` line numbers). Refer to a task by id **and** title, e.g. `44 ("Create a plugin to manage worktrees ...")`.

## Conventions

- **Create**: `tasq create "<title>" --desc "<body>" --tag <topic> [--prio A|B|C] [--status <status>]`, run from the repo root so `## Project` is this checkout. Topic tags in use: `tasq`, `bug`, `ux`, `perf`.
- **Read**: `tasq view <id> --json` (shape in `docs/json.md`); `tasq view <id> --no-pager` for markdown.
- **List**: `tasq list --json` with `--status <s>`, `--tag <t>` (repeatable, all must match), `--all` / `--done`.
- **Comment**: `tasq log <id> "<note>"` appends a dated line under `## Progress`. There is no other comment channel.
- **Status / priority**: `tasq set <id> <status|A|B|C> ["<note>"]`. Statuses: `in-progress`, `ready`, `waiting`, `blocked`, `later`.
- **Close**: `tasq done <id> "<final note>"`; reopen with `tasq reopen <id> <status> "<note>"`.
- **Title**: `tasq rename <id> "<title>"`.
- **Due date**: `tasq due <id> <YYYY-MM-DD|today|tomorrow|none>`.
- **Tags**: `tasq tag <id> +<add> -<remove>`; only the named tags change. Status and priority words are refused (use `tasq set`).
- **Description**: `tasq desc <id>` prints it; `tasq desc <id> "<body>"` replaces it, `tasq desc <id> -` reads it from stdin, `tasq desc <id> ""` clears it. A line starting with `## ` is refused.
- These four take `--note "<note>"` to log why in the same write. For several fields at once, `tasq view <id> --json`, edit the document, pipe it to `tasq apply`; it refuses what it cannot express (a removed progress entry, link or session) and writes nothing.
- **Order within a status group**: `tasq move <id> --top` puts a task first in its group (above every priority); `tasq move <id> --unrank` hands it back to the priority, due, id order.

## Plans for larger issues

A task too big for one session gets `ruli/features/<slug>/PLAN.md` (spec, phases, sub-tasks with
an id prefix such as `WT-201`, acceptance criteria, decisions) and `PROGRESS.md` (status table,
learnings, commit shas). The first line of PLAN.md names the task: `Task: tasq 44 ("<title>")`.
Sub-tasks live in PLAN.md, not as separate tasq tasks. Log milestones on the task with `tasq log`.

## When a skill says "publish to the issue tracker"

A single issue: `tasq create`. A spec or a ticket breakdown: write or extend `ruli/features/<slug>/PLAN.md` (sub-tasks as numbered `<PREFIX>-NNN` sections) and `PROGRESS.md`, and link them from the task with `tasq log`.

## When a skill says "fetch the relevant ticket"

`tasq view <id> --json`, plus `ruli/features/<slug>/PLAN.md` and `PROGRESS.md` when the task has a plan.

## Wayfinding operations

- **Map**: a task tagged `wayfinder-map` and `wf-<effort>`; Notes / Decisions-so-far / Fog go in its progress log (`tasq log`).
- **Child ticket**: a task tagged `wf-<effort>` plus `research`, `prototype`, `grilling` or `task`; the question goes in `--desc`.
- **Blocking**: create the child with `--status blocked` and the `Blocked by: <id>, <id>` line at the top of `--desc`. Unblock with `tasq set <id> ready` once every blocker is done.
- **Frontier**: `tasq list --tag wf-<effort> --status ready --json`, lowest id first.
- **Claim**: `tasq set <id> in-progress`.
- **Resolve**: `tasq done <id> "<answer>"`, then `tasq log <map-id> "Decided: <gist> (task <id>)"`.
