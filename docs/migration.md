# Migrating from `tasks` to `tasq`

**TL;DR:** `tasq` replaces the original `tasks` bash script. It keeps its
tasks in its own store directory (ADR 0026), not in the nb notebook the script
uses, so the two tools do not see each other's tasks, and the script's tasks
are not imported (ADR 0027). Switch command by command using the mapping
below.

The one-week side-by-side run the first plan called for (T-904, task 2) is
obsolete: it was closed when `tasq` got its own store.

## Command mapping

Every command the script dispatches (`original/tasks`, the `case` at the end
of the file) and its `tasq` counterpart.

| `tasks` | `tasq` | Notes |
|---|---|---|
| `tasks` | `tasq` (= `tasq list`) | Same grouped view. |
| `tasks <status\|tag>` (`tasks review-request`, `tasks A`) | `tasq <status\|tag\|prio>` or `tasq list <WORD>` | Also `--status`, `--tag` (repeatable), `--prio`, `--text`. |
| `tasks create <title> [--desc] [--status] [--prio] [--due] [--project] [--tag]... [--related]... [--mr]... [--note]` | `tasq create <TITLE>` with the same flags | `--status done` creates it closed, as before. Default note is `created via tasq create`. |
| `tasks set <id> <status\|priority> [note]` | `tasq set <ID> <VALUE> [NOTE]` | Priority with or without `#`. The note can also be `--note TEXT` (not both). A value the task already has writes nothing (`status unchanged`); with a note that is an error, since the note is not logged either (`tasq log` adds one alone). |
| `tasks log <id> <note>` | `tasq log <ID> <NOTE>` | |
| `tasks next` | `tasq next [--launcher L] [--dry-run]` | Launcher from `launch.default`. |
| `tasks pick <id>` | `tasq pick <ID> [--launcher L] [--dry-run]` | |
| `tasks done <id> [note]` | `tasq done <ID> [NOTE]` | NOTE can also be `--note TEXT` (not both). Writes `status: done` and the closing time (the script flipped the title to `# [x]` through `nb todo do`). A task that is already done is refused (exit 1). |
| none (`nb todo undo`, then `tasks set`) | `tasq reopen <ID> [STATUS] [NOTE]` | Drops `status: done` and the closing time, sets STATUS (default `workflow.default_status`) and logs NOTE (default `reopened`) in one write. A lone word that is not a status is the note; with `--note TEXT` (not both) a word there must be a status. |
| none (`nb edit`, or the `tasq view --json \| jq \| tasq apply` round trip) | `tasq rename <ID> <TITLE> [--note TEXT]` | Trimmed, one non-empty line. The same title writes nothing (`title unchanged`); with `--note` that is an error. |
| none (`nb edit`, or the `tasq apply` round trip) | `tasq due <ID> <DATE\|none> [--note TEXT]` | DATE as for `create --due` (YYYY-MM-DD, today, tomorrow, yesterday); `none` removes the `due` key. The same date writes nothing (`due unchanged`); with `--note` that is an error. |
| none (edit `## Tags` by hand, or the `tasq apply` round trip) | `tasq tag <ID> <TAG>... [--note TEXT]` | `+name` or `name` adds, `-name` removes, the other tags stay in place. Prints only the changes that took effect (`tags unchanged` when none, and then writes nothing; with `--note` that is an error). Status and priority words are refused (use `tasq set`). Long options go before or after the tags; short ones (`-v`) before, since `-name` is a tag. |
| none (`nb edit`, or the `tasq apply` round trip) | `tasq desc <ID> [TEXT\|-\|--edit] [--note TEXT]` | No TEXT prints the description; `-` reads stdin to EOF (the task is read first, so a change made while stdin is open is a conflict, not an overwrite); `""` removes the `## Description` section; `--edit` opens `$VISUAL`, else `$EDITOR`, on it (a non-zero exit or an unchanged file writes nothing; when the edit cannot be saved, the error names the file that keeps it). Blank lines at either end are dropped; a line starting with `## ` is refused (it would start a section). An unchanged description writes nothing; with `--note` that is an error, since the note is not logged either (`tasq log` adds one alone). |
| `tasks view <id>` | `tasq view <ID>` | glow rendering and OSC 8 links as before. |
| `tasks view <id> <nb args>` (nb passthrough) | none; `tasq view <ID> --raw` prints the file | Decided in T-305. |
| `tasks project <id> [path]` | `tasq project <ID> [PATH]` | |
| `tasks worktree <id> <path>` | `tasq worktree <ID> <PATH>` | |
| `tasks worktree <id> --create <branch>` | `tasq worktree <ID> --create <BRANCH>` (hidden), or `tasq worktree <ID> <BRANCH>` then `tasq worktree up <ID>` | Declares the worktree at `worktree.path` and runs `git worktree add`. The script called gwm; tasq provisions worktrees itself under `[worktree]` (`docs/config.md`, "Worktrees"). |
| `tasks session <id> <session-id> [desc]` | `tasq session <ID> <SESSION_ID> [DESC] [--launcher L]` | |
| `tasks mr <id> <mr-url> [title]` | `tasq mr <ID> <URL> [TITLE]` | Title resolved through the configured `[forge.*]`; without one it records the short reference (`group/project!123`). The script used `glab api` from `TASKS_DEFAULT_WORKTREE`. |
| `tasks update [args]` (Claude `/update-tasks` session) | `tasq sync --interactive` (the same session, running `/tasq:sync` in `work.default_project`); `tasq sync [--source NAME] [--dry-run] [--json]` headless | Sources are `[[source]]` blocks: GitLab/GitHub review requests and work items, plus an LLM bridge for Slack/Gmail-style inboxes ([`docs/sources.md`](sources.md)). The bridge was verified headless on 2026-10-05 (same connectors, ~90 s and ~$2 per run), so Slack/Gmail triage no longer needs an interactive session. |
| `tasks update <id> [<id>...]` | `tasq sync <ID>...` | Re-checks only those tasks. |
| `tasks update-support [args]` (Freshdesk) | none | Freshdesk is a plan Non-Goal. Keep using the script and its `/update-support-tasks` skill; the `support`-tagged tasks it creates list fine with `tasq support`. |
| `tasks summary [date] [--raw]` | `tasq summary [DAY] [--raw]` | Also weekday names and `last <weekday>`; summarizer from `[report.summary]`. |
| `tasks tlogs [when]` | `tasq tlogs [when]`, dispatched to the external `tasq-tlogs` plugin | Time logs are a plan Non-Goal for the main repo. `tasq <name>` runs an executable `tasq-<name>` found on `PATH`; `examples/plugins/tasq-tlogs` does what the script did: `tasq dates --json` for the range, then `claude "/time-logs <from> <to>"` from `work.default_project` under `direnv exec`. Put it on `PATH` and set `work.default_project` (the script's `TASKS_DEFAULT_WORKTREE`). `--dry-run` prints the command, `--propose` prints an even split per day without launching anything. |
| `/wrapup` (the skill the script's session prompt ends with) | `/tasq:wrapup [task-id]` | Claude Code plugin in `plugins/claude`; defaults to `$TASQ_TASK_ID`, set by `tasq next`/`pick`. |
| `tasks help` | `tasq --help`, `tasq <cmd> --help` | |

New in `tasq`, with no script equivalent: `tasq ui` (TUI), `tasq apply` (JSON
in), `tasq dates`, `tasq store info`, `tasq doctor`, `tasq config show`,
`tasq completions <shell>`, `tasq plugins list` plus `tasq-<name>` dispatch and
`[hooks]` command lines, and `--json` on every command
([`docs/json.md`](json.md)).

## Environment and configuration mapping

The script is configured with environment variables and hard-coded values.
`tasq` reads layered TOML (`~/.config/tasq/config.toml`, nearest `.tasq.toml`,
`[profile.<name>]`, `TASQ_*`, `--set`; [`docs/config.md`](config.md)). Key
names below are from `crates/core/src/config/mod.rs`.

| Script | Default in the script | `tasq` key | `tasq` env |
|---|---|---|---|
| `TASKS_NB_NOTEBOOK` | `home` | `store.name` (a tasq store under `data_dir`, ADR-0026) | `TASQ_STORE_NAME` |
| `NB_DIR` | `~/.nb` | none; tasks live in `data_dir` (default `~/.local/share/tasq/stores`) | `TASQ_DATA_DIR` |
| `TASKS_DEFAULT_WORKTREE` | `~/code/SF/silverfin_worspace/silverfin` | `work.default_project` (no default: unset means "no fallback directory") | `TASQ_DEFAULT_PROJECT` |
| `TASKS_PAGER` | `less -RFX` | `ui.pager` | `TASQ_PAGER` |
| `TASKS_GLOW_STYLE` | `dark` | `ui.glow_style` | `TASQ_GLOW_STYLE` |
| `TASKS_NO_OSC8` | unset | `ui.no_osc8` | `TASQ_NO_OSC8` |
| `TASKS_SUMMARY_MODEL` | `sonnet` | `report.summary.model` | `TASQ_SUMMARY_MODEL` |
| `claude -p` as the summarizer | hard-coded | `report.summary.summarizer` (`llm`/`raw`), `report.summary.command`, `report.summary.prompt_file` | `TASQ_SUMMARIZER`, `TASQ_SUMMARY_COMMAND`, `TASQ_SUMMARY_PROMPT_FILE` |
| `HERDR_ENV` set → herdr workspace, else `claude` | implicit | `launch.default` (`auto` reproduces it; or `claude`, `shell`, `tmux`, `herdr`) | `TASQ_LAUNCHER` |
| `direnv exec <dir>` when `.envrc` is allowed | hard-coded | `launch.env` (`direnv`, the default, or `inherit`) | `TASQ_LAUNCH_ENV` |
| session prompt text in `open_session` | hard-coded | `launch.claude.prompt_file` | |
| `STATUSES=(in-progress ready waiting blocked later)` | hard-coded | `workflow.statuses`, `workflow.default_status` (`ready`) | |
| `glab api` for MR titles, `gitlab.silverfin.com` | hard-coded | `[forge.gitlab] host`, `token_cmd = "glab config get token --host gitlab.silverfin.com"` (or `GITLAB_TOKEN`) | |
| `/update-tasks` skill (GitLab, Slack, Gmail) | Claude session | `[[source]]` blocks (`gitlab-review-requests`, `gitlab-work-items`, `github-*`, `llm-bridge`) | |
| `FRESHDESK_API_KEY`, `bin/freshdesk` | | none (Non-Goal) | |
| `nb` bookkeeping, `NB_AUTO_SYNC_SECONDS` | always through nb, sync on every checkpoint | none: the store is not committed anywhere; opt-in git sync is task 57 (ADR-0026) | |

A global config that reproduces the script's defaults:

```toml
# ~/.config/tasq/config.toml
[store]
name = "home"           # the store directory under data_dir

[work]
default_project = "~/code/SF/silverfin_worspace/silverfin"

[launch]
default = "auto"        # herdr inside herdr, claude otherwise

[forge.gitlab]
host = "gitlab.silverfin.com"
token_cmd = "glab config get token --host gitlab.silverfin.com"

[report.summary]
model = "sonnet"
```

`export TASKS_NB_NOTEBOOK=Zantop` per project becomes a `.tasq.toml` in that
project with `[store] name = "Zantop"`, or a
`[profile.zantop]` block and `--profile zantop` / `TASQ_PROFILE`.

## What `tasq` writes that the script never did

| What | Where |
|---|---|
| The frontmatter block (`id`, `status`, `priority`, `rank`, `due`, `closed`, `project`, `source`, `source_id`, `source_url`, `tags`) | The top of every task file, in place of the `# [x]` marker and the scalar sections |
| `sync(<source>): created`, `sync(<source>): merged`, `sync(<source>): needs attention` progress notes | `## Progress`, dated like every other note |
| The source's `flag` tag (`review-request`) on matched tasks | `tags` |
| `created via tasq create` default note | `## Progress` |

The `<!-- tasq: ... -->` comments mentioned in `docs/file-format.md` are still
TBD and are not written by any command today.

## Behavioural differences worth knowing

From the deviations log in `ruli/features/rust-rewrite/PROGRESS.md`:

- `tasq set <id> <value> ""` and `tasq done <id> ""` are errors: a note is
  either present and non-empty or absent. The script appended nothing for an
  empty note.
- `tasq dates "this week"` ends on Friday (the script ended on today, even on
  a weekend). Explicit ranges are clamped to today; a start after today is an
  error.
- `tasq <status>` on an empty status group prints a message instead of nothing.
- Usage errors exit 2 (clap convention); domain errors exit 1 with
  `tasq: ...`. The script exited 1 for both.
- `tasq view` has no nb passthrough; `--raw` prints the file verbatim.
- Ids never change. The script's ids were line numbers of nb's `.index`, and
  `nb index reconcile` could renumber them.
- Nothing is committed (the script committed every change through nb); sync
  for a tasq store is task 57.
- A launch creates the task's declared worktree when it is gone, without
  asking (the script offered to recreate it); `tasq worktree --create` runs
  plain `git worktree add`, and worktree provisioning (links, copies,
  templates, hooks) is configured under `[worktree]` instead of an external
  tool.
- `tasq next`/`pick` default to the `claude` launcher regardless of
  `HERDR_ENV`; set `launch.default = "auto"` to get the script's "herdr when
  inside herdr" behaviour.
- `tasq summary` hands the prompt and the notes to the summarizer on stdin;
  the script passed the prompt as an argument. Only matters if you replace
  `report.summary.command`.
- `--json` output is always `{"schema": 1, ...}`, never a bare array.

## Switch-over checklist

- [ ] Install: `cargo install tasq` (or a release tarball, or the Homebrew
      tap; [`docs/release.md`](release.md)). `tasq --version`.
- [ ] `tasq doctor`: the store location and where it was configured, every
      task file readable, no duplicate ids, no stale lock; optional tools
      (`git`, `glow`, `claude`, `direnv`, `glab`) reported.
- [ ] Write `~/.config/tasq/config.toml` from the mapping above;
      `tasq config show` prints every key with the layer that set it.
- [ ] Forge token: `glab config get token --host gitlab.silverfin.com` prints it in a
      shell, and `[forge.gitlab] token_cmd` is set to that command (or `GITLAB_TOKEN`).
      glab 1.118 has no `auth token` subcommand: it prints help with exit 0, which tasq
      would send as the token. Never paste the token into the config file.
- [ ] `tasq sync --dry-run`: read the planned creates/closes/notes against the
      tasks `tasks update` created; nothing is written. Then `tasq sync`.
- [ ] `tasq sync --interactive --dry-run` prints the `claude "/tasq:sync"` command
      that replaces `tasks update` (same `direnv exec` wrapping, run in
      `work.default_project`); `tasq sync --interactive` opens it.
- [ ] Inbox bridge: copy `examples/sources/inbox.md` to `~/.config/tasq/prompts/` and add
      the `inbox` `[[source]]` from `examples/sources/config.toml`. Each run is one headless
      Claude session (~90 s, ~$2), so keep its `auto = false` and run it on purpose with
      `tasq sync --source inbox` or the `S` picker of `tasq ui`
      ([`docs/sources.md`](sources.md), "Headless Claude Code").
- [ ] Point herdr and Claude workflows at `tasq next` and `tasq pick <id>`
      (they set `TASQ_TASK_ID` for the session).
- [ ] Install the Claude Code plugin (`claude plugin marketplace add <repo>`,
      `claude plugin install tasq@tasq`, or `claude --plugin-dir
      plugins/claude`) and the status line from
      [`plugins/claude/README.md`](../plugins/claude/README.md).
- [ ] Shell completions: `tasq completions zsh > ~/.zfunc/_tasq` (bash and
      fish in `tasq completions --help`).
- [ ] Keep `tasks update-support` on the script (no Freshdesk in `tasq`). It
      writes to the script's nb notebook, which `tasq` does not read: copy those
      tasks over with `tasq create`.
- [ ] Put `examples/plugins/tasq-tlogs` on your `PATH` and set
      `work.default_project` to the old `TASKS_DEFAULT_WORKTREE`; check with
      `tasq tlogs --dry-run`, then `tasq tlogs` replaces `tasks tlogs`.
- [ ] Last: retire the old `/wrapup` and `/update-tasks` skills once every
      session is started by `tasq`. Do not delete `original/tasks`; it is the
      reference the tests are built against.

## Rollback

`tasq` never writes the script's nb notebook: go back to the script and nb.
Tasks created or changed with `tasq` live only in the store directory; they do
not flow back to the notebook.
