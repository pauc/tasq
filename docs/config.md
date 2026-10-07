# Configuration

`tasq` reads TOML configuration in layers; later layers override earlier ones field by field:

1. Built-in defaults (below).
2. Global file: `$XDG_CONFIG_HOME/tasq/config.toml`, else `~/.config/tasq/config.toml`.
   `TASQ_CONFIG=<file>` or `--config <file>` replaces it (the file must then exist).
3. Project file: the nearest `.tasq.toml` walking up from the current directory (stopping at
   your home directory; nothing above home is read).
4. `[profile.<name>]` blocks from the loaded files, when selected with `--profile` or `TASQ_PROFILE`.
5. Environment variables (table below), including `TASQ_SET`.
6. `--set key=value` overrides.

Tables deep-merge; scalars and arrays replace (a later `[[source]]` list replaces the whole list).
Unknown keys are errors naming the file, line and column. `tasq config show` prints the effective
config and which layer set each value.

Values given through the environment or `--set` are converted to the key's type:
`true`/`false`/`yes`/`no`/`1`/`0`/`on`/`off` for booleans, a comma-separated list for arrays,
text otherwise. A key that does not exist is an error.

## Defaults

Serialising the default configuration gives this document. Keys whose default is unset
(`work.default_project`, `work.worktree_command`, `launch.claude.prompt_file`,
`report.summary.model`, `report.summary.prompt_file`) are shown commented out.

```toml
[store]
kind = "nb"                  # the only store today
notebook = "home"
bookkeeper = "auto"          # auto | nb | native

[workflow]
statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
default_status = "ready"

[work]
worktree_manager = "git"     # git | command
# worktree_command = "gwm create {new} {branch} --no-tmux -s"   # required with "command"
# default_project = "~/code/..."

[launch]
default = "claude"           # auto | claude | shell | tmux | herdr
detached = "auto"            # auto | tmux | herdr   (--detached, the TUI's Ctrl/Shift+Enter)
env = "direnv"               # inherit | direnv

[launch.claude]
# prompt_file = "~/.config/tasq/prompts/claude.md"

[launch.herdr]
placement = "auto"           # auto | workspace | tab

[ui]
pager = "less -RFX"
no_osc8 = false
glow_style = "dark"
week_start = "monday"        # first column of the calendar picker: monday .. sunday
due_format = "relative"      # relative (overdue 3d) | iso (due 2026-10-03) | both

[ui.colors]                  # status name (or "no-status") = colour, see "Colours"

[ui.keys]                    # action = key or list of keys, [] unbinds; see "Key bindings"
# launch-detached = "alt+enter"

[ui.theme]
preset = "dark"              # dark, light, solarized, gruvbox or mono; see "Colours"

[ui.theme.colors]            # role = colour (chip-bg, focus, link, ...); see "Colours"

# [forge.gitlab]            # kind and host inferred from the name when omitted
# kind = "gitlab"           # gitlab | github; required when the block name is neither
# host = "gitlab.example.com"
# token_cmd = "glab auth token"      # else GITLAB_TOKEN / GITHUB_TOKEN
# url = "https://gitlab.example.com/api/v4"   # API base; defaults from host

# [[source]]
# name = "gitlab-review-requests"
# kind = "gitlab-review-requests"   # gitlab-review-requests | gitlab-work-items |
#                                   # github-review-requests | github-work-items | llm-bridge
# forge = "gitlab"                  # forge-backed kinds
# command = "claude -p --output-format json"   # llm-bridge only
# prompt_file = "~/.config/tasq/prompts/inbox.md"  # llm-bridge only
# tags = ["gitlab", "review-request"]
# status = "ready"
# enabled = true
# auto = true                       # false: a bare `tasq sync` skips it; run it with --source or from the TUI
# create_new = true                 # false: only update tasks that already exist
# close_when_done = true            # mark the task done when the item is merged/closed/approved
# flag = "review-request"           # tag added to matched open tasks that lack it
# title = "Review MR !{iid}: {title}"  # {title} {iid} {project}
# labels = ["team::core"]           # work items: include only these labels
# exclude_labels = ["wontfix"]      # work items: skip these labels
# projects = ["group/", "owner/repo"]  # allow-list; a trailing / matches a group

[report.summary]
summarizer = "llm"           # raw | llm
command = "claude -p"        # reads the prompt (instructions + notes) on stdin
# model = "sonnet"           # appended as --model <model>
# prompt_file = "~/.config/tasq/prompts/summary.md"  # {{day}} {{date}} {{notes}}

[hooks]
post-create = []             # command lines, see "Hooks"
post-done = []
pre-launch = []
```

