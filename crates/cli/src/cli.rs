//! Command-line definition (clap derive).
//!
//! `tasq` with no subcommand lists open tasks; `tasq <word>` is shorthand
//! for `tasq list <word>`, where the word is a status, a tag or a priority,
//! exactly as the original script did. Global flags are accepted before or
//! after the subcommand.

// Help text is read on a terminal, where backticks around NO_COLOR or
// TASQ_PROFILE are noise rather than markup.
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

const LONG_ABOUT: &str = "\
Terminal task manager over nb markdown todos.

Tasks are the `*.todo.md` files of an nb notebook; `tasq` reads and writes
them directly, in the same format the original `tasks` script used, and
leaves index registration and git commits to nb when it is installed.

Every command accepts --json for machine-readable output. Exit codes: 0 on
success, 1 for an error you can fix (`tasq: <message>` on stderr), 2 for a
usage error or an internal failure.";

const EXAMPLES: &str = "\
Examples:
  tasq                       open tasks grouped by status
  tasq ready                 only the READY group
  tasq gitlab                grouped view of the tasks tagged #gitlab
  tasq A                     grouped view of the priority-A tasks
  tasq list --status waiting --tag support
  tasq list --all            open tasks, then the done ones
  tasq create \"Fix the build\" --prio A --due tomorrow --tag ci
  tasq set 12 in-progress    change status (or A/B/C for priority)
  tasq log 12 \"found the cause\"
  tasq done 12 \"merged\"
  tasq ui                    full-screen UI over the same tasks
  tasq summary               standup notes for the last working day
  tasq dates last week       the Monday and Friday, for scripts
  tasq store info            where the tasks live
  tasq doctor                check config, notebook, nb and optional tools
  tasq plugins list          tasq-<name> executables on PATH and the hooks
  tasq <name> [args]         run the plugin tasq-<name> (see docs/plugins.md)";

/// Top-level command line.
#[derive(Debug, Parser)]
#[command(
    name = "tasq",
    version,
    about = "Terminal task manager over nb markdown todos",
    long_about = LONG_ABOUT,
    after_help = EXAMPLES
)]
pub struct Cli {
    /// Flags every command accepts.
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Status, tag or priority to list; same as `tasq list <WORD>`.
    #[arg(value_name = "WORD")]
    pub word: Option<String>,

    /// The command to run; `list` when omitted.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Flags accepted by every command, before or after the subcommand.
/// Listed after the command's own options in `--help`.
#[derive(Debug, Clone, Args, Default)]
#[command(next_display_order = 900)]
pub struct GlobalArgs {
    /// Apply the `[profile.<NAME>]` block of the config files (also TASQ_PROFILE).
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Use FILE instead of the global config file (also TASQ_CONFIG).
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Override one config key, e.g. --set store.notebook=work. Repeatable.
    #[arg(long = "set", global = true, value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub set: Vec<(String, String)>,

    /// Print machine-readable JSON (every document carries "schema": 1).
    #[arg(long, global = true)]
    pub json: bool,

    /// When to colour the output: auto (only on a terminal, unless NO_COLOR is set), always or never.
    #[arg(long, global = true, value_enum, value_name = "WHEN", default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Never colour the output (same as --color never).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Print long output directly instead of through the pager (`ui.pager`).
    #[arg(long, global = true)]
    pub no_pager: bool,

    /// Show more detail (bookkeeping output, resolved paths). Repeatable.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

/// `--color` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ColorChoice {
    /// Colour when stdout is a terminal and NO_COLOR is unset.
    #[default]
    Auto,
    /// Always emit colour escapes.
    Always,
    /// Never emit colour escapes.
    Never,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List open tasks grouped by status (the default command).
    #[command(after_help = LIST_HELP)]
    List(ListArgs),

    /// Create a task.
    #[command(after_help = CREATE_HELP)]
    Create(CreateArgs),

