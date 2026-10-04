# Configuration

`tasq` reads TOML configuration in layers; later layers override earlier ones field by field:

1. Built-in defaults (below).
2. Global file: `$XDG_CONFIG_HOME/tasq/config.toml`, else `~/.config/tasq/config.toml`.
   `TASQ_CONFIG=<file>` or `--config <file>` replaces it.
3. Project file: the nearest `.tasq.toml` walking up from the current directory (stopping at
   your home directory; nothing above home is read).
4. `[profile.<name>]` blocks from the loaded files, when selected with `--profile` or `TASQ_PROFILE`.
5. Environment variables (table below).
6. `--set key=value` overrides.

Tables deep-merge; scalars and arrays replace (a later `[[source]]` list replaces the whole list).
Unknown keys are errors naming the file, line and column. `tasq config show` prints the effective
config and which layer set each value.

## Defaults

```toml
[store]
kind = "nb"
notebook = "home"
bookkeeper = "auto"          # auto | nb | native

[workflow]
statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
default_status = "ready"

[work]
worktree_manager = "git"     # git | command
# worktree_command = "gwm create {new} {branch} --no-tmux -s"
# default_project = "~/code/..."

[launch]
default = "claude"           # shell | claude | tmux | herdr
env = "direnv"               # inherit | direnv

[launch.claude]
# prompt_file = "~/.config/tasq/prompts/claude.md"

[ui]
pager = "less -RFX"
no_osc8 = false
glow_style = "dark"

[ui.colors]

# [forge.gitlab]            # kind and host inferred from the name when omitted
# host = "gitlab.example.com"
# token_cmd = "glab auth token"

# [[source]]
# name = "gitlab-review-requests"
# kind = "gitlab-review-requests"   # gitlab-review-requests | gitlab-work-items |
#                                   # github-review-requests | github-work-items | llm-bridge
# forge = "gitlab"
# tags = ["gitlab", "review-request"]
# status = "ready"
# enabled = true

[report.summary]
summarizer = "llm"           # raw | llm
command = "claude -p"
# model = "sonnet"
```

## Environment variables

| Variable | Effect |
|---|---|
| `TASQ_CONFIG` | replaces the global file (must exist) |
| `TASQ_PROFILE` | selects `[profile.<name>]` |
| `TASQ_NOTEBOOK` | `store.notebook` |
| `TASQ_BOOKKEEPER` | `store.bookkeeper` |
| `TASQ_DEFAULT_PROJECT` | `work.default_project` |
| `TASQ_WORKTREE_MANAGER` | `work.worktree_manager` |
| `TASQ_WORKTREE_COMMAND` | `work.worktree_command` |
| `TASQ_LAUNCHER` | `launch.default` |
| `TASQ_LAUNCH_ENV` | `launch.env` |
| `TASQ_PAGER` | `ui.pager` |
| `TASQ_NO_OSC8` | `ui.no_osc8` (`1/true/yes/on`, `0/false/no/off`) |
| `TASQ_GLOW_STYLE` | `ui.glow_style` |
| `TASQ_SUMMARIZER` | `report.summary.summarizer` |
| `TASQ_SUMMARY_MODEL` | `report.summary.model` |

Empty values count as unset. `XDG_CONFIG_HOME` is honoured for the global file location.

## Validation

- `workflow.default_status` must be one of `workflow.statuses`.
- A `[forge.<name>]` named `gitlab` or `github` infers `kind` and `host`; any other name needs `kind`.
- Forge-backed sources need `forge` pointing at a forge of the matching kind; `llm-bridge` needs `command`.
- `~` is expanded in paths.

## Worktree managers

`tasq worktree <id> --create <branch>` makes a worktree for the task's project (its `## Project`,
else `work.default_project`) and tracks it. `work.worktree_manager` picks how:

- `git` (default): `git worktree add` into `<project>-<branch>` next to the project, with `-b`
  when the branch exists neither locally nor on `origin`. The directory is reused if it exists.
- `command`: runs `work.worktree_command` inside the project. The value is a template split
  like a shell command line (quotes allowed, no shell), with `{branch}`, `{project}` (absolute
  path) and `{new}` substituted; `{new}` is `-b` when the branch does not exist yet and nothing
  otherwise (`{new:<text>}` substitutes `<text>` instead). The command must print the worktree
  path as its last line of standard output; earlier lines are shown to the user. Example for
  gwm: `"gwm create {new} {branch} --no-tmux -s"`.