## Every key

| Key | Type | Default | Meaning |
|---|---|---|---|
| `store.kind` | `nb` | `nb` | Store implementation. Only nb-compatible notebooks exist. |
| `store.notebook` | string | `home` | nb notebook name: `$NB_DIR/<name>` (default `~/.nb/<name>`) when that directory exists, else `nb notebooks show <name> --path`. |
| `store.bookkeeper` | `auto` \| `nb` \| `native` | `auto` | Who maintains `.index` and git commits after a write. `auto` is `nb` when it is on `PATH`, else `native`. |
| `workflow.statuses` | array of strings | `["in-progress", "ready", "waiting", "blocked", "later"]` | Statuses in display and `next` search order. Lowercase kebab-case. |
| `workflow.default_status` | string | `ready` | Status of new tasks. Must be one of `statuses`. |
| `work.default_project` | path | unset | Directory a session starts in when the task tracks neither a worktree nor a project. `~` expanded. |
| `work.worktree_manager` | `git` \| `command` | `git` | How `tasq worktree --create` makes a worktree. |
| `work.worktree_command` | string | unset | Template run by the `command` manager; required with it. See "Worktree managers". |
| `launch.default` | string | `claude` | Launcher for `next`/`pick`: `auto`, `claude`, `shell`, `tmux`, `herdr`. Not validated at load time. |
| `launch.detached` | string | `auto` | Launcher for `next`/`pick --detached` (the TUI's `Ctrl+Enter` and `Shift+Enter`): `herdr`, `tmux`, or `auto` for whichever the terminal runs in; an error when neither. See "Launchers". |
| `launch.env` | `inherit` \| `direnv` | `direnv` | Where the session's environment comes from. |
| `launch.claude.prompt_file` | path | unset | Prompt template replacing the built-in `crates/launch/templates/claude.md`. `~` expanded. |
| `launch.herdr.placement` | `auto` \| `workspace` \| `tab` | `auto` | What a herdr session opens: `auto` a tab in the workspace already holding the directory, else a workspace; `workspace` always a new workspace; `tab` always a tab (in the holding workspace, else the current one). |
| `ui.pager` | string | `less -RFX` | Pager for long output on a terminal, split without a shell. `cat` or empty disables it. |
| `ui.no_osc8` | bool | `false` | Disable OSC 8 hyperlinks in `tasq view`. |
| `ui.glow_style` | string | `dark` | Style passed to `glow -s`. |
| `ui.week_start` | weekday name | `monday` | First column of the calendar picker in `tasq ui` (`monday`, `tuesday`, ... `sunday`). |
| `ui.due_format` | `relative` \| `iso` \| `both` | `relative` | How the rows of `tasq list` and `tasq ui` show a due date: `overdue 3d`, `due today`, `due tomorrow`, `due in 4d`; `due 2026-10-03`; or `overdue 3d, 2026-10-03`. The detail pane of `tasq ui` always shows both, `--json` always the ISO date. A done task always shows its ISO date, dim. |
| `ui.colors.<name>` | string | empty table | Colour per status name, plus `no-status` and `done`, on top of the theme. See "Colours". |
| `ui.theme.preset` | `dark` \| `light` \| `solarized` \| `gruvbox` \| `mono` | `dark` | The built-in colour theme of `tasq list`, `tasq view` and `tasq ui`. See "Colours". |
| `ui.theme.colors.<role>` | string | empty table | Colour per role (`in-progress`, `ready`, `waiting`, `blocked`, `later`, `other-status`, `no-status`, `done`, `chip-bg`, `chip-fg`, `prio-a`, `focus`, `selection`, `error`, `dim`, `link`, `header`, `overdue`, `due-soon`), over the preset. See "Colours". |
| `ui.keys.<action>` | string or array of strings | empty table | Keys of a `tasq ui` action, replacing its defaults; `[]` unbinds it. See "Key bindings". |
| `forge.<name>.kind` | `gitlab` \| `github` | inferred from `<name>` | API the host speaks. Required when the block is not called `gitlab` or `github`. |
| `forge.<name>.host` | string | `gitlab.com` / `github.com` by kind | Host without scheme. |
| `forge.<name>.token_cmd` | string | unset | Command whose stdout is the token. Unset: `GITLAB_TOKEN` / `GITHUB_TOKEN`. |
| `forge.<name>.url` | string | `https://<host>/api/v4` (GitLab), `https://api.github.com` (github.com), `https://<host>/api/v3` (other GitHub) | API base URL override. |
| `source[].name` | string | required | Unique name, recorded in each task's `## Source` line. |
| `source[].kind` | string | required | `gitlab-review-requests`, `gitlab-work-items`, `github-review-requests`, `github-work-items`, `llm-bridge`. |
| `source[].forge` | string | unset | `[forge.<name>]` to use. Required by the forge-backed kinds; its kind must match. |
| `source[].command` | string | unset | Command run by `llm-bridge`. Required by it. |
| `source[].prompt_file` | path | unset | File fed to the `llm-bridge` command on stdin. `~` expanded. |
| `source[].tags` | array of strings | `[]` | Tags added to every task the source creates. |
| `source[].status` | string | `workflow.default_status` | Status of tasks the source creates. |
| `source[].enabled` | bool | `true` | Whether the source can run at all. `false`: never, and naming it is an error. |
| `source[].auto` | bool | `true` | Whether a bare `tasq sync` (and the TUI's `s`) includes it. `false`: it runs only when named with `--source` or checked in the TUI's `S` picker. |
| `source[].create_new` | bool | `true` | Create tasks for new items. `false`: only update existing tasks. |
| `source[].close_when_done` | bool | `true` | Log a note and mark the task done when its item is merged, closed, approved or reassigned. |
| `source[].flag` | string | unset | Tag added to matched open tasks that lack it. |
| `source[].title` | string | per kind | Title template: `{title}`, `{iid}`, `{project}`. Defaults `Review MR !{iid}: {title}`, `Review PR #{iid}: {title}`, `#{iid}: {title}`. |
| `source[].labels` | array of strings | `[]` | Work items: only those carrying one of these labels. |
| `source[].exclude_labels` | array of strings | `[]` | Work items: skip those carrying one of these labels. |
| `source[].projects` | array of strings | `[]` (every project) | `group/project`, a group prefix with a trailing `/`, or `owner/repo`. |
| `report.summary.summarizer` | `raw` \| `llm` | `llm` | Print the notes, or distil them with `command`. |
| `report.summary.command` | string | `claude -p` | Reads the rendered prompt on stdin, prints the summary. |
| `report.summary.model` | string | unset | Appended as `--model <model>`. |
| `report.summary.prompt_file` | path | unset | Template replacing the built-in `crates/launch/templates/summary.md`; `{{day}}`, `{{date}}`, `{{notes}}`. `~` expanded. |
| `hooks.post-create` | array of strings | `[]` | Command lines run after `tasq create`. |
| `hooks.post-done` | array of strings | `[]` | Command lines run after `tasq done`. |
| `hooks.pre-launch` | array of strings | `[]` | Command lines run before `tasq next`/`tasq pick` start a session. |
| `profile.<name>.*` | table | none | A partial configuration (any key above) applied when the profile is selected. Profiles do not nest. |

Source of truth: `crates/core/src/config/mod.rs`.

## Environment variables

| Variable | Effect |
|---|---|
| `TASQ_CONFIG` | replaces the global file (must exist) |
| `TASQ_PROFILE` | selects `[profile.<name>]` |
| `TASQ_SET` | newline-separated `key=value` overrides, applied like `--set` (any key); part of the env layer, wins over the `TASQ_*` variables below for the same key, loses to `--set` flags. `tasq` sets it for the plugins and hooks it runs, so they see the same `--set` overrides |
| `TASQ_NOTEBOOK` | `store.notebook` |
| `TASQ_BOOKKEEPER` | `store.bookkeeper` |
| `TASQ_DEFAULT_PROJECT` | `work.default_project` |
| `TASQ_WORKTREE_MANAGER` | `work.worktree_manager` |
| `TASQ_WORKTREE_COMMAND` | `work.worktree_command` |
| `TASQ_LAUNCHER` | `launch.default` |
| `TASQ_LAUNCH_DETACHED` | `launch.detached` |
| `TASQ_LAUNCH_ENV` | `launch.env` |
| `TASQ_HERDR_PLACEMENT` | `launch.herdr.placement` |
| `TASQ_PAGER` | `ui.pager` |
| `TASQ_NO_OSC8` | `ui.no_osc8` (`1/true/yes/on`, `0/false/no/off`) |
| `TASQ_GLOW_STYLE` | `ui.glow_style` |
| `TASQ_WEEK_START` | `ui.week_start` |
| `TASQ_DUE_FORMAT` | `ui.due_format` |
| `TASQ_THEME` | `ui.theme.preset` |
| `TASQ_SUMMARIZER` | `report.summary.summarizer` |
| `TASQ_SUMMARY_MODEL` | `report.summary.model` |
| `TASQ_SUMMARY_COMMAND` | `report.summary.command` |
| `TASQ_SUMMARY_PROMPT_FILE` | `report.summary.prompt_file` |

Empty values count as unset. `XDG_CONFIG_HOME` is honoured for the global file location.

Variables that are not configuration: `NO_COLOR` turns colour off; `NB_DIR` is where notebooks
live (nb's own variable); `TASQ_NOW="YYYY-MM-DD HH:MM"` fixes the clock (see
`docs/testing.md`); `TASQ_TASK_ID`, `TASQ_NOTEBOOK` and `TASQ_PROFILE` are set in launched
sessions; `TASQ_BIN`, `TASQ_HOOK` and `TASQ_TASK_ID` are set for plugins and hooks (see
`docs/plugins.md`).

## Validation

- `workflow.default_status` must be one of `workflow.statuses`.
- `work.worktree_manager = "command"` needs a non-empty `work.worktree_command`.
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

## Launchers

`tasq next` and `tasq pick <id>` set the task to in-progress and open a session in the first
tracked worktree that exists, else the task's `## Project`, else `work.default_project`.
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

### Detached sessions

`tasq pick <id> --detached` (and `tasq next --detached`) opens the session in a new window
instead of the current terminal, with `launch.detached` rather than `launch.default`: `herdr`
or `tmux`, or `auto` for whichever of the two the terminal runs in (`HERDR_ENV`, then
`TMUX`); outside both, `auto` is an error and nothing is written. `--launcher` overrides it as
usual. `--no-focus` opens the window in the background: herdr skips the focus step, tmux
passes `-d`. The TUI binds these to `Ctrl+Enter` (new window, switch to it) and `Shift+Enter`
(new window, stay), keeping the screen and showing the launcher's result in the status bar;
plain `Enter` stays "here, with `launch.default`". The two chords need a terminal that speaks
the kitty keyboard protocol and does not keep the chord for itself; elsewhere they arrive as a
plain `Enter`. Ghostty binds `Ctrl+Enter` to fullscreen on Linux: rebind the action
(`launch-detached = "alt+enter"` under `[ui.keys]`, see "Key bindings") or free the chord with
`keybind = ctrl+enter=unbind` in its config.

The session's environment carries `TASQ_TASK_ID`, `TASQ_NOTEBOOK` and, when a profile is
selected, `TASQ_PROFILE`. `--dry-run` prints the directory, the commands and the prompt
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
`TASQ_PROFILE`, `TASQ_CONFIG` and `TASQ_SET`, so a hook that runs `$TASQ_BIN` sees the same
configuration. `--dry-run` skips hooks. Hooks run from the CLI only: the terminal UI's `d` key
edits through the core and fires no hook, while its `Enter` runs `tasq pick`, so `pre-launch`
fires. Plugins and a worked example: `docs/plugins.md`.

```toml
[hooks]
post-done = ["~/bin/tasq-tlogs"]
pre-launch = ["~/bin/check-vpn --quiet"]
```

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
`tasq view`; `header` for titles and section headings (bold is always added); `overdue` for
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
In the one-line prompts (the `/` filter, the `l` and `d` notes, the `c` title), `Up`/`Down` go
through what was submitted earlier in the session, like a shell's history: filters, notes and
titles each have their own (`l` and `d` share the notes), going down past the newest brings back
what was being typed, and a recalled filter applies to the list at once. Only submitted, non-empty
entries count (not one closed with `Esc`); the history lives in memory and ends with `tasq ui`.
`E` opens the task file in `$VISUAL`, else `$EDITOR`,
else `vi`; `Enter`, `s` and `S` run `tasq pick <id>`, `tasq sync` and `tasq sync --source ...`
(the sources checked in the picker) as child processes with the same `--profile`, `--config`
and `--set` flags, while the UI has released the terminal. `?` lists every key.

### Key bindings

Every key of `tasq ui` except `Ctrl+C` is an *action* with default keys. `[ui.keys]` replaces
the keys of an action: one key, a list of keys, or `[]` to unbind it. Actions that are not
listed keep their defaults, so the table is normally one or two lines:

```toml
[ui.keys]
launch-detached = "alt+enter"              # Ghostty keeps Ctrl+Enter for fullscreen
launch-detached-stay = ["shift+enter", "alt+shift+enter"]
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
| `toggle-group` | `z`, `space` | list | fold the selected task's group to its header, or unfold the folded group under the cursor (kept for the session) |
| `toggle-done` | `a` | list | show or hide the DONE group: done tasks matching the filter, newest closed first, unfolded whenever it appears (`+done` in the list title) |
| `filter` | `/` | list | type a filter |
| `status`, `priority` | `t` / `p` | list | open the status / priority picker |
| `log`, `done`, `create` | `l` / `d` / `c` | list | type a note / a final note / a title |
| `edit` | `e` | list | open the edit view (title, status, priority, due, project, tags, description) |
| `editor` | `E` | list | open the file in the editor |
| `launch` | `enter` | list | `tasq pick <id>` here |
| `launch-detached` | `ctrl+enter` | list | `tasq pick <id> --detached` |
| `launch-detached-stay` | `shift+enter` | list | `tasq pick <id> --detached --no-focus` |
| `sync`, `sources` | `s` / `S` | list | `tasq sync` (the `auto = true` sources) / the source picker |
| `reload` | `r` | list | reload |
| `help` | `?` | list | the key overlay (any key closes it) |
| `toggle-detail` | `tab` | list | switch between list and detail |
| `show-detail`, `hide-detail` | `right` / `left` | list | show / hide the selected task's detail |
| `cancel` | `esc` | list, pickers | clear the filter, close the detail or the dialog |
| `confirm` | `enter` | pickers | apply the choice; in the source picker, run the checked sources |
| `quit` | `q` | list, pickers | leave `tasq ui`; in a picker, close it |

Typing (the filter, a note, a title, the edit view) is not configurable: characters, `Enter`,
`Esc`, `Backspace` and `Delete` do what they always do, `Left`/`Right` move the cursor and
`Home`/`End` or `Ctrl+A`/`Ctrl+E` go to the ends of the line, and `Ctrl+W`, `Ctrl+U` and
`Ctrl+K` delete the word before the cursor, back to the start of the line and on to its end,
as in readline (on the current line only: they never join lines). In the one-line prompts
`Up`/`Down` recall earlier entries of the session; in the edit view they move between rows.
An input longer than the status bar scrolls to keep the cursor in view. Neither are the toggles of the source picker: `Space` and
the row digits. `Ctrl+C` quits in every mode and cannot be rebound. A key
bound to two actions of the same mode, an unknown action or a key that does not parse stops
`tasq ui` at startup with the file and the `ui.keys.<action>` path. The `?` overlay and the
status-bar hints show the configured keys.

## Examples

Complete, commented files in `examples/config/`: `plain-markdown.toml` for a setup without nb
(native bookkeeper, shell launcher, raw summaries, no sources) and `author.toml` for a GitLab
plus worktree plus Claude Code plus hooks setup. Sources alone: `examples/sources/config.toml`.
