---
name: wrapup
description: Wrap up the current work session on a tasq task - record what happened as progress notes with `tasq log`, track the merge requests, worktree and session that were used, and set the final status with `tasq set` or `tasq done`. Use when the user says to wrap up, finish, close out or hand off the session, or at the end of a session started by `tasq next` / `tasq pick`. Takes an optional task id; defaults to `$TASQ_TASK_ID`.
argument-hint: "[task-id]"
arguments: [id]
allowed-tools: Bash(tasq *)
---

# Wrap up a tasq task

Record this session on the task so the next session (or the standup summary, `tasq summary`)
starts from facts, not from memory. Every write goes through the `tasq` CLI; never edit the
notebook files directly.

## 1. Find the task

1. Use `$id` when the skill was given one.
2. Otherwise use the `TASQ_TASK_ID` environment variable (`tasq next` and `tasq pick` set it,
   together with `TASQ_NOTEBOOK` and `TASQ_PROFILE`, so `tasq` already points at the right
   notebook).
3. Otherwise run `tasq list --status in-progress --json` and ask the user which task this
   session was about. Never guess an id.

Then read the task as it is now:

```sh
tasq view --json <id>
tasq config show --json   # .config.workflow.statuses: the statuses you may set
```

Look at `task.progress` so you do not repeat notes that are already there, and at
`task.merge_requests`, `task.worktrees` and `task.sessions` to see what is already tracked.

## 2. Write the progress notes

Reconstruct what happened in this session from the conversation and, when a repository was
involved, from `git log` and `git status` in the working directory. Write one to three notes,
newest last. Each note is one line of plain facts, under about 200 characters:

- what was done (merged, pushed, reviewed, investigated, decided), with merge request or
  issue references as `!123` / `#123` plus the URL when it is known;
- what is blocking or waiting, and on whom;
- the concrete next step.

Leave out narration, file-by-file detail and tooling noise. Then log each note:

```sh
tasq log <id> "<note>"
```

## 3. Track what the session produced

Only what is not tracked yet (compare with the JSON from step 1):

```sh
tasq mr <id> <merge-request-url> [title]      # every MR opened or worked on
tasq worktree <id> <path>                     # the git worktree the session worked in
tasq session <id> <session-id> "<what for>"   # only when the session id is known
tasq project <id> <dir>                       # when the task had no project yet
```

Never invent a session id; if you do not know it, skip `tasq session`.

## 4. Set the status

Pick the status that describes the task now, from the workflow statuses read in step 1:

- the work is finished: `tasq done <id> "<final note>"` (ask first if the user never said the
  task is finished);
- otherwise, when the status changed: `tasq set <id> <status> ["<why>"]`, for example
  `waiting` with a note naming who or what you wait on, `blocked` with the blocker,
  `ready` when it can be picked up again; priority with `tasq set <id> A|B|C`.
- unchanged: do nothing.

## 5. Confirm

Run `tasq view --raw <id>` and show the user the `## Progress` tail and the title line, then a
two-line recap: what was recorded, and what the next session should start with. Nothing else.