    /// Set the status or the priority of a task, optionally logging a note.
    ///
    /// VALUE is a status of the workflow (in-progress, ready, ...) or a
    /// priority (A, B, C, with or without the #). Prints `[id] -> value`.
    Set {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// New status or priority.
        #[arg(value_name = "VALUE")]
        value: String,
        /// Progress note to append at the same time.
        #[arg(value_name = "NOTE")]
        note: Option<String>,
    },

    /// Append a dated progress note to a task.
    Log {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// The note.
        #[arg(value_name = "NOTE")]
        note: String,
    },

    /// Mark a task done (`# [x]`, status tag removed), optionally logging a final note.
    Done {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Final progress note, appended before closing.
        #[arg(value_name = "NOTE")]
        note: Option<String>,
    },

    /// Open a work session on the next task: the first in-progress one, else the first ready one.
    #[command(after_help = LAUNCH_HELP)]
    Next {
        /// Launcher to use [default: launch.default, or launch.detached with --detached].
        #[arg(long, value_name = "NAME")]
        launcher: Option<String>,
        /// Open the session in a new window (launch.detached: herdr or tmux) instead of here.
        #[arg(long)]
        detached: bool,
        /// With --detached: open the window without switching to it.
        #[arg(long, requires = "detached")]
        no_focus: bool,
        /// Print what would happen (directory, commands, prompt) and launch nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Open a work session on a specific task.
    #[command(after_help = LAUNCH_HELP)]
    Pick {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Launcher to use [default: launch.default, or launch.detached with --detached].
        #[arg(long, value_name = "NAME")]
        launcher: Option<String>,
        /// Open the session in a new window (launch.detached: herdr or tmux) instead of here.
        #[arg(long)]
        detached: bool,
        /// With --detached: open the window without switching to it.
        #[arg(long, requires = "detached")]
        no_focus: bool,
        /// Print what would happen (directory, commands, prompt) and launch nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Show a task: rendered with glow on a terminal, plain markdown otherwise.
    ///
    /// On a terminal with glow installed the markdown is rendered and links
    /// become OSC 8 hyperlinks (label only, URL hidden; GitLab merge requests
    /// and issues show as !123 / #123). Set ui.no_osc8 = true if your
    /// terminal cannot follow them: long lines then list their URLs below.
    /// Without glow, or when piped, the file is printed as is.
    View {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Print the file verbatim, never render.
        #[arg(long)]
        raw: bool,
    },

    /// Show or set the task's project directory.
    ///
    /// Sessions start there when the task tracks no existing worktree.
    /// Without PATH the tracked directory is printed, or
    /// `no project tracked (default: ...)` with work.default_project.
    Project {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Directory to track (must exist; stored as an absolute path).
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Track a git worktree used for the task, or create one.
    ///
    /// `tasq worktree <ID> <PATH>` records an existing directory and its
    /// current branch (idempotent). `tasq worktree <ID> --create <BRANCH>`
    /// makes the worktree with work.worktree_manager (git: `git worktree add`
    /// into `<project>-<branch>` next to the project; command: your own tool,
    /// e.g. work.worktree_command = "gwm create {new} {branch} --no-tmux -s",
    /// run in the project, printing the worktree path last) and tracks it.
    Worktree {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Existing worktree directory to track.
        #[arg(
            value_name = "PATH",
            required_unless_present = "create",
            conflicts_with = "create"
        )]
        path: Option<PathBuf>,
        /// Create a worktree for BRANCH (new or existing) and track it.
        #[arg(long, value_name = "BRANCH")]
        create: Option<String>,
    },

    /// Track an agent session on a task (idempotent by session id).
    Session {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// The session id (what `claude --resume` takes).
        #[arg(value_name = "SESSION_ID")]
        session_id: String,
        /// Free-text description.
        #[arg(value_name = "DESC")]
        description: Option<String>,
        /// Launcher the session belongs to, for the resume hint [default: launch.default].
        #[arg(long, value_name = "NAME")]
        launcher: Option<String>,
    },

    /// Track a merge request on a task (idempotent by URL).
    ///
    /// The title is resolved from the URL when possible (today: the short
    /// reference, `group/project!123` or `owner/repo#123`); pass TITLE to
    /// set it explicitly.
    Mr {
        /// Task id.
        #[arg(value_name = "ID")]
        id: String,
        /// Merge request URL.
        #[arg(value_name = "URL")]
        url: String,
        /// Title to record instead of resolving one.
        #[arg(value_name = "TITLE")]
        title: Option<String>,
    },

    /// Refresh tasks from the configured sources (merge requests to review, assigned issues, an LLM inbox).
    #[command(after_help = SYNC_HELP)]
    Sync {
        /// Only these sources (`[[source]] name`, repeatable). Without it, every enabled source with `auto = true`.
        #[arg(long, value_name = "NAME")]
        source: Vec<String>,
        /// Print the changes and write nothing (with --interactive: print the command and do not run it).
        #[arg(long)]
        dry_run: bool,
        /// Open a Claude Code briefing session running `/tasq:sync` in work.default_project instead of syncing here.
        #[arg(long, conflicts_with_all = ["source", "ids"])]
        interactive: bool,
        /// Re-check only these tasks against their sources instead of a full sweep.
        #[arg(value_name = "ID")]
        ids: Vec<String>,
    },

    /// Update a task from JSON (`tasq view --json` shape) on stdin or in FILE.
    ///
    /// The document is `{"schema": 1, "task": {...}}`; see docs/json.md.
    /// The store applies the differences it can express (status, priority,
    /// project, done, appended progress, worktrees, sessions, links) and
    /// refuses anything else without writing.
    Apply {
        /// File to read instead of stdin.
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,
    },

    /// Standup summary of what you worked on during a day, from the progress notes.
    #[command(after_help = SUMMARY_HELP)]
    Summary {
        /// The day: YYYY-MM-DD, today, yesterday, a weekday name or `last <weekday>` [default: the last working day].
        #[arg(value_name = "DAY")]
        day: Option<String>,
        /// Print the notes themselves, grouped per task, without summarizing.
        #[arg(long)]
        raw: bool,
    },

    /// Resolve a day or a date range to `FROM TO`, for scripts and plugins.
    #[command(after_help = DATES_HELP)]
    Dates {
        /// The spec, as one or several words (see below) [default: today].
        #[arg(value_name = "SPEC")]
        spec: Vec<String>,
    },

    /// Browse and edit the open tasks in a full-screen terminal UI.
    #[command(after_help = UI_HELP)]
    Ui,

    /// The store behind the tasks: where it is and how to sync it.
    #[command(subcommand)]
    Store(StoreCommand),

    /// Check the configuration, the notebook, nb and optional tools.
    ///
    /// Prints one line per check (OK, WARN or FAIL) with a hint for anything
    /// that is not OK, and exits 1 when any check failed.
    Doctor,

    /// Show the effective configuration.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Plugins: `tasq-<name>` executables on PATH and the configured hooks.
    #[command(subcommand, after_help = PLUGINS_HELP)]
    Plugins(PluginsCommand),

    /// Print a shell completion script to stdout
    ///
    /// bash:  tasq completions bash > ~/.local/share/bash-completion/completions/tasq
    /// zsh:   tasq completions zsh > ~/.zfunc/_tasq      (with ~/.zfunc in fpath)
    /// fish:  tasq completions fish > ~/.config/fish/completions/tasq.fish
    #[command(verbatim_doc_comment)]
    Completions {
        /// The shell to generate for.
        #[arg(value_enum)]
        shell: Shell,
    },
}

const LIST_HELP: &str = "\
With no filter, open tasks are grouped by status in workflow order
(IN PROGRESS, READY, WAITING, BLOCKED, LATER, then NO STATUS). Inside a group
tasks are sorted by priority, then due date (undated last), then id.

WORD is interpreted like the original script: a status prints that one
group, A/B/C prints the grouped view of that priority, anything else is a
tag. The explicit flags can be combined and also combine with WORD.

Done tasks are left out unless --all (a DONE group after the open ones,
sorted by priority, due date and id) or --done (only them) is given; the
other filters still apply. With --json the DONE group follows the open
tasks in the `tasks` array.";

const CREATE_HELP: &str = "\
The file gets the sections the original script wrote, in its order:
Description, Project, Due, Related (with Merge requests), Tags, Progress.
Status defaults to workflow.default_status and priority to B. The first
progress note is --note, or `created via tasq create`.

--due accepts YYYY-MM-DD, today, tomorrow and yesterday. --project must be
an existing directory and is stored as an absolute path; without it the
task's project is the directory tasq runs in, so `pick` always has
somewhere to start (change it later with `tasq project`). --status done
creates the task already closed (`# [x]`, no status tag). Merge requests
need a title; until a forge lookup exists the title falls back to
`group/project!123` (GitLab) or `owner/repo#123` (GitHub).

Prints `[id] created: Title (#status #prio)`.";

const LAUNCH_HELP: &str = "\
The task is set to in-progress, then a session starts in the first tracked
worktree that exists, else the task's project, else work.default_project.
When every tracked worktree is gone, the newest one is named and, on a
terminal, you are offered to recreate it on its recorded branch.

Launchers (--launcher, or launch.default): claude runs Claude Code with the
task prompt (template: launch.claude.prompt_file), through `direnv exec`
when launch.env = direnv and the directory's .envrc is allowed; shell execs
$SHELL there; tmux opens a new window (inside tmux only); herdr opens a
workspace with a Claude agent (inside herdr only); auto is herdr inside
herdr, else claude. --detached uses launch.detached instead (herdr, tmux,
or auto for whichever the terminal runs in) and --no-focus leaves the new
window in the background; launch.herdr.placement says whether a herdr
window is a workspace or a tab. The session gets TASQ_TASK_ID,
TASQ_NOTEBOOK and, when selected, TASQ_PROFILE.";

const SYNC_HELP: &str = "\
Every enabled [[source]] with `auto = true` (the default) runs in config
order; `--source NAME` (repeatable) runs exactly the named ones instead,
`auto` or not, which is how a source with `auto = false` (an LLM bridge
that costs a full session, say) is run on purpose. One failing source is
reported and does not stop the others (the exit code is 1 when any
failed). For each
source the items it reports are reconciled with the tasks: a new open item
becomes a task (with the source's tags and status, and a `## Source` line
for later matching), a tracked item that is done (merged, closed, approved
by you, reassigned, gone) logs a note and marks the task done. Tracked
tasks the sweep no longer lists are re-checked individually. With task ids,
only those tasks are re-checked. See docs/sources.md.

`--interactive` is the morning briefing in one command (the script's
`tasks update`): it replaces this process with `claude \"/tasq:sync\"` in
work.default_project, through `direnv exec` when launch.env is `direnv`
and the directory's .envrc is allowed, with TASQ_NOTEBOOK (and
TASQ_PROFILE) set so the session's `tasq` sees the same notebook. The
skill then runs `tasq sync` itself and reports. `--dry-run` prints the
command instead.";

const SUMMARY_HELP: &str = "\
Every progress note logged on DAY is collected, one bullet per task
(`- [id] Title (done) — note`, or one indented bullet per note), including
done tasks. Without DAY the last working day is used, so on a Monday you
get Friday. A weekday name means the most recent one, today included;
`last friday` means the one before today.

The notes are then distilled by report.summary.command (default
`claude -p`), which reads the prompt and the notes on stdin; the prompt is
the built-in template or report.summary.prompt_file, and
report.summary.model is passed as --model. The result is shown like `tasq
view` (glow on a terminal). --raw prints the notes themselves under a bold
header and never runs the command; report.summary.summarizer = \"raw\"
makes that the default. Nothing logged prints `Nothing logged on <day>.`.

--json: {day, header, summarizer, tasks: [{id, title, done, notes}],
notes, summary (null when raw)}.";

const DATES_HELP: &str = "\
SPEC is case-insensitive and may be split across arguments:
  (nothing), today            today
  yesterday, YYYY-MM-DD       that day
  monday ... sunday, mon ...  the most recent one, today included
  last <weekday>              the most recent one before today
  week, this week             Monday of this week to Friday (or today)
  last week                   Monday to Friday of the previous week
  month, this month           the 1st to today
  last month                  the whole previous month
  last N days                 the N days ending today
  <day> <day>                 both days and everything between

A range never extends past today. Output is `FROM TO` (YYYY-MM-DD); with
--json: {spec, from, to, days: [...], working_days: [...]}.";

const UI_HELP: &str = "\
The list is the grouped view of `tasq`; Right shows the selected task's
detail beside it (or, below 100 columns, in its place) and Left hides it
again. Keys: j/k move, g/G first
and last, / filter (text matches titles; #word is a status, tag or
priority as for `tasq <word>`), t status, p priority, l log a note, d mark
done (with an optional final note), E open the file in $VISUAL or $EDITOR,
Enter open a work session here (`tasq pick`), Ctrl+Enter open it in a new
window and switch to it, Shift+Enter open it in a new window and stay
(`tasq pick --detached [--no-focus]`, launch.detached), s run the sources
that run by default (`tasq sync`), S pick the sources to run (Space
toggles, Enter runs `tasq sync --source ...`), r reload, ? help, q quit.
Edits are the same operations as
`tasq set`, `log` and `done`. Group colours follow [ui.colors] (status
name, or `no-status`); NO_COLOR or --color never gives a monochrome UI.
Ctrl+Enter and Shift+Enter need a terminal with the kitty keyboard
protocol; elsewhere they are a plain Enter. Every key but Ctrl+C is an
action that [ui.keys] can rebind (`launch-detached = \"alt+enter\"`); the
? overlay shows the configured keys. See docs/config.md, \"Key bindings\".";

const PLUGINS_HELP: &str = "\
`tasq <name> [args...]` runs the executable tasq-<name> found on PATH when
<name> is not a built-in command, with the remaining arguments verbatim and
TASQ_BIN (this binary), TASQ_PROFILE, TASQ_CONFIG and TASQ_SET (the --set
flags, one per line) in its environment, so `$TASQ_BIN ... --json` inside
the plugin sees the same configuration. Built-in commands always win; a
plugin also wins over the bare `tasq <word>` filter, which stays available
as `tasq list <word>`.

Hooks are command lines under [hooks] in the config: post-create (after
`tasq create`), post-done (after `tasq done`) and pre-launch (before
`tasq next`/`pick` start a session; a failure aborts the launch). Each
gets {\"schema\": 1, \"hook\": ..., \"task\": {...}} on stdin and TASQ_HOOK,
TASQ_TASK_ID and TASQ_BIN in its environment. See docs/plugins.md.";

/// `tasq create` arguments.
#[derive(Debug, Clone, Args, Default)]
pub struct CreateArgs {
    /// Task title (the `# [ ] Title` line).
    #[arg(value_name = "TITLE")]
    pub title: String,

    /// `## Description` text.
    #[arg(long, value_name = "TEXT")]
    pub desc: Option<String>,

    /// Initial status, or `done` [default: workflow.default_status].
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    /// Priority A, B or C [default: B].
    #[arg(long, value_name = "PRIO")]
    pub prio: Option<String>,

    /// Due date: YYYY-MM-DD, today, tomorrow or yesterday.
    #[arg(long, value_name = "DATE")]
    pub due: Option<String>,

    /// Project directory sessions start in (must exist) [default: the current directory].
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,

    /// Topic tag, with or without the leading #. Repeatable.
    #[arg(long, value_name = "TAG")]
    pub tag: Vec<String>,

    /// Related link for `## Related`. Repeatable.
    #[arg(long, value_name = "URL")]
    pub related: Vec<String>,

    /// Merge request to track under `### Merge requests`. Repeatable.
    #[arg(long, value_name = "URL")]
    pub mr: Vec<String>,

    /// First progress note [default: "created via tasq create"].
    #[arg(long, value_name = "TEXT")]
    pub note: Option<String>,
}

/// `tasq list` arguments.
#[derive(Debug, Clone, Args, Default)]
pub struct ListArgs {
    /// Status, tag or priority (with or without the leading #).
    #[arg(value_name = "WORD")]
    pub word: Option<String>,

    /// Only tasks with this status.
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,

    /// Only tasks carrying this tag. Repeatable; every tag must match.
    #[arg(long, value_name = "TAG")]
    pub tag: Vec<String>,

    /// Only tasks with this priority (A, B or C).
    #[arg(long, value_name = "PRIO")]
    pub prio: Option<String>,

    /// Only tasks whose title contains TEXT (case-insensitive).
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,

    /// Include done tasks, in a DONE group after the open ones.
    #[arg(long, conflicts_with = "done")]
    pub all: bool,

    /// Only done tasks.
    #[arg(long)]
    pub done: bool,
}

/// `tasq store ...`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum StoreCommand {
    /// Print the notebook path, task count and how ids behave.
    Info,
    /// Exchange commits with the notebook's remote (`nb sync`, or
    /// `git pull --rebase && git push` without nb).
    Sync,
}

/// `tasq config ...`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration as TOML, each key annotated with
    /// the layer that set it (defaults, a file, a profile, env or --set).
    Show,
}

