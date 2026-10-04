#!/bin/sh
# Claude Code status line showing the tasq task of this session.
#
# Sessions started by `tasq next` / `tasq pick` carry TASQ_TASK_ID (and
# TASQ_NOTEBOOK / TASQ_PROFILE, so `tasq view` reads the right notebook).
# The line is `[id] title`; outside a tasq session it is the current
# directory. Install it with, in ~/.claude/settings.json:
#
#   "statusLine": { "type": "command",
#                   "command": "/path/to/plugins/claude/statusline/tasq-statusline.sh" }
#
# The command receives Claude Code's status JSON on stdin; only
# workspace.current_dir is used here, and only when jq is installed.

input=$(cat)

if [ -n "${TASQ_TASK_ID:-}" ] && command -v tasq >/dev/null 2>&1; then
  title=$(tasq view --raw "$TASQ_TASK_ID" 2>/dev/null | head -n 1 | sed 's/^# \[.\] //')
  if [ -n "$title" ]; then
    printf '[%s] %s' "$TASQ_TASK_ID" "$title"
    exit 0
  fi
fi

if command -v jq >/dev/null 2>&1; then
  dir=$(printf '%s' "$input" | jq -r '.workspace.current_dir // empty')
else
  dir=$PWD
fi
printf '%s' "$dir" | sed "s|^$HOME|~|"
