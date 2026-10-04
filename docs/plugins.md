# Plugins

`tasq` is extended from outside the binary (ADR-0006): a plugin is an executable named
`tasq-<name>` on `PATH`, written in any language, that talks to `tasq` through its `--json`
output and its editing commands. Hooks are command lines in the configuration that `tasq` runs
around three events. Nothing is linked into the binary; the versioned JSON documents in
`docs/json.md` are the whole API.

## Running a plugin

```sh
tasq tlogs last week          # runs tasq-tlogs with the arguments "last week"
tasq --profile work tlogs     # the plugin sees the work profile too
tasq plugins list             # what is on PATH, and the configured hooks
```

`tasq <name> [args...]` runs `tasq-<name>` when:

1. `<name>` is not a built-in command (`list`, `create`, `plugins`, ... and `help`).
   Built-ins always win; a plugin called `tasq-list` is never run.
2. An executable file `tasq-<name>` exists in a `PATH` entry (the first one wins).

Otherwise the command line is parsed as usual, so `tasq ready` is the READY group unless a
`tasq-ready` executable exists. A plugin therefore shadows the bare `tasq <word>` filter for
its name; `tasq list <word>` always reaches the filter.

The plugin receives the arguments after its name verbatim (`tasq tlogs --json x` runs
`tasq-tlogs --json x`) and replaces the `tasq` process, so its exit code, stdin and streams are
the user's. Global flags before the name belong to `tasq` and are forwarded as environment
variables:

| Variable | Set when | Value |
|---|---|---|
| `TASQ_BIN` | always | path of the `tasq` binary that ran the plugin |
| `TASQ_PROFILE` | `--profile` given (or already in the environment) | the profile name |
| `TASQ_CONFIG` | `--config` given (or already in the environment) | absolute path of the config file |
| `TASQ_SET` | `--set` given (or already in the environment) | `key=value` overrides, one per line, existing entries first |

`tasq` itself reads the same variables (`docs/config.md`), so `"$TASQ_BIN" list --json` inside
the plugin resolves the same notebook, profile and overrides as the command that started it.
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
| create / edit / close | `tasq create ... --json`, `tasq set`, `tasq log`, `tasq done`, `tasq mr`, `tasq session`, `tasq worktree`, `tasq project` |
| arbitrary edit of a task | `tasq view <id> --json`, change the `task`, `tasq apply` (refuses what the store cannot express) |

Every document carries `"schema": 1`; check it and refuse anything else. Exit codes: `0`,
`1` for an error you can fix (message on stderr), `2` for a usage error.

### The reference plugin: `tasq-tlogs`

`examples/plugins/tasq-tlogs` (bash and jq) is the shape of the author's time-log tool with
the personal parts removed. `tasq tlogs [SPEC...]` resolves the range with `tasq dates --json`,
collects `tasq summary --json --raw <day>` for every working day, and prints a proposal that
splits `TLOGS_HOURS` (default 8) evenly across the tasks that received a note that day:

```
$ tasq tlogs last week
2026-09-28 (Monday)
  [12] Rewrite the tasks script in Rust  6h  (3 notes)
  [15] Review MR !77: Faster index         2h  (1 note)
2026-09-29 (Tuesday)
  nothing logged
```

With `--json` the same data is `{"schema": 1, "from", "to", "days": [{day, weekday, tasks:
[{id, title, notes, hours}]}]}`, ready for whatever posts the entries (GitLab `/spend`, a
timesheet). To install it: copy or symlink the file somewhere on `PATH`.

The CLI integration test `plugins::example_tlogs_plugin_runs_through_dispatch` runs this
script through the dispatcher against the fixture notebook.

## Hooks

Hooks are command lines under `[hooks]`:

```toml
[hooks]
post-create = ["~/bin/notify-new-task"]
post-done = ["/path/to/log-event.sh"]
pre-launch = ["check-vpn --quiet"]
```

| Hook | Runs | On failure |
|---|---|---|
| `post-create` | after `tasq create` wrote the task | warning on stderr; the task exists |
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

`pre-launch` adds `"workdir"` (the directory the session will start in), `"in_worktree"` and
`"launcher"` (the resolved name, `claude`, `shell`, `tmux` or `herdr`). The hook's stdout is
shown only with `-v`; a non-zero exit reports its stderr (else stdout). Commands run in order;
after a failing `post-*` hook the remaining ones still run. `--dry-run` lists the `pre-launch`
hooks as skipped steps and runs nothing. A command line that cannot be split (an unbalanced
quote) is a configuration error, not a warning.

`examples/plugins/hooks/log-event.sh` is a complete hook: it appends
`<time> <hook> [<id>] <title>` to `~/.local/share/tasq/hooks.log`.

### Hooks and the terminal UI

`tasq ui` fires the same hooks as the commands it stands in for: `Enter` runs `tasq pick`, so
`pre-launch` fires there, and `d` runs the `post-done` hooks in-process after the close, with
the same document and environment as `tasq done` (ADR-0010). A failing `post-done` hook shows
in the UI's status bar after the `[id] done: ...` line; the task stays closed. A hook's stdout
is not shown in the UI, `-v` or not.

### What does not fire a hook

- `tasq apply` with `"done": true`, and `tasq sync` closing a task: only `tasq done` and the
  UI's `d` key fire `post-done`.
- `tasq create --status done`: `post-create` fires (a task was created), `post-done` does not.

## `tasq plugins list`

```
Plugins on PATH (tasq-<name>):
  tlogs  /home/me/bin/tasq-tlogs

Hooks ([hooks] in the config):
  post-done    /home/me/code/tasq/examples/plugins/hooks/log-event.sh
  pre-launch   check-vpn --quiet
```

With `--json`: `{"schema": 1, "plugins": [{"name", "path"}], "hooks": {"post-create": [...],
"post-done": [...], "pre-launch": [...]}}`. Only the first executable per name is listed, as
only that one would run. Non-executable files and directories named `tasq-*` are ignored.

## In-process extension

Stores, sources and launchers that ship with `tasq` are Rust implementations of the
`tasq-core` traits (`Store`, `Source`, `Launcher`), compiled in and gated by cargo features
where optional (`herdr`). That is the path for an adapter contributed to this repository; see
`CONTRIBUTING.md`. ADR-0006 explains why user plugins are out of process instead.
