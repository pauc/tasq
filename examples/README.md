# Examples

`sources/`: a `[forge.gitlab]` block plus three `[[source]]` entries (GitLab review requests,
GitLab work items filtered by group and label, and an `llm-bridge` inbox) to merge into
`~/.config/tasq/config.toml`, and `inbox.md`, the prompt the bridge feeds to `claude -p` so it
prints Slack and Gmail items as the JSON array `tasq sync` expects. How a sweep reconciles
items into tasks is in `docs/sources.md`.

`config/`: two complete, commented configuration files. `plain-markdown.toml` is for a machine
without nb: native bookkeeping, the shell launcher with an inherited environment, raw summaries
and no sources, with the two commands that create the notebook directory. `author.toml` is the
author's setup with the real key names: a self-hosted GitLab forge with `glab auth token`,
gwm worktrees through `work.worktree_command`, `launch.default = "auto"`, the LLM summarizer
and a `[hooks]` entry pointing at the example hook. Every key is defined in
`docs/config.md`; an unknown key is an error.

`plugins/`: `tasq-tlogs`, the reference out-of-process plugin, and `hooks/log-event.sh`, a
hook. The plugin is an executable that `tasq tlogs ...` finds on `PATH` and that talks to
`tasq` only through `$TASQ_BIN ... --json` (`dates`, `summary`); the hook reads the event
document on stdin and appends a line to a log. The mechanism (discovery, environment, hook
documents) is described in `docs/plugins.md`.
