# Plugins

`tasq` is extended from outside the binary (ADR-0006): a plugin is an executable named
`tasq-<name>` on `PATH`, written in any language, that talks to `tasq` through its `--json`
output and its editing commands. A plugin enabled in the configuration is also told about
events (ADR 0030): the store being opened, a command having written tasks, `tasq doctor`
running. Hooks are command lines in the configuration that `tasq` runs around three other
moments. Nothing is linked into the binary; the versioned JSON documents in `docs/json.md` and
the event payloads below are the whole API.

This page is normative: a plugin can be written from it alone.
`examples/plugins/tasq-echo` (bash builtins only) is the smallest plugin that follows it: it
appends every event it is told about to a log file.

## Running a plugin

```sh
tasq tlogs last week          # runs tasq-tlogs with the arguments "last week"
tasq --profile work tlogs     # the plugin sees the work profile too
tasq plugins list             # what is on PATH, and the configured hooks
```

`tasq <name> [args...]` runs `tasq-<name>` when:

1. `<name>` is not a built-in command or alias (`list`, `ls`, `create`, `plugins`, ... and
   `help`). Built-ins always win; a plugin called `tasq-list` or `tasq-ls` is never run.
2. An executable file `tasq-<name>` exists in a `PATH` entry (the first one wins).

Otherwise the command line is parsed as usual, and an unknown name is a usage error
(`unrecognized subcommand`). Filters always go through `tasq list <word>` (or `tasq ls
<word>`), so a plugin named after a status or a tag shadows nothing.

The plugin receives the arguments after its name verbatim (`tasq tlogs --json x` runs
`tasq-tlogs --json x`) and replaces the `tasq` process, so its exit code, stdin and streams are
the user's. Global flags before the name belong to `tasq` and are forwarded as environment
variables:

| Variable | Set when | Value |
|---|---|---|
| `TASQ_BIN` | always | path of the `tasq` binary that ran the plugin |
| `TASQ_PROFILE` | `--profile` given (or already in the environment) | the profile name |
| `TASQ_CONFIG_DIR` | `--config-dir` given (or already in the environment) | absolute path of the config directory |
| `TASQ_SET` | `--set` given (or already in the environment) | `key=value` overrides, one per line, existing entries first |

`tasq` itself reads the same variables (`docs/config.md`), so `"$TASQ_BIN" list --json` inside
the plugin resolves the same store, profile and overrides as the command that started it.
`--json`, `--color`, `--no-pager` and `-v` before the name are not forwarded.

Dispatch happens before the configuration is loaded: a plugin runs even when the config is
broken, and finds out through its own `tasq` calls.

## Writing a plugin

Read with `--json`, write through the commands:

| Need | Command |
|---|---|
| open tasks in display order | `tasq list --json` (filters: `--status`, `--tag`, `--prio`, `--text`) |
| done tasks too, or only them | `tasq list --all --json` (done tasks last), `tasq list --done --json` |
| one task | `tasq view <id> --json` |
| a day's progress notes per task | `tasq summary --json --raw [DAY]` |
| resolve a date range | `tasq dates --json [SPEC]` (`from`, `to`, `days`, `working_days`) |
| the effective configuration | `tasq config show --json` |
| create / edit / close | `tasq create ... --json`, `tasq set`, `tasq log`, `tasq done`, `tasq reopen`, `tasq mr`, `tasq session`, `tasq worktree`, `tasq project` |
| arbitrary edit of a task | `tasq view <id> --json`, change the `task`, `tasq apply` (refuses what the store cannot express) |
| rewrite task files directly | `tasq store lock -- <cmd>` (see "The store lock") |
| follow the store | enable the plugin and handle its events (see "Enabling a plugin") |

Every document carries `"schema": 1`; check it and refuse anything else. Exit codes: `0`,
`1` for an error you can fix (message on stderr), `2` for a usage error.

### The reference plugin: `tasq-tlogs`

`examples/plugins/tasq-tlogs` (bash and jq) is the author's time-log tool, the replacement for
`tasks tlogs`. `tasq tlogs [SPEC...]` resolves the range with `tasq dates --json`, reads
`work.default_project` and `launch.env` from `tasq config show --json`, and replaces itself
with a Claude session in that directory:

```
$ tasq tlogs last week --dry-run
cd /home/me/code/app && direnv exec /home/me/code/app claude /time-logs\ 2026-09-28\ 2026-10-02
```

