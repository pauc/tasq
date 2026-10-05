# Migrating from `tasks` to `tasq`

**TL;DR:** `tasq` and the original `tasks` bash script can run on the same nb
notebook at the same time. They read the same files, use the same ids and write
the same format; nb keeps doing the index and git bookkeeping when it is
installed. Switch command by command, keep the script around until
`tasq doctor` is clean for a week, then retire the old skills.

The one-week side-by-side acceptance (plan T-904) is **pending the author**;
the procedure is at the end of this document.

## Why running both is safe

- **Same files.** Both tools operate on `$NB_DIR/<notebook>/*.todo.md`
  (`YYYYMMDDHHMMSS.todo.md`). `tasq` never copies, converts or indexes the
  notebook anywhere else.
- **Same ids.** Both read `.index` and use the line number as the task id.
  `tasq` only ever appends to `.index` (through `nb index add`, or natively
  with the same bytes) and never rebuilds or reorders it. `nb index reconcile`
  renumbers ids for both tools alike, which is why `tasq doctor` points at it
  instead of running it.
- **Lossless format.** `tasq` parses a file into a document that keeps unknown
  sections, unknown lines, ordering, blank lines and line endings byte for
  byte, and rewrites only the section an edit touched. For every file the
  script produced, `write(parse(x)) == x` (see
  [`docs/file-format.md`](file-format.md), "Lossless editing").
- **Same bookkeeping.** With `nb` on `PATH` (`store.bookkeeper = "auto"`,
  the default), every `tasq` write is followed by `nb index add` for new
  files and `nb git checkpoint --wait "[tasq] Update: <file>"`, so `nb
  history`, `nb sync` and nb's `auto_sync` behave as before
  ([ADR-0007](adr/0007-nb-under-the-hood.md)). Without nb, `tasq` appends
  to `.index` and commits with `git` itself.
- **Conflicts are detected, not merged.** `tasq` captures mtime, length and
  hash of a file when it reads it and refuses to write (`Conflict`) if the
  file changed in between. Alternating the two tools is fine; editing the
  same task from both at the same second is not, and `tasq` is the one that
  will say so.

## Command mapping

Every command the script dispatches (`original/tasks`, the `case` at the end
of the file) and its `tasq` counterpart.

