# Configuration

`tasq` reads TOML configuration from one directory, the **config directory** (ADR 0031):
`--config-dir <dir>`, else `TASQ_CONFIG_DIR`, else `$XDG_CONFIG_HOME/tasq`, else
`~/.config/tasq`. A directory named by `--config-dir` or `TASQ_CONFIG_DIR` must exist (a
relative one starts at the current directory); the default one may be absent, and every file
in it is optional:

```
~/.config/tasq/
├── config.toml           the global config
├── profiles/<name>.toml  one profile per file
└── repos/<any>.toml      one repository config per file
```

Nothing is read from the directory tasq runs in or from a repository. Later layers override
earlier ones field by field:

1. Built-in defaults (below).
2. `config.toml`.
3. The selected profile, `profiles/<name>.toml`, when selected with `--profile` or
   `TASQ_PROFILE` (see "Profiles").
4. The repository config, for the worktree commands only: the file in `repos/` whose `path` is
   the repository's main checkout (see "Repository configs").
5. Environment variables (table below), including `TASQ_SET`.
6. `--set key=value` overrides.

Tables deep-merge; scalars and arrays replace (a later `[[source]]` list replaces the whole list).
Unknown keys are errors naming the file, line and column. `tasq config show` prints the effective
config and which layer set each value; it never shows a repository config, `tasq worktree
config [DIR]` does (see "Repository configs").

Values given through the environment or `--set` are converted to the key's type:
`true`/`false`/`yes`/`no`/`1`/`0`/`on`/`off` for booleans, a comma-separated list for arrays,
text otherwise. A key that does not exist is an error.

## Profiles

A profile is a named partial configuration: `profiles/<name>.toml` in the config directory is
the profile `<name>`, applied over `config.toml` only when selected with `--profile <name>` or
`TASQ_PROFILE=<name>` (`--profile` wins). Its body has the shape of `config.toml`, any key of
this page, with no wrapper table:

```toml
# ~/.config/tasq/profiles/work.toml
[store]
name = "work"

[[source]]
name = "gitlab-review-requests"
kind = "gitlab-review-requests"
forge = "gitlab"
```

Only the selected file is read. Selecting a name with no file is an error that names the file
it looked for and the profiles there are (`profile "x" is not defined: no file
~/.config/tasq/profiles/x.toml; available profiles: home, work`). Profiles do not nest, and a
`[profile.<name>]` table in `config.toml` or a profile file is an error at its name:
`profiles are files now: move [profile.<name>] to <config dir>/profiles/<name>.toml`.
`tasq config show` names the profile file as the layer of each key it sets
(`profile work (~/.config/tasq/profiles/work.toml)`).

To pick a profile per directory, export `TASQ_PROFILE` from that directory's `.envrc`; tasq
itself never looks at the directory it runs in.

## Defaults

Serialising the default configuration gives this document. Keys whose default is unset
(`work.default_project`, `worktree.ports`, `launch.claude.prompt_file`,
`report.summary.model`, `report.summary.prompt_file`) are shown commented out. `data_dir`
has no built-in value: loading fills it in from the environment (`$XDG_DATA_HOME/tasq/stores`,
else `~/.local/share/tasq/stores`), so `tasq config show` prints it as a default.

```toml
# data_dir = "~/.local/share/tasq/stores"   # $XDG_DATA_HOME/tasq/stores when set
worktrees_enabled = false    # worktrees per task, see "Worktrees"

[store]
name = "default"             # the store directory is <data_dir>/<name>

[workflow]
statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
default_status = "ready"

[work]
# default_project = "~/code/..."

[launch]
default = "claude"           # auto | claude | shell | tmux | herdr
window = "auto"              # auto | tmux | herdr   (--window, the TUI's Ctrl/Shift+Enter)
env = "direnv"               # inherit | direnv
# log = "~/.local/state/tasq/launch.log"

[launch.claude]
# prompt_file = "~/.config/tasq/prompts/launch/claude.md"

[launch.herdr]
placement = "auto"           # auto | workspace | tab

[ui]
pager = "less -RFX"
no_osc8 = false
glow_style = "dark"
week_start = "monday"        # first column of the calendar picker: monday .. sunday
due_format = "relative"      # relative (overdue 3d) | iso (due 2026-10-03) | both
detail_position = "right"    # where Right shows the task detail in tasq ui: right | bottom

[ui.colors]                  # status name (or "no-status") = colour, see "Colours"

[ui.keys]                    # action = key or list of keys, [] unbinds; see "Key bindings"
# launch-window = "alt+enter"

[ui.theme]
preset = "dark"              # dark, light, solarized, gruvbox or mono; see "Colours"

[ui.theme.colors]            # role = colour (chip-bg, focus, link, ...); see "Colours"

# [forge.gitlab]            # kind and host inferred from the name when omitted
# kind = "gitlab"           # gitlab | github; required when the block name is neither
# host = "gitlab.example.com"
# token_cmd = "glab config get token --host gitlab.example.com"  # else GITLAB_TOKEN / GITHUB_TOKEN
# url = "https://gitlab.example.com/api/v4"   # API base; defaults from host

# [[source]]                      # a key marked with kinds is an error on any other kind
# name = "gitlab-review-requests"
# kind = "gitlab-review-requests"   # gitlab-review-requests | gitlab-work-items |
#                                   # github-review-requests | github-work-items | llm-bridge
# forge = "gitlab"                  # forge-backed kinds
# command = "claude -p --output-format json"   # llm-bridge only
# prompt_file = "~/.config/tasq/prompts/source/inbox.md"  # llm-bridge only
# tags = ["gitlab", "review-request"]
# status = "ready"
# enabled = true
# auto = true                       # false: a bare `tasq sync` skips it; run it with --source or from the TUI
# create_new = true                 # false: only update tasks that already exist
# done_when_closed = true           # forge-backed kinds: done when the MR closes unmerged or the issue closes
# done_when_merged = true           # review requests: ... when the MR is merged
# done_when_approved = true         # review requests: ... when you approved the MR
# done_when_unassigned = true       # work items: ... when the open issue is no longer yours
# flag = "review-request"           # tag added to matched open tasks that lack it
# title = "Review MR !{iid}: {title}"  # forge-backed kinds: {title} {iid} {project}
# labels = ["team::core"]           # work items: include only these labels
# exclude_labels = ["wontfix"]      # work items: skip these labels
# exclude_authors = ["dependabot"]  # review requests: skip MRs by these usernames
# drafts = false                    # review requests: true keeps draft MRs
# projects = ["group/", "owner/repo"]  # work items: allow-list; a trailing / matches a group

[report.summary]
summarizer = "llm"           # raw | llm
command = "claude -p"        # reads the prompt (instructions + notes) on stdin
# model = "sonnet"           # appended as --model <model>
# prompt_file = "~/.config/tasq/prompts/report/summary.md"  # {{day}} {{date}} {{notes}}

[hooks]
post-create = []             # command lines, see "Hooks"
post-done = []
pre-launch = []

[plugins]
enabled = []                 # tasq-<name> plugins told about events, see "Plugins"
timeout = 2                  # seconds a plugin may take over one event

# [plugin.git]               # one table per plugin, passed to it as is
# interval = 60
```