The session runs `/time-logs <from> <to>`, a personal skill that is not shipped here (it reads
HiBob, GitLab activity, Google Calendar and Slack, proposes a breakdown per day and posts it
with `/spend`). Write your own under that name, or edit the prompt in the script. The command
is wrapped in `direnv exec` when `launch.env` is `direnv`, `direnv` is on `PATH` and the
directory's `.envrc` is allowed (a `.envrc` that direnv refuses is a warning on stderr and
a bare `claude`). `work.default_project` unset or missing is exit 1 with the key to set.

`--propose` launches nothing: for every working day of the range it lists the tasks that
received a progress note (`tasq summary --json --raw <day>`) and splits `TLOGS_HOURS` (default
8) evenly across them, a starting point for whatever posts the entries:

```
$ tasq tlogs --propose last week
2026-09-28 (Monday)
  [12] Rewrite the tasks script in Rust  6h  (3 notes)
  [15] Review MR !77: Faster index         2h  (1 note)
2026-09-29 (Tuesday)
  nothing logged
```

With `--propose --json` the same data is `{"schema": 1, "from", "to", "days": [{day,
weekday, tasks: [{id, title, notes, hours}]}]}`. To install the plugin: copy or symlink the
file somewhere on `PATH`.

The CLI integration test `plugins::example_tlogs_plugin_runs_through_dispatch` runs this
script through the dispatcher against the fixture store: `--propose` in both formats,
`--dry-run` with `--set work.default_project=...`, and the error without a directory.

## Enabling a plugin

A plugin on `PATH` can always be run as `tasq <name>`. To be told about events it must also be
enabled, by name, in any configuration layer:

```toml
[plugins]
enabled = ["git"]     # tasq-git, told about every event; in this order
timeout = 2           # seconds a plugin may take over one event (default 2)

[plugin.git]          # the plugin's own settings, passed to it as is
interval = 60
remote = "origin"
```

- `[plugins]` is tasq's: an unknown key there is an error with file, line and column, like
  any other key. A name in `enabled` must not be empty, contain `/` or whitespace, be `.` or
  `..`, start with `-`, be a built-in command or alias (`help` included) or be listed twice;
  such a name fails every command, `tasq config show` included, with the file that set it.
  `tasq doctor` reports each built-in name as a `FAIL` check named `plugins` and runs its
  other checks without them.
- `[plugin.<name>]` is the plugin's: tasq checks nothing under it and hands the whole table to
  the plugin in every payload (`config`). It is layered like the rest of the configuration:
  `config.toml`, a profile, `TASQ_SET` and `--set plugin.git.interval=30`
  apply key by key. A value from `TASQ_SET` or `--set` takes the type the key has in the
  layers below it (`interval = 60` makes `--set plugin.git.interval=30` an integer, `soon`
  an error), and is a string when no layer set the key.
- `--set plugins.enabled=git,echo` replaces the list; `--set plugins.timeout=5` the timeout.
  There is no `TASQ_PLUGINS_*` variable.

An enabled plugin that is not on `PATH` never fails a command. Every command that opens the
store warns on stderr:

```
tasq: warning: plugin git is enabled but tasq-git is not on PATH
```

`tasq doctor` reports it as a `FAIL` check named `tasq-git`, fixed by "install tasq-git or
remove it from [plugins] enabled", and no event is sent to it. `tasq plugins list` and
`tasq store info` list the enabled plugins (see below and `docs/json.md`).

## Events

For every event, tasq runs each installed enabled plugin, one at a time in `enabled` order, as

```
tasq-<name> hook
```

with the event's payload on stdin (one JSON document, no trailing newline), and waits for it.

| Event | Sent | Payload adds |
|---|---|---|
| `store-open` | once per command that opens the store, after the store's own warnings; once when `tasq ui` starts and again on each `r` | nothing |
| `post-write` | once per command that wrote at least one task, after its last write and its output; in `tasq ui`, once per action that wrote | `command`, `writes` |
| `doctor` | once per `tasq doctor`; the plugin answers with checks on stdout (see "Doctor checks") | nothing |

Commands that never open the store send nothing: `config`, `completions`, `dates`,
`plugins list`, `store lock`, plugin dispatch (`tasq <name>`). `doctor` sends `doctor`, not
`store-open`. The automatic reload of `tasq ui` (see "The terminal UI") sends nothing, so a
plugin is not woken by the files its own background job changed.

### The process