| `tasks` | `tasq` | Notes |
|---|---|---|
| `tasks` | `tasq` (= `tasq list`) | Same grouped view. |
| `tasks <status\|tag>` (`tasks review-request`, `tasks A`) | `tasq <status\|tag\|prio>` or `tasq list <WORD>` | Also `--status`, `--tag` (repeatable), `--prio`, `--text`. |
| `tasks create <title> [--desc] [--status] [--prio] [--due] [--project] [--tag]... [--related]... [--mr]... [--note]` | `tasq create <TITLE>` with the same flags | `--status done` creates it closed, as before. Default note is `created via tasq create`. |
| `tasks set <id> <status\|priority> [note]` | `tasq set <ID> <VALUE> [NOTE]` | Priority with or without `#`. |
| `tasks log <id> <note>` | `tasq log <ID> <NOTE>` | |
| `tasks next` | `tasq next [--launcher L] [--dry-run]` | Launcher from `launch.default`. |
| `tasks pick <id>` | `tasq pick <ID> [--launcher L] [--dry-run]` | |
| `tasks done <id> [note]` | `tasq done <ID> [NOTE]` | `tasq` edits the title line itself instead of calling `nb todo do`; same result (`# [x]`, status tag removed). |
| `tasks view <id>` | `tasq view <ID>` | glow rendering and OSC 8 links as before. |
| `tasks view <id> <nb args>` (nb passthrough) | none; `tasq view <ID> --raw`, or `nb show <notebook>:<ID> <args>` | Decided in T-305. |
| `tasks project <id> [path]` | `tasq project <ID> [PATH]` | |
| `tasks worktree <id> <path>` | `tasq worktree <ID> <PATH>` | |
| `tasks worktree <id> --create <branch>` (gwm) | `tasq worktree <ID> --create <BRANCH>` | Default manager is `git` (`git worktree add` into `<project>-<branch>`); for gwm set `work.worktree_manager`/`work.worktree_command` (below). |
| `tasks session <id> <session-id> [desc]` | `tasq session <ID> <SESSION_ID> [DESC] [--launcher L]` | |
| `tasks mr <id> <mr-url> [title]` | `tasq mr <ID> <URL> [TITLE]` | Title resolved through the configured `[forge.*]`; without one it records the short reference (`group/project!123`). The script used `glab api` from `TASKS_DEFAULT_WORKTREE`. |
| `tasks update [args]` (Claude `/update-tasks` session) | `tasq sync [--source NAME] [--dry-run] [--json]`; `/tasq:sync` for the Claude briefing | Sources are `[[source]]` blocks: GitLab/GitHub review requests and work items, plus an LLM bridge for Slack/Gmail-style inboxes ([`docs/sources.md`](sources.md)). The bridge was verified headless on 2026-10-05 (same connectors, ~90 s and ~$2 per run), so Slack/Gmail triage no longer needs an interactive session. |
| `tasks update <id> [<id>...]` | `tasq sync <ID>...` | Re-checks only those tasks. |
| `tasks update-support [args]` (Freshdesk) | none | Freshdesk is a plan Non-Goal for v1. Keep using the script and its `/update-support-tasks` skill; the `support`-tagged tasks it creates list fine with `tasq support`. |
| `tasks summary [date] [--raw]` | `tasq summary [DAY] [--raw]` | Also weekday names and `last <weekday>`; summarizer from `[report.summary]`. |
| `tasks tlogs [when]` | `tasq tlogs [when]`, dispatched to the external `tasq-tlogs` plugin | Time logs are a plan Non-Goal for the main repo. `tasq <name>` runs an executable `tasq-<name>` found on `PATH`; the reference plugin is `examples/plugins/tasq-tlogs`, built on `tasq dates --json` and `tasq summary`. Until it is on your `PATH`, keep `tasks tlogs`. |
| `/wrapup` (the skill the script's session prompt ends with) | `/tasq:wrapup [task-id]` | Claude Code plugin in `plugins/claude`; defaults to `$TASQ_TASK_ID`, set by `tasq next`/`pick`. |
| `tasks help` | `tasq --help`, `tasq <cmd> --help` | |

New in `tasq`, with no script equivalent: `tasq ui` (TUI), `tasq apply` (JSON
in), `tasq dates`, `tasq store info|sync`, `tasq doctor`, `tasq config show`,
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
| `TASKS_NB_NOTEBOOK` | `home` | `store.notebook` | `TASQ_NOTEBOOK` |
| `NB_DIR` | `~/.nb` | honoured as is (notebook dir is `$NB_DIR/<name>`, else `nb notebooks show <name> --path`) | `NB_DIR` |
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
| `gwm create <new> <branch> --no-tmux -s` | hard-coded | `work.worktree_manager = "command"`, `work.worktree_command` | `TASQ_WORKTREE_MANAGER`, `TASQ_WORKTREE_COMMAND` |
| `glab api` for MR titles, `gitlab.silverfin.com` | hard-coded | `[forge.gitlab] host`, `token_cmd = "glab auth token"` (or `GITLAB_TOKEN`) | |
| `/update-tasks` skill (GitLab, Slack, Gmail) | Claude session | `[[source]]` blocks (`gitlab-review-requests`, `gitlab-work-items`, `github-*`, `llm-bridge`) | |
| `FRESHDESK_API_KEY`, `bin/freshdesk` | | none (Non-Goal) | |
| `nb` bookkeeping | always through nb | `store.bookkeeper` (`auto`, `nb`, `native`) | `TASQ_BOOKKEEPER` |

A global config that reproduces the script's defaults:

```toml
# ~/.config/tasq/config.toml
[store]
notebook = "home"

[work]
default_project = "~/code/SF/silverfin_worspace/silverfin"
worktree_manager = "command"
worktree_command = "gwm create {new} {branch} --no-tmux -s"

[launch]
default = "auto"        # herdr inside herdr, claude otherwise

[forge.gitlab]
host = "gitlab.silverfin.com"
token_cmd = "glab auth token"

[report.summary]
model = "sonnet"
```

`export TASKS_NB_NOTEBOOK=Zantop` per project becomes a `.tasq.toml` in that
project with `[store] notebook = "Zantop"`, or a `[profile.zantop]` block and
`--profile zantop` / `TASQ_PROFILE`.

## What each tool writes that the other does not know about

### `tasq` writes, the script ignores

| What | Where | Effect on the script |
|---|---|---|
| `## Source` section, one line `<source-name>: <external-id> [url]` | After `## Due`, only on tasks created or matched by `tasq sync` | The script keeps unknown sections verbatim; it never reads them. |
| `sync(<source>): created`, `sync(<source>): merged`, `sync(<source>): needs attention` progress notes | `## Progress`, dated like every other note | Shown as ordinary notes. |
| The source's `flag` tag (`#review-request`) on matched tasks | `## Tags` | A topic tag; `tasks review-request` filters on it, as before. |
| `created via tasq create` default note | `## Progress` | Ordinary note. |
| `[tasq] Update: <file>` commit messages | nb's git history | Visible in `nb history`; nb's own are `[nb] Add:`/`[nb] Done:`. |

No other new sections. The `<!-- tasq: ... -->` comments mentioned in
`docs/file-format.md` are still TBD and are not written by any command today.

### The script writes, `tasq` reads losslessly

Everything: the title line (including `nb todo do`'s `# [x]` flip), `##
Description`, `## Project`, `## Due`, `## Related` with `### Merge requests`,
`## Tags` (status and priority tags become fields of the task; topic tags stay
tags), `## Progress`, `## Worktrees`, `## Sessions`. Sections `tasq` does not
know, and any stray line, round-trip byte for byte.

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
- `tasq worktree --create` defaults to plain `git worktree add`; gwm is one
  line of config (see above), not built in.
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
- [ ] `tasq doctor`: notebook found, `.index` verifies, nb and git detected,
      optional tools (`glow`, `claude`, `direnv`, `glab`) reported.
- [ ] Write `~/.config/tasq/config.toml` from the mapping above;
      `tasq config show` prints every key with the layer that set it.
- [ ] Forge token: `glab auth token` works in a shell, and
      `[forge.gitlab] token_cmd = "glab auth token"` is set (or
      `GITLAB_TOKEN`). Never paste the token into the config file.
- [ ] `tasq sync --dry-run`: read the planned creates/closes/notes against the
      tasks `tasks update` created; nothing is written. Then `tasq sync`.
- [ ] Inbox bridge: copy `examples/sources/inbox.md` to `~/.config/tasq/prompts/` and add
      the `inbox` `[[source]]` from `examples/sources/config.toml`. Each run is one headless
      Claude session (~90 s, ~$2), so use `tasq sync --source inbox` or `enabled = false`
      ([`docs/sources.md`](sources.md), "Headless Claude Code").
- [ ] Point herdr and Claude workflows at `tasq next` and `tasq pick <id>`
      (they set `TASQ_TASK_ID` for the session).
- [ ] Install the Claude Code plugin (`claude plugin marketplace add <repo>`,
      `claude plugin install tasq@tasq`, or `claude --plugin-dir
      plugins/claude`) and the status line from
      [`plugins/claude/README.md`](../plugins/claude/README.md).
- [ ] Shell completions: `tasq completions zsh > ~/.zfunc/_tasq` (bash and
      fish in `tasq completions --help`).
- [ ] Keep `tasks update-support` on the script (no Freshdesk in v1). Keep
      `tasks tlogs` until `examples/plugins/tasq-tlogs` is on your `PATH`;
      `tasq tlogs` then dispatches to it.
- [ ] Last: retire the old `/wrapup` and `/update-tasks` skills once every
      session is started by `tasq`. Do not delete `original/tasks`; it is the
      reference the tests are built against.

## Rollback

Nothing to roll back. Stop running `tasq`; the files are nb files, the index is
nb's index, the commits are in the notebook's git history. The only traces are
the `## Source` sections and `sync(...)` notes on synced tasks, which the
script and nb ignore. Delete the `## Source` section by hand if you want a
file byte-identical to what the script would have written.

## Side-by-side verification (one week, manual acceptance)

**Status: pending the author.** Run `tasks` and `tasq` alternately on the real
notebook for one week. Each day:

```sh
nb index verify                              # "Index corrupted" on stderr is the signal, not the exit code
tasq doctor                                  # every line OK or WARN with a hint; exit 0
git -C ~/.nb/home log --oneline -5           # one commit per write, [tasq] and [nb] interleaved
```

After every `tasq` write (`set`, `log`, `done`, `project`, `worktree`,
`session`, `mr`, `sync`), before the next command:

```sh
git -C ~/.nb/home diff HEAD~1 -- <file>      # only the edited section changed
```

A clean week is: no `Index corrupted`, no `FAIL` from `tasq doctor`, every
diff limited to the section the command named, and `tasks` still listing and
rendering every task `tasq` touched. Record the outcome in
`ruli/features/rust-rewrite/PROGRESS.md` under T-904.