## Every key

| Key | Type | Default | Meaning |
|---|---|---|---|
| `data_dir` | path | `$XDG_DATA_HOME/tasq/stores`, else `~/.local/share/tasq/stores` | The directory holding the stores (ADR-0026). `~` expanded; a relative path is taken from the working directory. |
| `store.name` | string | `default` | The store's directory name under `data_dir`: one path component, neither empty nor `.`/`..`, no `/`. A store that does not exist yet is empty; the first write creates it. |
| `workflow.statuses` | array of strings | `["in-progress", "ready", "waiting", "blocked", "later"]` | Statuses in display and `next` search order. Lowercase kebab-case. |
| `workflow.default_status` | string | `ready` | Status of new tasks. Must be one of `statuses`. |
| `work.default_project` | path | unset | Directory a session starts in when the task tracks neither a worktree nor a project. `~` expanded. |
| `launch.default` | string | `claude` | Launcher for `next`/`pick`: `auto`, `claude`, `shell`, `tmux`, `herdr`. Not validated at load time. |
| `launch.window` | string | `auto` | Launcher for `next`/`pick --window` (the TUI's `Ctrl+Enter` and `Shift+Enter`): `herdr`, `tmux`, or `auto` for whichever the terminal runs in; an error when neither. See "Launchers". |
| `launch.env` | `inherit` \| `direnv` | `direnv` | Where the session's environment comes from. |
| `launch.log` | path | unset | File every `next`/`pick` appends its launch trace to. See "Debugging a launch". `~` expanded. |
| `launch.claude.prompt_file` | path | unset | Prompt template replacing the built-in `crates/launch/templates/claude.md`. `~` expanded. |
| `launch.herdr.placement` | `auto` \| `workspace` \| `tab` | `auto` | What a herdr session opens: `auto` a tab in the workspace already holding the directory, else a workspace; `workspace` always a new workspace; `tab` always a tab (in the holding workspace, else the current one). |
| `ui.pager` | string | `less -RFX` | Pager for long output on a terminal, split without a shell. `cat` or empty disables it. |
| `ui.no_osc8` | bool | `false` | Disable OSC 8 hyperlinks in `tasq view` and on the link labels of the `tasq ui` detail. |
| `ui.glow_style` | string | `dark` | Style passed to `glow -s`. |
| `ui.week_start` | weekday name | `monday` | First column of the calendar picker in `tasq ui` (`monday`, `tuesday`, ... `sunday`). |
| `ui.due_format` | `relative` \| `iso` \| `both` | `relative` | How the rows of `tasq list` and `tasq ui` show a due date: `overdue 3d`, `due today`, `due tomorrow`, `due in 4d`; `due 2026-10-03`; or `overdue 3d, 2026-10-03`. The detail pane of `tasq ui` always shows both, `--json` always the ISO date. A done task always shows its ISO date, dim. |
| `ui.detail_position` | `right` \| `bottom` | `right` | Where `tasq ui` shows the selected task's detail (`Right`, `Tab`): beside the list from 100 columns, or under it (list on top, half the height each) from 20 rows. Narrower or shorter terminals show the detail instead of the list. |
| `ui.colors.<name>` | string | empty table | Colour per status name, plus `no-status` and `done`, on top of the theme. See "Colours". |
| `ui.theme.preset` | `dark` \| `light` \| `solarized` \| `gruvbox` \| `mono` | `dark` | The built-in colour theme of `tasq list`, `tasq view` and `tasq ui`. See "Colours". |
| `ui.theme.colors.<role>` | string | empty table | Colour per role (`in-progress`, `ready`, `waiting`, `blocked`, `later`, `other-status`, `no-status`, `done`, `chip-bg`, `chip-fg`, `prio-a`, `focus`, `selection`, `error`, `dim`, `link`, `header`, `overdue`, `due-soon`), over the preset. See "Colours". |
| `ui.keys.<action>` | string or array of strings | empty table | Keys of a `tasq ui` action, replacing its defaults; `[]` unbinds it. See "Key bindings". |
| `forge.<name>.kind` | `gitlab` \| `github` | inferred from `<name>` | API the host speaks. Required when the block is not called `gitlab` or `github`. |
| `forge.<name>.host` | string | `gitlab.com` / `github.com` by kind | Host without scheme. |
| `forge.<name>.token_cmd` | string | unset | Command whose stdout is the token. Unset: `GITLAB_TOKEN` / `GITHUB_TOKEN`. |
| `forge.<name>.url` | string | `https://<host>/api/v4` (GitLab), `https://api.github.com` (github.com), `https://<host>/api/v3` (other GitHub) | API base URL override. |
| `source[].name` | string | required | Unique name, recorded in each task's `source` key. |
| `source[].kind` | string | required | `gitlab-review-requests`, `gitlab-work-items`, `github-review-requests`, `github-work-items`, `llm-bridge`. |
| `source[].forge` | string | unset | Forge-backed kinds: `[forge.<name>]` to use. Required by them; its kind must match. |
| `source[].command` | string | unset | `llm-bridge`: the command it runs. Required by it. |
| `source[].prompt_file` | path | unset | `llm-bridge`: file fed to the command on stdin. `~` expanded. |
| `source[].tags` | array of strings | `[]` | Tags added to every task the source creates. |
| `source[].status` | string | `workflow.default_status` | Status of tasks the source creates. |
| `source[].enabled` | bool | `true` | Whether the source can run at all. `false`: never, and naming it is an error. |
| `source[].auto` | bool | `true` | Whether a bare `tasq sync` (and the TUI's `s`) includes it. `false`: it runs only when named with `--source` or checked in the TUI's `S` picker. |
| `source[].create_new` | bool | `true` | Create tasks for new items. `false`: only update existing tasks. |
| `source[].done_when_closed` | bool | `true` | Review requests and work items: log a note and mark the task done when its MR is closed without being merged, or its issue is closed. `false`: the task stays open. |
| `source[].done_when_merged` | bool | `true` | Review requests: log a note and mark the task done when its MR is merged. |
| `source[].done_when_approved` | bool | `true` | Review requests: log a note and mark the task done when you approved its MR. |
| `source[].done_when_unassigned` | bool | `true` | Work items: log a note and mark the task done when its issue is open but no longer assigned to you. `false` keeps it open through a handoff for review. |
| `source[].flag` | string | unset | Tag added to matched open tasks that lack it. |
| `source[].title` | string | per kind | Forge-backed kinds: title template: `{title}`, `{iid}`, `{project}`. Defaults `Review MR !{iid}: {title}`, `Review PR #{iid}: {title}`, `#{iid}: {title}`. |
| `source[].labels` | array of strings | `[]` | Work items: only those carrying one of these labels. |
| `source[].exclude_labels` | array of strings | `[]` | Work items: skip those carrying one of these labels. |
| `source[].exclude_authors` | array of strings | `[]` | Review requests: skip merge requests by these authors (exact usernames, `dependabot`). Tracked tasks still close when their MR merges (with `done_when_merged`). |
| `source[].drafts` | bool | `false` | Review requests: `true` makes draft merge requests tasks too; `false` skips them until marked ready. |
| `source[].projects` | array of strings | `[]` (every project) | Work items: only these projects, `group/project`, a group prefix with a trailing `/`, or `owner/repo`. |
| `report.summary.summarizer` | `raw` \| `llm` | `llm` | Print the notes, or distil them with `command`. |
| `report.summary.command` | string | `claude -p` | Reads the rendered prompt on stdin, prints the summary. |
| `report.summary.model` | string | unset | Appended as `--model <model>`. |
| `report.summary.prompt_file` | path | unset | Template replacing the built-in `crates/launch/templates/summary.md`; `{{day}}`, `{{date}}`, `{{notes}}`. `~` expanded. |
| `hooks.post-create` | array of strings | `[]` | Command lines run after `tasq create`. |
| `hooks.post-done` | array of strings | `[]` | Command lines run after `tasq done`. |
| `hooks.pre-launch` | array of strings | `[]` | Command lines run before `tasq next`/`tasq pick` start a session. |
| `plugins.enabled` | array of strings | `[]` | Plugins told about events, each run as `tasq-<name>` from `PATH`, in this order. Not empty, no `/`, no whitespace, not `.` or `..`, no leading `-`, not a built-in command, not listed twice. See "Plugins". |
| `plugins.timeout` | integer (seconds) | `2` | How long one plugin may take over one event before its process group is killed and a warning reported. `0` kills every plugin at once. |
| `plugin.<name>.*` | any TOML | empty table | The plugin's own settings, not checked by tasq, sent to it as `config` in every event payload. See "Plugins". |
| `worktrees_enabled` | bool | `false` | Whether worktrees per task exist. Off hides the commands, the JSON field and the TUI section; files are never rewritten because of it. Top level only, never in a repository config. See "Worktrees". |

Source of truth: `crates/core/src/config/mod.rs`.

## Environment variables

| Variable | Effect |
|---|---|
| `TASQ_CONFIG_DIR` | the config directory (must exist); `--config-dir` wins over it |
| `TASQ_PROFILE` | selects `profiles/<name>.toml`; `--profile` wins over it |
| `TASQ_SET` | newline-separated `key=value` overrides, applied like `--set` (any key); part of the env layer, wins over the `TASQ_*` variables below for the same key, loses to `--set` flags. `tasq` sets it for the plugins and hooks it runs, so they see the same `--set` overrides |
| `TASQ_DATA_DIR` | `data_dir` |
| `TASQ_STORE_NAME` | `store.name` |
| `TASQ_DEFAULT_PROJECT` | `work.default_project` |
| `TASQ_LAUNCHER` | `launch.default` |
| `TASQ_LAUNCH_WINDOW` | `launch.window` |
| `TASQ_LAUNCH_ENV` | `launch.env` |
| `TASQ_LAUNCH_LOG` | `launch.log` |
| `TASQ_HERDR_PLACEMENT` | `launch.herdr.placement` |
| `TASQ_PAGER` | `ui.pager` |
| `TASQ_NO_OSC8` | `ui.no_osc8` (`1/true/yes/on`, `0/false/no/off`) |
| `TASQ_GLOW_STYLE` | `ui.glow_style` |
| `TASQ_WEEK_START` | `ui.week_start` |
| `TASQ_DUE_FORMAT` | `ui.due_format` |
| `TASQ_DETAIL_POSITION` | `ui.detail_position` |
| `TASQ_THEME` | `ui.theme.preset` |
| `TASQ_SUMMARIZER` | `report.summary.summarizer` |
| `TASQ_SUMMARY_MODEL` | `report.summary.model` |
| `TASQ_SUMMARY_COMMAND` | `report.summary.command` |
| `TASQ_SUMMARY_PROMPT_FILE` | `report.summary.prompt_file` |
| `TASQ_WORKTREES_ENABLED` | `worktrees_enabled` |
| `TASQ_WORKTREE_PATH` | `worktree.path` |
| `TASQ_WORKTREE_BRANCH` | `worktree.branch` |
| `TASQ_WORKTREE_BASE` | `worktree.base` |
| `TASQ_WORKTREE_ON_CREATE` | `worktree.on_create` |
| `TASQ_WORKTREE_ON_DONE` | `worktree.on_done` |
| `TASQ_WORKTREE_DELETE_BRANCH` | `worktree.delete_branch` |

Empty values count as unset. `XDG_CONFIG_HOME` is honoured for the config directory,
`XDG_DATA_HOME` for the default `data_dir`.

Variables that are not configuration: `NO_COLOR` turns colour off; `TASQ_NOW="YYYY-MM-DD
HH:MM"` fixes the clock (see `docs/testing.md`); `TASQ_TASK_ID`, `TASQ_DATA_DIR`,
`TASQ_STORE_NAME` and, when in effect, `TASQ_PROFILE` and `TASQ_CONFIG_DIR` are set in launched
sessions (the first two double as the overrides above, so the session's `tasq` opens the same
store); `TASQ_BIN`, `TASQ_HOOK` and
`TASQ_TASK_ID` are set for plugins and hooks, `TASQ_EVENT` for a plugin told about an event
(a `tasq` started with it set tells no plugin and prints no lasting warnings), and
`TASQ_STORE_DIR` and `TASQ_LOCKED=1` for the command `tasq store lock` runs (see
`docs/plugins.md`).

## Validation

- `workflow.default_status` must be one of `workflow.statuses`.
- `store.name` is one directory name: not empty, not `.` or `..`, no `/`; the error names the
  layer that set it.
- `[worktree.ports]` needs `block` of at least 1, `span` of at least `block`, and
  `base + span` at most 65536.
- A `[profile.<name>]` table is an error naming the file to move it to (see "Profiles").
- `enabled` under `[worktree]` is an error at its value naming `worktrees_enabled` (see
  "Worktrees"), from a file, `--set` or `TASQ_SET`.
- `work.worktree_manager` and `work.worktree_command` no longer exist; setting them is an error
  pointing here.
- `worktree.copy_ignored` (copy gitignored files) is not implemented yet; setting it is an error.
  List the files under `worktree.copies` meanwhile.
- A `[forge.<name>]` named `gitlab` or `github` infers `kind` and `host`; any other name needs `kind`.
- Forge-backed sources need `forge` pointing at a forge of the matching kind; `llm-bridge` needs `command`.
- A `[[source]]` key its kind does not read is an error at the key, naming the source and the
  kind (`` source "mrs": `labels` does not apply to kind gitlab-review-requests ``). Every kind
  reads `name`, `kind`, `tags`, `status`, `enabled`, `auto`, `create_new` and `flag`; the
  description of every other `source[]` key above starts with the kinds that read it ("Review
  requests", "Work items", "Forge-backed kinds", `llm-bridge`). See `docs/sources.md`, "When a
  task is marked done".
- `close_when_done` no longer exists; setting it is an error naming `done_when_closed`,
  `done_when_merged`, `done_when_approved` and `done_when_unassigned`.
- `~` is expanded in paths.

## Worktrees

Worktrees per task are off by default (ADR 0022). With `worktrees_enabled = false` (a top-level
key, in `config.toml` or a profile, or `TASQ_WORKTREES_ENABLED`) nothing about
them shows anywhere; turn them on to declare a worktree for a task, create it when the task is
launched and release it when the task is closed.

Off (the default) means invisible: `tasq worktree` is hidden from `--help` and completions and
refused with the key to set, task JSON has no `worktrees` key (hook documents included) and
`next`/`pick --dry-run --json` no `in_worktree`, the terminal UI shows no Worktrees section, the
Claude prompt drops its worktree instructions, and sessions start in the project even when the
task file lists worktrees. Task files are never rewritten because of the setting: a
`## Worktrees` section stays as it is (`tasq view` still prints the file) and is used again
once worktrees are enabled.

Declaring writes the task's `## Worktrees` entry and nothing else: no `git` runs and nothing is
created on disk. `tasq create ... --worktree [BRANCH]` declares one for the new task, and with
`on_create = "auto"` every new task whose project is inside a git work tree gets one without the
flag (the check reads the filesystem). `tasq worktree set <id> [BRANCH]` declares one on an existing
task; an argument that is an existing directory, starts with `/` or `.`, or contains `~` is
tracked as an existing worktree instead. A relative rendered `path` is taken from the project.

`tasq next`/`pick` (and the terminal UI's Enter) create the declared worktree when none exists,
before the task is set in-progress: the first declared worktree with a branch, at its stored
path, in the task's project repository. The branch is the local one when it exists, else
`origin/<branch>` (fetched, and tracked) when the remote has it, else a new branch from
`worktree.base` that tracks nothing; without `origin/HEAD` the default base falls back to
`HEAD` with a warning. A failure stops the launch with git's error and leaves nothing at the
path; `--dry-run` lists the step and fetches and creates nothing. `tasq worktree up <id>
[--from REF] [--dry-run]` does the same without launching, and the hidden `tasq worktree set <id>
--create <branch>` declares on `branch` and creates it at once.

Once git has created it, the worktree is provisioned: `copies` first, then `symlinks`, then
`templates` (so a template can replace a copied file). Copies and links come from the
repository's main checkout at the same relative path; a copied directory keeps its files and
links. A copy or link whose source does not exist, or whose target the checkout already has (a
tracked file), is skipped with a warning. Templates are rendered with the variables below before
git runs, so a missing template or an unknown `{{variable}}` in one stops the launch with nothing
created; a rendered file replaces what the checkout has. Copy, link and template targets must be
relative paths that stay inside the worktree. `--dry-run` lists every step.

Hooks: `pre-create` runs in the main checkout before `git worktree add` (a failure stops
everything, nothing is created), `post-create` in the new worktree after provisioning (a failure
is a warning). A hook line is split into words like a shell would (quotes group, nothing is
expanded), then each word is rendered with the template variables, so a value with spaces stays
one argument; no shell runs it, write `sh -c '...'` for one. A hook gets the user's environment
plus `TASQ_WORKTREE`, `TASQ_BRANCH`, `TASQ_TASK_ID`, every variable as `TASQ_VAR_<NAME>`
(`TASQ_VAR_BRANCH_DB`, `TASQ_VAR_DB_PREFIX`, ...) and, when in effect, `TASQ_PROFILE` and
`TASQ_CONFIG_DIR`; its output shows with `-v`.

Every hook runs without asking, whichever layer set it (ADR 0031): there is no approval step, and
nothing is read from a repository or the directory tasq runs in.

Closing a task (`tasq done`, the terminal UI's `d`) releases its worktrees when `on_done =
"release"`; `tasq worktree release <id>` does the same without closing it. Each declared worktree
that exists is removed with `git worktree remove` unless it is the main checkout, is not a
worktree of the repository, contains the current directory (a session running `tasq done` from
inside it), is locked, or has modified, staged or untracked files. Files that provisioning wrote
(copies, links, rendered templates) do not count: they can be made again, and they go with the
worktree. `pre-remove` runs inside the worktree first (a failure keeps it), `post-remove` in the
main checkout after (a failure is a warning). Then, with `delete_branch = "safe"`, the local
branch is deleted when nothing is lost: it is merged into the default branch (`origin/HEAD`'s
branch, remote or local, else the local `main` or `master`; a squash or rebase merge counts), or
its upstream on the remote (else `origin/<branch>`) contains it. Nothing is fetched first: the
remote branches are as last fetched. Whatever is kept is named in a warning; `tasq worktree
release <id> --force` removes a dirty worktree and deletes a branch that is neither merged nor
pushed, nothing else. `--dry-run` lists what would go. The task is closed even when a release
fails, and the `## Worktrees` entries always stay: launching the task again creates the worktree
again, on the kept local branch, the pushed one, or a new one from `base`.

`tasq worktree prune [--all] [--dry-run]` cleans up what closes left behind: a worktree
kept because it was dirty at the time, one of a task closed with `on_done = "keep"` or outside
tasq, a directory deleted by hand. It first runs `git worktree prune` in the current directory's
repository, so git forgets worktrees whose directory is gone (and their branches can be
deleted), then releases the worktrees of every done task working there with the rules above,
never with `--force`: a dirty worktree is named in a warning and kept, every time. `on_done`
does not apply; the command is the request. `--all`, or running outside a repository, covers
every repository a task declares worktrees in. `--dry-run` lists what would be forgotten,
removed and deleted.

`tasq worktree ls [ID] [--all]` lists declared worktrees with their task, status, branch and
state (`clean`, `<n> changed` not counting provisioned files, `not created`): of one task, of
the tasks working in the current directory's repository, or of every task with `--all` (or
outside a repository); a closed task's worktree shows only while it exists. `tasq worktree
switch [TARGET]` goes to `task:<id>` (its worktree, created first when missing), `^` (the main
checkout), `-` (where the last switch came from, kept in `$XDG_STATE_HOME/tasq/previous-worktree`),
a directory, or a branch (a worktree git lists for the current repository, else one a task
declares); without TARGET it offers the worktrees through `fzf`, or a numbered list when fzf is
not installed. A program cannot change its shell's directory, so `switch` needs the function
`tasq worktree shell-init bash|zsh|fish` prints (`eval "$(tasq worktree shell-init zsh)"` in
`~/.zshrc`, `tasq worktree shell-init fish | source` in fish): it runs tasq with
`TASQ_CD_FILE` set and changes directory to what `switch` wrote there. Without the function
`switch` prints the path with a hint. `tasq worktree path TARGET` prints the directory for
scripts and never creates anything.

```toml
worktrees_enabled = true       # top level, above every table

[worktree]
path = "{{project_parent}}/{{project_name}}-{{branch_safe}}"   # where a declared worktree lives
branch = "{{id}}-{{slug}}"     # branch of a worktree declared without one
base = "origin/HEAD"           # start point of a new branch
on_create = "auto"             # auto | never
on_done = "release"            # release | keep
delete_branch = "safe"         # safe | never
symlinks = ["node_modules"]    # linked from the project into a new worktree
copies = [".envrc"]            # copied from the project
templates = [{ source = "templates/.envrc.template", target = ".envrc" }]

[worktree.variables]           # extra {{name}} variables
db_prefix = "app"

[worktree.ports]               # {{port}}: a stable block per branch
base = 4000
span = 4000
block = 10

[worktree.hooks]               # command lines, where they run: table below
pre-create = []
post-create = ["direnv allow ."]
pre-remove = []
post-remove = []
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `worktree.path` | template | `{{project_parent}}/{{project_name}}-{{branch_safe}}` | Where a declared worktree lives. Rendered when it is declared and stored on the task. |
| `worktree.branch` | template | `{{id}}-{{slug}}` | Branch of a worktree declared without one. |
| `worktree.base` | string | `origin/HEAD` | Ref a new branch starts from. |
| `worktree.on_create` | `auto` \| `never` | `auto` | `auto` declares a worktree for every new task whose project is a git checkout; `never` only with `--worktree`. |
| `worktree.on_done` | `release` \| `keep` | `release` | Closing a task removes its clean worktrees (`release`) or leaves them (`keep`). |
| `worktree.delete_branch` | `safe` \| `never` | `safe` | On release, delete the branch when it is merged into the default branch or pushed (`safe`), or never. |
| `worktree.symlinks` | array of paths | `[]` | Paths in the main checkout linked into a new worktree, at the same relative path. |
| `worktree.copies` | array of paths | `[]` | Paths in the main checkout (files or directories) copied into a new worktree. |
| `worktree.templates` | array of `{source, target}` | `[]` | Files rendered into a new worktree; `target` relative to the worktree, a relative `source` to the directory of the file that set it (a profile's starts at `profiles/`, so `../` reaches the config directory), else to the main checkout when `--set` or the environment set it. |
| `worktree.variables` | table of strings | empty | Extra template variables. |
| `worktree.ports` | `{base, span, block}` | unset | Enables `{{port}}`: `base` plus a stable per-branch offset below `span`, a multiple of `block`. |
| `worktree.hooks.pre-create`, `post-create`, `pre-remove`, `post-remove` | arrays of command lines | `[]` | Templates, split into words and run without a shell (see above). `pre-create` and `post-remove` run in the main checkout, `post-create` and `pre-remove` in the worktree. A failing `pre-*` hook aborts the step; a failing `post-*` hook is a warning. |

`[worktree]` in `config.toml` or a profile holds the personal defaults; what belongs to one
repository goes in its repository config.

### Repository configs

A repository config is a file in `repos/` of the config directory with a `path`, the main
checkout of one repository (the working tree that holds its `.git` directory, not a worktree
added from it), and a `[worktree]` table. The file name is free:

```toml
# ~/.config/tasq/repos/acme.toml
path = "~/code/acme_workspace/acme"

[worktree]
symlinks = ["node_modules"]
templates = [{ source = "templates/acme.envrc", target = ".envrc" }]  # ~/.config/tasq/repos/templates/

[worktree.hooks]
post-create = ["direnv allow ."]
```

The worktree commands (`worktree up`/`release`/`ls`/`prune`/`switch`, and `create`, `next`,
`pick` and `done` when they declare, create or release a worktree) resolve the task's project to its
repository's main checkout and apply the repository config whose `path` is exactly that
directory, from whatever directory tasq runs in and whichever profile is selected. It is the
layer after the profile, so its values win over the profile's `[worktree]` defaults and lose
to the environment and `--set`. There is no walk up: a `path` naming a parent directory, a
subdirectory or a linked worktree never applies. Write `path` as the real directory, without
symlinks or `..`: the main checkout is found with them resolved, and `tasq doctor` warns about
a `path` that differs.

- `path` is required and must be absolute or start with `~/`.
- Any key other than `path` and `[worktree]` is an error, including `worktrees_enabled` and
  `enabled` under `[worktree]` (`worktrees_enabled is not a repository setting: set it in config.toml or
  a profile`).
- Every file in `repos/` is read on each worktree command, so a broken one fails them all, and
  two files with the same `path` are an error naming both.
- A relative template `source` starts at the repository config's directory (`repos/`).
- In a bare repository with linked worktrees, git names no main checkout, so each worktree is
  its own: a repository config whose `path` is one of them applies only to that worktree.
- Hooks run without asking, like every hook (ADR 0031).

`tasq worktree config [DIR]` shows the `[worktree]` table that applies to the repository `DIR`
(default: the current directory) is in, the layer that set each key and the repository config
that matched (`none` when no file did); `--json` gives `repository`, `repository_config`,
`worktree` and `origins`. With worktrees enabled, `tasq doctor` checks each repository config
(`repo-config`): `OK` when its `path` is a main checkout and the config loads with it applied,
`WARN` when it does not exist, is not inside a git repository, is a linked worktree or a
subdirectory, or reaches the main checkout through a symlink or `..`, with the `path` to set;
`FAIL` when the files do not load, or when a matching one does not load merged (an undefined
template variable, ...), as every worktree command for that repository would.

### Template variables

Worktree templates use the same `{{name}}` engine as the Claude prompt (`{{#name}}...{{/name}}`
sections included). What a template may name depends on when it is rendered:

| Variable | Value | `branch` | `path` | files and hooks |
|---|---|---|---|---|
| `id`, `title` | the task's id and title | yes | yes | yes |
| `slug` | the title in lowercase ASCII, other characters `-`, at most 40 bytes (`fix-the-login-form`) | yes | yes | yes |
| `project`, `project_name`, `project_parent` | the project directory (the repository's main checkout when the project is a linked worktree or a subdirectory of one), its name, its parent | yes | yes | yes |
| `repo` | the repository name from `remote.origin.url`, else `project_name` | yes | yes | yes |
| `branch` | the branch | | yes | yes |
| `branch_safe` | the branch with every character but ASCII letters and digits as `_`, runs collapsed, trimmed (`feature/x` is `feature_x`) | | yes | yes |
| `branch_db` | `branch_safe` in lowercase, at most 63 bytes | | yes | yes |
| `port` | with `[worktree.ports]`: the first port of the branch's block, stable for a project and branch | | yes | yes |
| `worktree`, `worktree_name` | the worktree directory and its name | | | yes |
| `[worktree.variables]` entries | as written | yes | yes | yes |

A template naming anything else is a configuration error with the key, the file and the names
it could use, whether or not worktrees are enabled. `[worktree.variables]` names are letters,
digits and underscores and may not reuse a built-in name. The contents of template files are
checked when they are rendered.

## Launchers

`tasq next` and `tasq pick <id>` set the task to in-progress and open a session in the first
tracked worktree that exists, else the task's `project`, else `work.default_project`.
`launch.default` (or `--launcher`) picks how:

- `claude`: runs `claude "<prompt>"` in that directory. With `launch.env = "direnv"` and an
  allowed `.envrc`, the command is wrapped in `direnv exec <dir>` so the session gets the
  directory's own environment; a `.envrc` that direnv has not allowed is reported with the
  `direnv allow` command to run. The prompt comes from `launch.claude.prompt_file` or the
  built-in template (`crates/launch/templates/claude.md`): `{{id}}`, `{{title}}`, `{{file}}`,
  `{{markdown}}`, `{{workdir}}`, `{{worktrees}}`, `{{sessions}}`, `{{statuses}}`, and
  `{{#name}}...{{/name}}` sections kept only when the variable is non-empty (`{{#herdr}}`
  inside herdr).
- `shell`: `exec $SHELL` in the directory.
- `tmux`: a new tmux window there (only inside tmux).
- `herdr`: a herdr workspace with a Claude agent and the prompt pasted in, or a tab in the
  workspace already holding the directory, as `launch.herdr.placement` says (only inside
  herdr); falls back to `claude` in the current pane when herdr cannot open one.
- `auto`: `herdr` when `HERDR_ENV` is set, else `claude`.

### Sessions in a new window

`tasq pick <id> --window` (and `tasq next --window`) opens the session in a new window
instead of the current terminal, with `launch.window` rather than `launch.default`: `herdr`
or `tmux`, or `auto` for whichever of the two the terminal runs in (`HERDR_ENV`, then
`TMUX`); outside both, `auto` is an error and nothing is written. `--launcher` overrides it as
usual. `--no-focus` opens the window in the background: herdr skips the focus step, tmux
passes `-d`. The TUI binds these to `Ctrl+Enter` (new window, switch to it) and `Shift+Enter`
(new window, stay), keeping the screen and showing the launcher's result in the status bar;
plain `Enter` stays "here, with `launch.default`". The two chords need a terminal that speaks
the kitty keyboard protocol and does not keep the chord for itself; elsewhere they arrive as a
plain `Enter`. Ghostty binds `Ctrl+Enter` to fullscreen on Linux: rebind the action
(`launch-window = "alt+enter"` under `[ui.keys]`, see "Key bindings") or free the chord with
`keybind = ctrl+enter=unbind` in its config.

### Debugging a launch

`-v` prints a trace of the launch on stderr: every `herdr` or `tmux` command the launcher ran,
shell-quoted, whether it succeeded, and its stdout and stderr. `launch.log` (or
`TASQ_LAUNCH_LOG`) appends the same trace to a file on every launch, under a
`--- <time> task <id> with <launcher> in <dir>` header and followed by the outcome
(`opened: ...` or `error: ...`) and any warnings; it is the way to see what happened to a
session opened from the TUI, which shows only the last line. A log file that cannot be written
is ignored. Warnings that were silent before now say why: a herdr pane that could not be
opened names herdr's error, a failed `focus` step is reported, and a failed `agent start`
lists the error of both names it tried. A new pane's shell takes a while to reach its prompt
(zsh with rc files, direnv and gitstatus: 100-300 ms, more on a fresh worktree) and herdr
refuses `agent start` with `agent_pane_busy` until then, so the launcher retries it every
100 ms for up to 5 s; the trace keeps only the attempt that got through (or the last one)
and adds `waited N ms for the pane's shell (K agent_pane_busy replies)`. A pane still busy
after 5 s fails the launch with herdr's message, the time waited and the pane's foreground
process from `herdr pane process-info` (`foreground: gitstatusd (pid 42)`), the thing to
look at when a shell hangs on startup.

The session's environment carries `TASQ_TASK_ID`, `TASQ_DATA_DIR` (resolved, absolute),
`TASQ_STORE_NAME` and, when in effect, `TASQ_PROFILE` and `TASQ_CONFIG_DIR`. `--dry-run` prints the directory, the commands and the prompt
without launching (with `--json`: `task`, `workdir`, `in_worktree`, `launcher`, `env`, `steps`)
and skips the `pre-launch` hooks.

## Hooks

`[hooks]` holds three arrays of command lines, run by the CLI around task events. Each command
is split like a shell command line (quotes allowed, no shell), `~` is expanded, and the command
reads a JSON document on stdin:

| Hook | When | Stdin | Failure |
|---|---|---|---|
| `post-create` | after `tasq create` (or the UI's `c` key) wrote the task | `{"schema":1,"hook":"post-create","task":{...}}` | warning on stderr |
| `post-done` | after `tasq done` (or the UI's `d` key) closed the task | `{"schema":1,"hook":"post-done","task":{...}}` | warning on stderr |
| `pre-launch` | before `tasq next`/`tasq pick` start a session | the same plus `"workdir"` and `"launcher"` | a non-zero exit aborts the launch |

`task` is the `Task` object of `docs/json.md`. The environment carries `TASQ_HOOK` (the hook
name), `TASQ_TASK_ID`, `TASQ_BIN` (the `tasq` binary to call back) and, when in effect,
`TASQ_PROFILE`, `TASQ_CONFIG_DIR` (absolute) and `TASQ_SET`, so a hook that runs `$TASQ_BIN` sees the same
configuration. `--dry-run` skips hooks. The terminal UI's `c` and `d` keys run the
`post-create` and `post-done` hooks as `tasq create` and `tasq done` do, and its `Enter` runs
`tasq pick`, so `pre-launch` fires. Plugins and a worked example: `docs/plugins.md`.

```toml
[hooks]
post-done = ["~/bin/tasq-tlogs"]
pre-launch = ["~/bin/check-vpn --quiet"]
```

## Plugins

`[plugins] enabled` lists the plugins tasq tells about events (ADR 0030): `store-open` when a
command or `tasq ui` opens the store, `post-write` after a command that wrote tasks, `doctor`
during `tasq doctor`. Each is run as `tasq-<name> hook` with a JSON payload on stdin; one that
fails or outlasts `plugins.timeout` is a warning, never a failed command. An enabled plugin
missing from `PATH` is a warning on every command that opens the store and a `FAIL` check in
`tasq doctor`.

```toml
[plugins]
enabled = ["git"]
timeout = 5

[plugin.git]
interval = 60
email = "me@example.com"
```

`[plugins]` is checked like every other table (an unknown key is an error with file, line and
column). `[plugin.<name>]` is not: tasq hands it to the plugin as the payload's `config`,
merged across layers key by key, so a profile can change one setting of a
plugin configured globally. `--set` and `TASQ_SET` reach it too (`--set
plugin.git.interval=30`); such a value takes the type the key has in the layers below, and is
a string when no layer set it. The rest (payloads, environment, lasting warnings, doctor
checks, `tasq store lock`): `docs/plugins.md`. The keys of `[plugin.git]` (`name`, `email`,
`ssh_key`, `interval`): `docs/git-sync.md`.

## Colours

`tasq list`, `tasq view` and the terminal UI (`tasq ui`) take their colours from one theme
(ADR 0018), built in three layers:

1. **`ui.theme.preset`**, a built-in table. `dark` (the default) is the original script:
   `in-progress` blue, `ready` green, `waiting` yellow, `blocked` red, `later` magenta, any
   other configured status cyan, the no-status and done groups dim, tag chips white on dark
   blue, `#A` red, the focus border cyan, the selection reversed, errors red, links light
   blue, overdue dates bold red, dates due today or tomorrow yellow. `light` uses darker shades and a real grey instead of faint text, which many
   light-background terminals cannot show. `solarized` and `gruvbox` are 256-colour
   approximations of the dark variants. `mono` has no colours: bold, dim and reversed only.
2. **`[ui.theme.colors]`**, one colour per role, over the preset.
3. **`[ui.colors]`**, one colour per status name, over both; `no-status` and `done` name the
   two groups that are not a status. This is where a status the roles do not know gets its
   colour (`review = "red"`).

```toml
[ui.theme]
preset = "light"

[ui.theme.colors]
focus = "208"            # a 256-colour palette index
selection = "236"        # a colour here is a background; "reversed" swaps the colours
chip-bg = "none"         # no colour: the chip becomes plain text in chip-fg
link = "blue"            # black red green yellow blue magenta cyan white

[ui.colors]
review = "cyan"          # a status the roles do not name
no-status = "dim"        # dim (grey/gray), reversed (reverse), none (plain/default)
done = "green"           # the DONE group of `tasq list --all` / `--done`
```

The roles: the eight groups `in-progress`, `ready`, `waiting`, `blocked`, `later`,
`other-status`, `no-status`, `done`; `chip-bg` and `chip-fg` for tag chips; `prio-a` for the
`#A` marker; `focus` for the focused field of the edit view and the calendar border;
`selection` for the selected row and the chosen option; `error` for error messages and a
refused field; `dim` for ids, dates, hints and calendar weekends; `link` for the hyperlinks of
`tasq view` and the link labels of the `tasq ui` detail (underlined there); `header` for titles and section headings (bold is always added); `overdue` for
the due date of an open task that is past (bold is always added) and `due-soon` for one due
today or tomorrow; later dates use `dim`.

A colour is a name, `dim`, `reversed`, `none` or a number from 0 to 255; anything else is
ignored, as is a role name that does not exist. An unknown preset is a config error.
`TASQ_THEME=light` sets the preset from the environment. `NO_COLOR` or `--color never` turns
every colour off in both front ends; bold, dim and reversed stay.

## Terminal UI

`tasq ui` is the grouped list, with the selected task's detail shown on `Right` and hidden on
`Left` (beside the list from 100 columns, in its place below that; `Tab` switches). Long rows
wrap at the pane width. Its edits are the same operations as
`tasq set`, `tasq log` and `tasq done`; `e` opens the edit view for the title, status, priority,
due date, project, tags and description (`Tab`/`Shift+Tab` and the arrows between rows,
`Left`/`Right` cycle the status and priority rows, `Home`/`End` or `Ctrl+A`/`Ctrl+E` go to the
ends of the line, `Ctrl+W`/`Ctrl+U`/`Ctrl+K` delete as in readline, `Enter` is the next row or a newline in the description, `Ctrl+S` saves,
`Esc` cancels). `Enter` on the Due box opens a calendar over the view: the arrows move by a
day and a week, `PageUp`/`PageDown` by a month, `t` jumps to today, `Enter` puts the day in the
box as ISO, `Esc` closes it; typing a date or `today`/`tomorrow` into the box works as before.
`ui.week_start` sets the grid's first column (`monday` by default).
`c` opens the same view empty, as `New task`, with `workflow.default_status` and the project
`tasq ui` was given; `Ctrl+S` creates the task. With worktrees enabled it has a Worktree box:
a branch there declares a worktree on it (`tasq create --worktree BRANCH`), empty leaves it to
`worktree.on_create`.
In the one-line prompts (the `/` filter, the `l` and `d` notes), `Up`/`Down` go
through what was submitted earlier in the session, like a shell's history: filters and notes
each have their own (`l` and `d` share the notes), going down past the newest brings back
what was being typed, and a recalled filter applies to the list at once. Only submitted, non-empty
entries count (not one closed with `Esc`); the history lives in memory and ends with `tasq ui`.
`E` opens the task file in `$VISUAL`, else `$EDITOR`,
else `vi`; `Enter`, `s` and `S` run `tasq pick <id>`, `tasq sync` and `tasq sync --source ...`
(the sources checked in the picker) as child processes with the same `--profile`, `--config-dir`
and `--set` flags, while the UI has released the terminal. `?` lists every key.

### Key bindings

Every key of `tasq ui` except `Ctrl+C` is an *action* with default keys. `[ui.keys]` replaces
the keys of an action: one key, a list of keys, or `[]` to unbind it. Actions that are not
listed keep their defaults, so the table is normally one or two lines:

```toml
[ui.keys]
launch-window = "alt+enter"              # Ghostty keeps Ctrl+Enter for fullscreen
launch-window-stay = ["shift+enter", "alt+shift+enter"]
sync = []                                  # s does nothing
```

A key is `[ctrl+][alt+][shift+]<key>`, modifiers in any order and case; `<key>` is one
character (`j`, `G`, `/`, `?`) or a name: `enter`, `esc`, `tab`, `backspace`, `space`, `up`,
`down`, `left`, `right`, `home`, `end`, `pgup`, `pgdn`, `del`, `ins`, `f1` to `f12`. `shift`
goes with a named key only; a shifted character is written as that character (`G`, not
`shift+g`), because that is what the terminal sends. Modifiers match exactly: `ctrl+enter` is
not `ctrl+shift+enter`. `--set ui.keys.<action>=k1,k2` takes a comma-separated list.

| Action | Default | Where | Does |
|---|---|---|---|
| `up`, `down` | `k`, `up` / `j`, `down` | list, pickers | move the selection or the cursor |
| `page-up`, `page-down` | `ctrl+u`, `pgup` / `ctrl+d`, `pgdn` | list | move ten tasks |
| `top`, `bottom` | `g`, `home` / `G`, `end` | list | first / last task |
| `move-up`, `move-down` | `ctrl+k` / `ctrl+j` | list | move the selected task before the task above it / after the task below it in its status group (`tasq move --before` / `--after` the task listed next to it, so in a filtered view it lands next to its visible neighbour); the selection follows the task. At the ends of a group, on a folded group's header and in the DONE group nothing moves and the status bar says why. The rank is never shown ([ADR 0025](adr/0025-manual-rank-within-a-status-group.md)) |
| `toggle-group` | `z`, `space` | list | fold the selected task's group to its header, or unfold the folded group under the cursor (kept for the session) |
| `toggle-today` | `T` | list | the Today view: open tasks in the workflow's first status (`in-progress`) or due today or earlier, across statuses; with `toggle-done`, the tasks closed today (`+today` in the list title) |
| `toggle-done` | `a` | list | show or hide the DONE group: done tasks matching the filter, newest closed first, unfolded whenever it appears (`+done` in the list title) |
| `filter` | `/` | list | type a filter |
| `status`, `priority` | `t` / `p` | list | open the status / priority picker; on a done task (DONE group) the status picker reopens it (`tasq reopen`), starting at `workflow.default_status` |
| `log`, `done` | `l` / `d` | list | type a note / a final note |
| `create` | `c` | list | open the create view (the edit view's fields, empty; a Worktree box with worktrees enabled) |
| `edit` | `e` | list | open the edit view (title, status, priority, due, project, tags, description) |
| `editor` | `E` | list | open the file in the editor |
| `view` | `v` | list | `tasq view <id>`: the task rendered by glow, in the pager. With less (`ui.pager`), it runs with `-+F -+X` so it stays open full screen and `q` returns; with another pager, a key returns after it |
| `launch` | `enter` | list | `tasq pick <id>` here |
| `launch-window` | `ctrl+enter` | list | `tasq pick <id> --window` |
| `launch-window-stay` | `shift+enter` | list | `tasq pick <id> --window --no-focus` |
| `sync`, `sources` | `s` / `S` | list | `tasq sync` (the `auto = true` sources) / the source picker |
| `reload` | `r` | list | reload |
| `help` | `?` | list | the key overlay (any key closes it) |
| `toggle-detail` | `tab` | list | switch between list and detail |
| `show-detail`, `hide-detail` | `right` / `left` | list | show / hide the selected task's detail |
| `detail-up`, `detail-down` | `K`, `ctrl+y` / `J`, `ctrl+e` | list | scroll the detail a line; `page-up`/`page-down` page it when it replaces the list |
| `cancel` | `esc` | list, pickers | clear the filter, close the detail or the dialog |
| `confirm` | `enter` | pickers | apply the choice; in the source picker, run the checked sources |
| `quit` | `q` | list, pickers | leave `tasq ui`; in a picker, close it |

Typing (the filter, a note, a title, the edit view) is not configurable: characters, `Enter`,
`Esc`, `Backspace` and `Delete` do what they always do, `Left`/`Right` move the cursor and
`Home`/`End` or `Ctrl+A`/`Ctrl+E` go to the ends of the line, and `Ctrl+W`, `Ctrl+U` and
`Ctrl+K` delete the word before the cursor, back to the start of the line and on to its end,
as in readline (on the current line only: they never join lines), so `move-up`'s default
`ctrl+k` only moves a task in the list. In the one-line prompts
`Up`/`Down` recall earlier entries of the session; in the edit view they move between rows.
An input longer than the status bar scrolls to keep the cursor in view. Neither are the toggles of the source picker: `Space` and
the row digits. `Ctrl+C` quits in every mode and cannot be rebound. A key
bound to two actions of the same mode, an unknown action or a key that does not parse stops
`tasq ui` at startup with the file and the `ui.keys.<action>` path. The `?` overlay and the
status-bar hints show the configured keys.

## Examples

Complete, commented files in `examples/config/`: `plain-markdown.toml` for a plain setup
(the default store, shell launcher, raw summaries, no sources) and `author.toml` for a GitLab
plus worktree plus Claude Code plus hooks setup. Sources alone: `examples/sources/config.toml`.