| | |
|---|---|
| Arguments | `hook`, nothing else. A plugin that also serves `tasq <name> ...` must tell the two apart by this first argument. |
| Working directory | `tasq`'s own. |
| Environment | the environment `tasq` was started with, plus `TASQ_BIN` (the running `tasq`), `TASQ_PROFILE`, `TASQ_CONFIG_DIR` and `TASQ_SET` when in effect (as for dispatch, above), and `TASQ_EVENT` (the event name). |
| Process group | its own; on a timeout the whole group is killed. The plugin is reaped only after that, so its group id cannot have been reused. A Ctrl-C at the terminal reaches `tasq`'s group, not the plugin's, so `tasq` passes it on: the plugin's group gets SIGINT, has until the timeout to be over (then, or on a second Ctrl-C, it is killed like a timeout), is reaped, and `tasq` exits 130 without running anything more (no further plugin, and for `store-open` not the command). Outside an event Ctrl-C ends `tasq` as usual. |
| stdin | the payload, then end of file. The plugin's group is not the terminal's foreground group, so a plugin that opens `/dev/tty` is stopped until the timeout kills it; a Ctrl-C meanwhile cannot reach a stopped plugin, so it too waits for the timeout. |
| stdout | shown at `-v` as `tasq: plugin <name>: <event>: <output>`; for `doctor`, the answer. |
| stderr | shown only in the warning when the plugin fails. |
| Exit code | `0` is success. Anything else is a warning; the command's own exit code never changes. |
| Timeout | `plugins.timeout` seconds (default 2), counted until the plugin has exited **and** closed its stdout and stderr. A background job that inherited them keeps the event open: detach it with its output redirected (`job </dev/null >/dev/null 2>&1 &`). `0` kills every plugin at once. |

A failure is one warning naming the plugin and the event, on stderr (in the TUI, in the
status bar), and the remaining plugins still run:

```
tasq: warning: plugin git: store-open event failed: exit status 3: rebase in progress
tasq: warning: plugin git: post-write event failed: timed out after 2s
```

The message after `exit status <n>:` is the plugin's stderr, else its stdout, trimmed; it is
left out when both are empty. A plugin that cannot be started reads `could not run <path>:
<OS error>`, one killed by a signal `killed by a signal`.

**Calling back into tasq.** A plugin reads the store through `"$TASQ_BIN" list --json`,
`view --json`, and so on, and sees the same store, profile and overrides. A `tasq` started
with `TASQ_EVENT` set (by a plugin, during an event) tells no plugin about anything and prints
no lasting warnings, so such a call cannot wake the plugin again. A job the plugin detaches
inherits `TASQ_EVENT` too: the writes it makes through `$TASQ_BIN` send no `post-write`.
Unset it in the job if those writes should be seen by other plugins. A plugin that writes task
files itself, rather than through tasq, takes the store lock first (see "The store lock").

### Payloads

Every payload is one JSON object with these fields, in this order:

| Field | Value |
|---|---|
| `schema` | `1`. Fields are only ever added within a version; refuse a schema you do not know. |
| `event` | `"store-open"`, `"post-write"` or `"doctor"`, the same as `TASQ_EVENT`. |
| `store` | `{"name", "location"}`: `store.name` from the configuration and the store directory, absolute (`<data_dir>/<store.name>`). The directory may not exist yet: an empty store is created by its first write. |
| `config` | the plugin's `[plugin.<name>]` table as JSON, `{}` when there is none. TOML datetimes become their TOML text (`"2026-10-26"`). |
| `command` | `post-write` only: the subcommand that wrote, as clap names it (an alias reports the command: `ls` is `list`; `worktree set` is `worktree`). `ui` in the terminal UI. |
| `writes` | `post-write` only: `[{"id", "file", "kind"}]`, every task the command wrote, each once, in the order first written. `id` is the task id as a string (as in `docs/json.md`), `file` the absolute path of its task file, `kind` `"create"` for a new file or `"update"` for a rewritten one. A task created and then updated by the same command (`create --worktree`) is one `create`. |

No payload carries task contents: a plugin that wants them asks `"$TASQ_BIN" view <id> --json`.

`store-open`, with `[plugin.git] interval = 60`:

```json
{"schema":1,"event":"store-open","store":{"name":"default","location":"/home/me/.local/share/tasq/stores/default"},"config":{"interval":60}}
```

`post-write`, after `tasq move 10 --after 3` ranked task 3 as well:

```json
{"schema":1,"event":"post-write","store":{"name":"default","location":"/home/me/.local/share/tasq/stores/default"},"config":{"interval":60},"command":"move","writes":[{"id":"3","file":"/home/me/.local/share/tasq/stores/default/01K6Q8ZT3D0000000000000003.md","kind":"update"},{"id":"10","file":"/home/me/.local/share/tasq/stores/default/01K6Q8ZT3D000000000000000A.md","kind":"update"}]}
```

`post-write`, after `tasq create "Fresh"`:

```json
{"schema":1,"event":"post-write","store":{"name":"default","location":"/home/me/.local/share/tasq/stores/default"},"config":{},"command":"create","writes":[{"id":"8","file":"/home/me/.local/share/tasq/stores/default/01K8G0RZ5M7Q2W9V3XKJ4T6B1C.md","kind":"create"}]}
```

`doctor`:

```json
{"schema":1,"event":"doctor","store":{"name":"default","location":"/home/me/.local/share/tasq/stores/default"},"config":{}}
```

### Which writes `post-write` reports

A write is a task file whose bytes changed. Every command that writes through the store sends
`post-write`: `create`, `set`, `log`, `done`, `reopen`, `rename`, `tag`, `due`, `desc`,
`apply`, `sync`, `move`, `mr`, `session`, `project`, `worktree`, and `next`/`pick` when they
set the task in progress (sent just before the launcher runs, since a launcher may replace the
`tasq` process). In `tasq ui`, every action that wrote sends one with `"command": "ui"`, `e`
(`$EDITOR` on the task file) included when the file changed; `Enter` and `S` run `tasq pick`
and `tasq sync` as child processes, which send their own.

None is sent when nothing was written: a read, an edit to the value the task already has (`set
3 A` on an `A` task, `move` to where the task already is, `apply` of an unchanged document), a
`--dry-run`, a command that fails before its first write. A command that fails after a write
still sends the writes that happened, then exits with its error.

`post-write` comes after the command's output and after its `[hooks]`: `post-create` and
`post-done` hooks run first, `pre-launch` hooks too (a `pre-launch` hook that aborts the
launch is followed by the `post-write` of the status already written).

The writes are found by reading each task's file before and after the store writes it, outside
the store lock: a file another process changed in between is reported too (it did change).

## Lasting warnings

An event happens once; a problem may last (a push that keeps failing, a rebase that stopped).
A plugin keeps the problems it wants shown in

```
<store>/.tasq/plugins/<name>/warnings
```

UTF-8 text, one warning per line; lines are trimmed and blank ones skipped. The plugin owns the
file: tasq only reads it, never creates, changes or deletes it. A missing file or directory, or
one that cannot be read, is no warnings. Write it atomically (a temp file in the same directory,
then `mv`) so tasq never reads half a line, and remove it, or empty it, when the problem is gone.

While it has lines, they are shown:

- **Every command that opens the store** prints each on stderr, after the `store-open` event
  (so a plugin can refresh the file there) and after the missing-plugin warnings:

  ```
  tasq: warning: git: push failed: no network
  ```

  A `tasq` run with `TASQ_EVENT` set prints none.
- **`tasq ui`** shows them in the status bar in place of the key hints, checked at start and
  every 2 s: `warning: git: push failed: no network`, or `3 warnings: git: push failed: no
  network` for several (the first one, and the count). A message or a prompt takes the line
  first; the warnings come back with the next key.
- **`tasq doctor`** lists each as a `WARN` check named `<name>/warning`, with the fix
  `shown until tasq-<name> removes it from <file>`. They are read after the `doctor` event, so
  a plugin can refresh them while answering it.

The warnings of every enabled plugin are read, installed or not.

## Doctor checks

`tasq doctor` sends each installed enabled plugin the `doctor` event and reads its stdout as a
JSON array of checks:

```json
[
  {"name": "remote", "status": "ok", "message": "origin reachable"},
  {"name": "push", "status": "warn", "message": "3 commits not pushed", "fix": "tasq git push"}
]
```

| Field | |
|---|---|
| `name` | the check's name; listed as `<plugin>/<name>` |
| `status` | `"ok"`, `"warn"` or `"fail"` |
| `message` | one line, shown next to the name (`detail` in `tasq doctor --json`) |
| `fix` | optional; what to do, shown under it |

Unknown fields are ignored. Blank output is no checks: a plugin with nothing to say (or one
written before this event) prints nothing and exits `0`. Anything else is one `FAIL` check
named `<plugin>/doctor`:

