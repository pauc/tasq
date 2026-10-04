Work with me on my next task: todo [{{id}}] from my nb notebook (file: {{file}}).

{{markdown}}

Working directory: {{workdir}}{{#worktrees}}

Tracked development worktrees for this task (we start in the first one that exists):
{{worktrees}}{{/worktrees}}{{#sessions}}

Previous Claude sessions on this task — if earlier context would help, suggest resuming one with: claude --resume <session-id>
{{sessions}}{{/sessions}}

Start by reading the task and its Related links to build context (use the glab CLI for GitLab URLs — run it from this repo so it authenticates). Give me a short summary of where the task stands and propose a first step, then help me work on it.{{#herdr}} This session runs in a herdr workspace created for this task with a provisional label — as your very first action, rename it to a short meaningful title: 2-3 keywords that identify the task and fit a narrow sidebar (e.g. "Company metrics"), no ids or prefixes, via: herdr workspace rename $HERDR_WORKSPACE_ID "<title>"{{/herdr}}

As we make progress, record it: tasq log {{id}} "<note>". If the status changes: tasq set {{id}} <{{statuses}}> [note]; priority: tasq set {{id}} <A|B|C>. Track the task's project directory with: tasq project {{id}} <path> (sessions start in the first tracked worktree that exists, otherwise there). Track development worktrees with: tasq worktree {{id}} <path>; to create a new worktree use: tasq worktree {{id}} --create <branch>, which runs the configured worktree manager and tracks the result. Track Claude sessions with: tasq session {{id}} <session-id> [desc], and merge requests with: tasq mr {{id}} <mr-url> [title]. When the task is finished, mark it done: tasq done {{id}} [note]. At the end of the session, run the /tasq:wrapup skill to record progress and final status.
