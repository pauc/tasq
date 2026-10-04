#!/bin/sh
# log-event.sh: a tasq hook that appends one line per event to a log file.
#
# Configure it for any hook, with an absolute path:
#
#   [hooks]
#   post-create = ["/path/to/log-event.sh"]
#   post-done = ["/path/to/log-event.sh"]
#   pre-launch = ["/path/to/log-event.sh"]
#
# tasq runs it with the hook document on stdin ({"schema": 1, "hook": ..., "task":
# {...}}) and TASQ_HOOK, TASQ_TASK_ID and TASQ_BIN in the environment. A hook
# that exits non-zero is a warning for post-* hooks and aborts the launch for
# pre-launch; this one only writes a line. Needs jq.
set -eu

log=${TASQ_HOOK_LOG:-${XDG_DATA_HOME:-$HOME/.local/share}/tasq/hooks.log}
mkdir -p "$(dirname "$log")"
title=$(jq -r '.task.title')
printf '%s %s [%s] %s\n' "$(date '+%Y-%m-%d %H:%M')" "$TASQ_HOOK" "$TASQ_TASK_ID" "$title" >>"$log"