| Answer | Check |
|---|---|
| not a JSON array of checks (text, an unknown `status`, a missing field) | `the doctor answer is not a list of checks: <parse error>`, fix `tasq-<name> must print a JSON array of {name, status, message, fix} on doctor` |
| a non-zero exit, a timeout, a plugin that cannot start | `plugin <name>: doctor event failed: <reason>`, fix `tasq-<name> hook must answer doctor and exit 0` |

So a plugin must print nothing on stdout for the `doctor` event unless it is the answer, and
exit `0` on an event it does not know.

The plugins' checks come after the store's and before the tool checks, each plugin in
`enabled` order: its `tasq-<name>` check (`OK` with its path, or `FAIL` when it is not on
`PATH`), its answer, its lasting warnings. Every check a plugin owns starts with `<name>/`, so
a plugin named `git` never reads as the `git` tool check. Any `FAIL`, a plugin's included,
makes `tasq doctor` exit 1.

```
OK    lock         no write in progress
OK    tasq-git     plugin git is /home/me/bin/tasq-git
OK    git/remote   origin reachable
WARN  git/push     3 commits not pushed
                   fix: tasq git push
WARN  git/warning  push failed: no network
                   fix: shown until tasq-git removes it from /home/me/.local/share/tasq/stores/default/.tasq/plugins/git/warnings
OK    git          git found at /usr/bin/git
```

## The store lock

Every tasq write takes `<store>/.tasq/lock` for its duration (an exclusive create; a lock not
touched for two minutes is taken for a dead holder and removed). When it is taken and when it
is refreshed, the lock's mtime is set to tasq's clock (`TASQ_NOW` when set), the clock its age
is judged by, or to the wall time when that is later: a lock is never stamped older than it
is, and one stamped ahead of a waiter's clock is live to it. A plugin that rewrites task
files itself (a `git pull`, a rebase, a restore from backup) runs that work under the same lock,
so that it never races a write:

```sh
tasq store lock [--timeout SECS] -- <cmd> [args...]
```

- Takes the lock, waiting while another holds it (with no limit by default; the stale rule
  already bounds a dead holder), then runs `<cmd>` with the terminal's stdin, stdout and
  stderr, releases the lock and exits with `<cmd>`'s exit code (128 plus the signal when a
  signal ended it).
- `<cmd>`'s environment is the environment `tasq` was started with, plus `TASQ_BIN`,
  `TASQ_PROFILE`, `TASQ_CONFIG_DIR` and `TASQ_SET` when in effect, `TASQ_STORE_DIR` (the store
  directory, absolute) and `TASQ_LOCKED=1`.
- While `<cmd>` runs, the lock's mtime is refreshed every 30 s, so a command that runs longer
  than two minutes is not taken for dead. A `tasq` killed meanwhile (Ctrl-C included) leaves
  the lock behind until it goes stale.
- Exit 1 without running `<cmd>`: the lock cannot be taken (`could not take the store lock
  <path>: <error>`), it is still held after `--timeout` seconds (`the store lock <path> is
  still held after <n>s; <cmd> was not run`), or `<cmd>` cannot be started (`could not run
  <cmd>: <error>`).
- It does not open the store: no `store-open`, no missing-plugin or lasting warnings. The store
  directory is created if it does not exist.
- `<cmd>` must not write through tasq: that write waits for the very lock `<cmd>` runs under.
  Reads (`$TASQ_BIN list --json`) take no lock and are fine. `TASQ_LOCKED` is informational.

```sh
tasq store lock -- git -C "$store" pull --rebase
tasq store lock --timeout 10 -- sh -c 'cd "$TASQ_STORE_DIR" && git rebase origin/main'
```

## The terminal UI

`tasq ui` sends `store-open` when it starts and again on each `r`, and `post-write` after each
action that wrote (above). A plugin's failure or timeout shows in the status bar after the
action's own message; the UI waits for the plugin, so a slow one freezes it for up to
`plugins.timeout` seconds.

Every 2 s, while no picker or prompt is open, the UI compares the names, lengths and
modification times of the store's `.md` files with what it last saw and reloads when they
differ, keeping the selected task (by id) and the scroll. Files changed by another terminal, a
hand edit or a plugin's job therefore show without `r`, and an edit made after the reload
starts from the new contents. A rewrite that keeps both the length and the mtime of a file is
not noticed until `r`.

## Hooks