/// `tasq plugins ...`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum PluginsCommand {
    /// List the `tasq-<name>` executables on PATH (first match per name)
    /// and the hooks configured under `[hooks]`.
    List,
}

/// Parses `KEY=VALUE` for `--set`.
fn parse_key_value(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.trim().is_empty() => {
            Ok((key.trim().to_owned(), value.to_owned()))
        }
        _ => Err(format!("expected KEY=VALUE, got {text:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn bare_word_is_a_list_filter() {
        let cli = Cli::try_parse_from(["tasq", "ready"]).unwrap();
        assert_eq!(cli.word.as_deref(), Some("ready"));
        assert!(cli.command.is_none());
        let cli = Cli::try_parse_from(["tasq"]).unwrap();
        assert_eq!(cli.word, None);
        assert!(cli.command.is_none());
    }

    #[test]
    fn global_flags_before_and_after_the_subcommand() {
        let cli = Cli::try_parse_from(["tasq", "--json", "store", "info"]).unwrap();
        assert!(cli.global.json);
        let cli = Cli::try_parse_from(["tasq", "store", "info", "--json", "-vv"]).unwrap();
        assert!(cli.global.json);
        assert_eq!(cli.global.verbose, 2);
        let cli = Cli::try_parse_from(["tasq", "--no-color", "ready"]).unwrap();
        assert!(cli.global.no_color);
        assert_eq!(cli.word.as_deref(), Some("ready"));
    }

    #[test]
    fn set_parses_key_value() {
        let cli = Cli::try_parse_from([
            "tasq",
            "--set",
            "store.notebook=work",
            "--set",
            "ui.no_osc8=true",
        ])
        .unwrap();
        assert_eq!(
            cli.global.set,
            vec![
                ("store.notebook".to_owned(), "work".to_owned()),
                ("ui.no_osc8".to_owned(), "true".to_owned())
            ]
        );
        assert!(Cli::try_parse_from(["tasq", "--set", "nonsense"]).is_err());
        assert!(Cli::try_parse_from(["tasq", "--set", "=x"]).is_err());
        assert_eq!(
            parse_key_value("a=b=c").unwrap(),
            ("a".to_owned(), "b=c".to_owned())
        );
    }

    #[test]
    fn list_flags() {
        let cli = Cli::try_parse_from([
            "tasq", "list", "gitlab", "--status", "ready", "--tag", "a", "--tag", "b", "--prio",
            "A",
        ])
        .unwrap();
        let Some(Command::List(args)) = cli.command else {
            panic!("expected list");
        };
        assert_eq!(args.word.as_deref(), Some("gitlab"));
        assert_eq!(args.status.as_deref(), Some("ready"));
        assert_eq!(args.tag, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(args.prio.as_deref(), Some("A"));
    }
}
