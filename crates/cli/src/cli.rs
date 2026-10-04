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
  tasq create \"Fix the build\" --prio A --due tomorrow --tag ci
  tasq set 12 in-progress    change status (or A/B/C for priority)
  tasq log 12 \"found the cause\"
  tasq done 12 \"merged\"
  tasq store info            where the tasks live
  tasq doctor                check config, notebook, nb and optional tools";

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
tag. The explicit flags can be combined and also combine with WORD.";

const CREATE_HELP: &str = "\
The file gets the sections the original script wrote, in its order:
Description, Project, Due, Related (with Merge requests), Tags, Progress.
Status defaults to workflow.default_status and priority to B. The first
progress note is --note, or `created via tasq create`.

--due accepts YYYY-MM-DD, today, tomorrow and yesterday. --project must be
an existing directory and is stored as an absolute path. --status done
creates the task already closed (`# [x]`, no status tag). Merge requests
need a title; until a forge lookup exists the title falls back to
`group/project!123` (GitLab) or `owner/repo#123` (GitHub).

Prints `[id] created: Title (#status #prio)`.";

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

    /// Project directory sessions start in (must exist).
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