Hooks are command lines under `[hooks]`, wired per moment and given the task. They predate
enabled plugins and are unchanged by them: a plugin that should follow every write is enabled
under `[plugins]`; a hook is the lighter tool for one command around one moment.

Hooks are configured like this:

```toml
[hooks]
post-create = ["~/bin/notify-new-task"]
post-done = ["/path/to/log-event.sh"]
pre-launch = ["check-vpn --quiet"]
```

| Hook | Runs | On failure |
|---|---|---|
| `post-create` | after `tasq create`, or the terminal UI's `c` key, wrote the task | warning on stderr (in the UI: in the status bar); the task exists |
| `post-done` | after `tasq done`, or the terminal UI's `d` key, closed the task | warning on stderr (in the UI: in the status bar); the task is closed |
| `pre-launch` | in `tasq next`/`tasq pick`, after the task is set to in-progress and the working directory is resolved, right before the launcher runs | the launch is aborted with the hook's message (exit 1) |

Each entry is split like a shell command line (quotes allowed, no shell, `~` expanded in the
program) and run from the current directory with the inherited environment plus the variables
of the table above and:

| Variable | Value |
|---|---|
| `TASQ_HOOK` | `post-create`, `post-done` or `pre-launch` |
| `TASQ_TASK_ID` | the task id |

The hook reads its document on stdin:

```json
{"schema": 1, "hook": "post-create", "task": { ...the Task of docs/json.md... }}
```

`pre-launch` adds `"workdir"` (the directory the session will start in), `"in_worktree"` (only
with worktrees enabled) and `"launcher"` (the resolved name, `claude`, `shell`, `tmux` or `herdr`). The hook's stdout is
shown only with `-v`; a non-zero exit reports its stderr (else stdout). Commands run in order;
after a failing `post-*` hook the remaining ones still run. `--dry-run` lists the `pre-launch`
hooks as skipped steps and runs nothing. A command line that cannot be split (an unbalanced
quote) is a configuration error, not a warning.

`examples/plugins/hooks/log-event.sh` is a complete hook: it appends
`<time> <hook> [<id>] <title>` to `~/.local/share/tasq/hooks.log`.

### Hooks and the terminal UI

`tasq ui` fires the same hooks as the commands it stands in for: `Enter` runs `tasq pick`, so
`pre-launch` fires there; `d` runs the `post-done` hooks in-process after the close, with
the same document and environment as `tasq done` (ADR-0010); `c` runs the `post-create` hooks
the same way after the write (ADR-0011). A failing hook shows in the UI's status bar after the
`[id] done: ...` or `[id] created: ...` line; the task stays closed, or exists. A hook's stdout
is not shown in the UI, `-v` or not.

### What does not fire a hook

- `tasq apply` with `"done": true`, and `tasq sync` closing a task: only `tasq done` and the
  UI's `d` key fire `post-done`.
- `tasq create --status done`: `post-create` fires (a task was created), `post-done` does not.

An enabled plugin sees all of these through `post-write`, which every writing command sends.

## `tasq plugins list`

```
Enabled plugins ([plugins] in the config):
  git   not on PATH
  echo  /home/me/bin/tasq-echo

Plugins on PATH (tasq-<name>):
  echo   /home/me/bin/tasq-echo
  tlogs  /home/me/bin/tasq-tlogs

Hooks ([hooks] in the config):
  post-done    /home/me/code/tasq/examples/plugins/hooks/log-event.sh
  pre-launch   check-vpn --quiet
```

With `--json`: `{"schema": 1, "enabled": [{"name", "path"}], "plugins": [{"name", "path"}],
"hooks": {"post-create": [...], "post-done": [...], "pre-launch": [...]}}`; `enabled` is in
`[plugins] enabled` order, with `path` `null` for a plugin that is not on `PATH`. Only the
first executable per name is listed, as only that one would run. Non-executable files and
directories named `tasq-*` are ignored. `plugins list` does not open the store and sends no
event.

`tasq store info` ends with `plugins:     git, echo` (`(none)` when none is enabled); with
`--json`, `"plugins": ["git", "echo"]`.

## In-process extension

Stores, sources and launchers that ship with `tasq` are Rust implementations of the
`tasq-core` traits (`Store`, `Source`, `Launcher`), compiled in and gated by cargo features
where optional (`herdr`). That is the path for an adapter contributed to this repository; see
`CONTRIBUTING.md`. ADR-0006 explains why user plugins are out of process instead.
