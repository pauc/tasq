//! Configuration model and layered loading.
//!
//! [`Config`] is the effective configuration every other module reads.
//! Nothing here performs I/O behind your back: [`Config::load`] takes a
//! [`LoadOptions`] that names the working directory, the home directory and
//! the environment, so tests run it against temporary directories.
//!
//! # Layers
//!
//! Later layers win, key by key (a file that sets only `store.notebook`
//! leaves every other key alone):
//!
//! 1. built-in defaults ([`Config::default`]);
//! 2. the global file, `$XDG_CONFIG_HOME/tasq/config.toml` or
//!    `~/.config/tasq/config.toml`, replaced by `TASQ_CONFIG` / `--config`
//!    when given (that file must then exist);
//! 3. the nearest `.tasq.toml` walking up from the working directory (see
//!    [`project_file`] for the stop rule);
//! 4. the selected profile, a `[profile.<name>]` block from any of the files
//!    above, chosen with `--profile` or `TASQ_PROFILE`;
//! 5. `TASQ_*` environment variables (see [`ENV_KEYS`]);
//! 6. explicit overrides passed by the command line (`--set key=value`).
//!
//! Each layer is a partial TOML document. They are deep-merged as TOML
//! tables, then the result is deserialised into [`Config`] once, so the
//! typed defaults live in exactly one place (the `Default` impls below).
//! Every file is also deserialised on its own first, which is what turns an
//! unknown key or a misspelt status into an error with that file's name,
//! line and column.
//!
//! # Default configuration
//!
//! Serialising [`Config::default`] yields the reference document below.
//! Keys whose default is "unset" (`work.default_project`,
//! `launch.claude.prompt_file`, `report.summary.model`,
//! `report.summary.prompt_file`) are simply absent.
//!
//! ```toml
//! [store]
//! kind = "nb"
//! notebook = "home"
//! bookkeeper = "auto"
//!
//! [workflow]
//! statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
//! default_status = "ready"
//!
//! [work]
//! worktree_manager = "git"
//!
//! [launch]
//! default = "claude"
//! detached = "auto"
//! env = "direnv"
//!
//! [launch.claude]
//!
//! [launch.herdr]
//! placement = "auto"
//!
//! [ui]
//! pager = "less -RFX"
//! no_osc8 = false
//! glow_style = "dark"
//! week_start = "monday"
//! due_format = "relative"
//!
//! [ui.colors]
//!
//! [ui.keys]
//!
//! [ui.theme]
//! preset = "dark"
//!
//! [ui.theme.colors]
//!
//! [forge]
//!
//! [report.summary]
//! summarizer = "llm"
//! command = "claude -p"
//!
//! [hooks]
//! post-create = []
//! post-done = []
//! pre-launch = []
//! ```

mod error;
mod load;
mod path;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Status, Workflow};
use crate::theme::Preset;

pub use self::error::ConfigError;
pub use self::load::{
    ENV_CONFIG, ENV_KEYS, ENV_PROFILE, ENV_SET, GLOBAL_FILE, Layer, LoadOptions, Loaded, Origin,
    PROJECT_FILE, global_file, parse_env_set, project_file,
};
pub use self::path::expand_tilde;

/// The effective configuration.
///
/// Build it with [`Config::load`] (layered files, profile and environment)
/// or start from [`Config::default`]. Unknown keys anywhere in the document
/// are errors, so a typo never silently falls back to a default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Where tasks live and who maintains the notebook's bookkeeping.
    pub store: StoreConfig,
    /// The status workflow.
    pub workflow: WorkflowConfig,
    /// Where sessions start and how worktrees are made.
    pub work: WorkConfig,
    /// Which launcher `next`/`pick` use and how it gets its environment.
    pub launch: LaunchConfig,
    /// Terminal output preferences.
    pub ui: UiConfig,
    /// Named forge clients (`[forge.<name>]`), referenced by sources.
    pub forge: BTreeMap<String, ForgeConfig>,
    /// External sources synchronised by `tasq sync` (`[[source]]`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<SourceConfig>,
    /// Report settings.
    pub report: ReportConfig,
    /// Commands run around CLI events (`[hooks]`).
    pub hooks: HooksConfig,
}

/// `[store]`: where tasks live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoreConfig {
    /// Store implementation. Default: `nb`.
    pub kind: StoreKind,
    /// nb notebook name (resolved to `~/.nb/<notebook>` or asked of nb).
    /// Default: `home`, as in the original script.
    pub notebook: String,
    /// Who maintains `.index` and git commits. Default: `auto`.
    pub bookkeeper: Bookkeeper,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            kind: StoreKind::Nb,
            notebook: "home".to_owned(),
            bookkeeper: Bookkeeper::Auto,
        }
    }
}

/// Store implementations. Only nb-compatible notebooks exist today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreKind {
    /// An nb notebook: markdown todos plus a `.index` file.
    #[default]
    Nb,
}

/// Who maintains the notebook's `.index` and git history after a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bookkeeper {
    /// Use nb when it is on `PATH`, otherwise the native fallback.
    #[default]
    Auto,
    /// Always shell out to nb (`nb index add`, `nb git checkpoint`).
    Nb,
    /// Never spawn nb: append to `.index` and commit with git directly.
    Native,
}

/// `[workflow]`: the ordered status list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkflowConfig {
    /// Statuses in display and `next` search order. Each must be lowercase
    /// kebab-case. Default: `in-progress, ready, waiting, blocked, later`.
    pub statuses: Vec<Status>,
    /// Status given to new tasks. Must be one of `statuses`. Default: `ready`.
    pub default_status: Status,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            statuses: Status::DEFAULTS.to_vec(),
            default_status: Status::READY,
        }
    }
}

impl WorkflowConfig {
    /// The configured statuses as a [`Workflow`].
    pub fn workflow(&self) -> Workflow {
        Workflow::new(self.statuses.clone())
    }
}

/// `[work]`: where sessions start.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkConfig {
    /// Directory a session starts in when the task tracks neither a
    /// worktree nor a project. `~` is expanded. Default: unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_project: Option<PathBuf>,
    /// How `tasq worktree --create` makes a worktree. Default: `git`.
    pub worktree_manager: WorktreeManager,
    /// The command run by the `command` manager, a template with `{branch}`,
    /// `{project}` and `{new}` (`-b` for a branch that does not exist yet;
    /// `{new:<text>}` for another flag). It runs inside the project and must
    /// print the worktree path as its last line. Required when
    /// `worktree_manager = "command"`. Default: unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_command: Option<String>,
}

/// How `tasq worktree --create` makes a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeManager {
    /// `git worktree add` into `<project>-<branch>` next to the project.
    #[default]
    Git,
    /// A user-supplied command (`work.worktree_command`), for tools such as
    /// gwm that provision the new worktree (linked files, `.envrc`, hooks).
    Command,
}

/// `[launch]`: how `next` and `pick` open a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchConfig {
    /// Launcher name: `shell`, `claude`, `tmux`, `herdr`, or a plugin's.
    /// Not validated here because launchers are registered by other crates.
    /// Default: `claude`.
    pub default: String,
    /// Launcher for a session in a new window (`--detached`, the TUI's
    /// `Ctrl+Enter` and `Shift+Enter`): `herdr`, `tmux`, or `auto` for
    /// whichever of the two the current terminal runs in. Default: `auto`.
    pub detached: String,
    /// Where the session's environment comes from. Default: `direnv`.
    pub env: EnvStrategy,
    /// Settings of the Claude Code launcher.
    pub claude: ClaudeLaunchConfig,
    /// Settings of the herdr launcher.
    pub herdr: HerdrLaunchConfig,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            default: "claude".to_owned(),
            detached: "auto".to_owned(),
            env: EnvStrategy::Direnv,
            claude: ClaudeLaunchConfig::default(),
            herdr: HerdrLaunchConfig::default(),
        }
    }
}

/// Where a launched session gets its environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvStrategy {
    /// The environment of the shell that ran `tasq`.
    Inherit,
    /// The working directory's own environment via `direnv exec <dir>`.
    #[default]
    Direnv,
}

/// `[launch.herdr]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HerdrLaunchConfig {
    /// Where a herdr session opens. Default: `auto`.
    pub placement: Placement,
}

/// What "a new window" is in herdr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// A tab in the workspace already holding the task's directory, else
    /// a new workspace (the original script's rule).
    #[default]
    Auto,
    /// Always a new workspace.
    Workspace,
    /// Always a tab: in the holding workspace, else in the current one.
    Tab,
}

/// `[launch.claude]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaudeLaunchConfig {
    /// Prompt template overriding the built-in one. `~` is expanded.
    /// Default: unset (built-in template).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_file: Option<PathBuf>,
}

/// `[ui]`: terminal output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// Pager command for long output on a terminal. Default: `less -RFX`.
    pub pager: String,
    /// Disable OSC 8 hyperlinks in `view`. Default: `false`.
    pub no_osc8: bool,
    /// `glow` style passed with `-s`. Default: `dark`.
    pub glow_style: String,
    /// Colour overrides, element name to colour spec; the TUI defines the
    /// names. Default: empty.
    pub colors: BTreeMap<String, String>,
    /// Key binding overrides, action name to key spec(s); the TUI defines
    /// the action names and the key syntax and parses the strings
    /// (ADR-0013). An action that is not listed keeps its default keys;
    /// an empty list unbinds it. Default: empty.
    pub keys: BTreeMap<String, KeySpec>,
    /// The first column of the TUI's calendar picker (ADR-0017).
    /// Default: `monday`.
    pub week_start: WeekStart,
    /// How list rows show a due date: `relative` (`overdue 3d`), `iso`
    /// (`due 2026-10-03`) or `both` (ADR-0019). Default: `relative`.
    pub due_format: DueFormat,
    /// `[ui.theme]`: the colour theme of both front ends (ADR-0018).
    pub theme: ThemeConfig,
}

/// `[ui.theme]`: a built-in preset and per-role overrides, under which
/// `[ui.colors]` still overrides by status name (ADR-0018).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// The built-in theme: `dark`, `light`, `solarized`, `gruvbox` or
    /// `mono`. Default: `dark`.
    pub preset: Preset,
    /// Colour per role (`chip-bg`, `focus`, `link`, ...; the role names are
    /// `tasq_core::theme::Role::key`). Default: empty.
    pub colors: BTreeMap<String, String>,
}

/// The day a week starts on, for the calendar picker's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    /// ISO weeks, as in Europe.
    #[default]
    Monday,
    /// `tuesday`.
    Tuesday,
    /// `wednesday`.
    Wednesday,
    /// `thursday`.
    Thursday,
    /// `friday`.
    Friday,
    /// `saturday`, as in parts of the Middle East.
    Saturday,
    /// `sunday`, as in the US.
    Sunday,
}

impl WeekStart {
    /// The chrono weekday.
    pub fn weekday(self) -> chrono::Weekday {
        match self {
            Self::Monday => chrono::Weekday::Mon,
            Self::Tuesday => chrono::Weekday::Tue,
            Self::Wednesday => chrono::Weekday::Wed,
            Self::Thursday => chrono::Weekday::Thu,
            Self::Friday => chrono::Weekday::Fri,
            Self::Saturday => chrono::Weekday::Sat,
            Self::Sunday => chrono::Weekday::Sun,
        }
    }
}

/// How a list row shows a due date (`ui.due_format`, ADR-0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DueFormat {
    /// Relative to today: `overdue 3d`, `due today`, `due in 4d`.
    #[default]
    Relative,
    /// The date: `due 2026-10-03`.
    Iso,
    /// Both: `overdue 3d, 2026-10-03`.
    Both,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            pager: "less -RFX".to_owned(),
            no_osc8: false,
            glow_style: "dark".to_owned(),
            colors: BTreeMap::new(),
            keys: BTreeMap::new(),
            week_start: WeekStart::Monday,
            due_format: DueFormat::Relative,
            theme: ThemeConfig::default(),
        }
    }
}

/// The keys of one `[ui.keys]` action: a single key (`"alt+enter"`) or a
/// list (`["j", "down"]`, `[]` to unbind). The strings are opaque here;
/// the TUI parses them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    /// One key.
    One(String),
    /// Zero or more keys.
    Many(Vec<String>),
}

impl KeySpec {
    /// The key strings, one or many.
    pub fn keys(&self) -> &[String] {
        match self {
            Self::One(key) => std::slice::from_ref(key),
            Self::Many(keys) => keys,
        }
    }
}

/// `[forge.<name>]`: one GitLab or GitHub client shared by sources.
///
/// `kind` and `host` may be omitted in a file. [`Config::load`] fills them in:
/// `kind` from the block name when it is `gitlab` or `github`, `host` from
/// the kind (`gitlab.com`, `github.com`). After a successful load both are
/// always `Some`; they are only `None` on a hand-built value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ForgeConfig {
    /// Which API the host speaks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ForgeKind>,
    /// Host name without scheme, for example `gitlab.example.com`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Command whose stdout is the API token (`glab auth token`,
    /// `gh auth token`). Default: unset, meaning `GITLAB_TOKEN` /
    /// `GITHUB_TOKEN` from the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_cmd: Option<String>,
    /// API base URL, for self-hosted instances with an unusual layout or
    /// for tests. Default: `https://<host>/api/v4` (GitLab),
    /// `https://api.github.com` for `github.com`, else
    /// `https://<host>/api/v3` (GitHub Enterprise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Forge APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    /// GitLab REST API.
    Gitlab,
    /// GitHub REST API.
    Github,
}

impl ForgeKind {
    /// The public instance of this forge, used when `host` is omitted.
    pub fn default_host(self) -> &'static str {
        match self {
            Self::Gitlab => "gitlab.com",
            Self::Github => "github.com",
        }
    }

    /// The spelling used in config files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gitlab => "gitlab",
            Self::Github => "github",
        }
    }

    /// The kind a forge block is assumed to be from its name alone.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "gitlab" => Some(Self::Gitlab),
            "github" => Some(Self::Github),
            _ => None,
        }
    }
}

/// `[[source]]`: one external source for `tasq sync`.
///
/// Which fields apply depends on `kind`; [`Config::load`] checks that the
/// required ones are present (see [`SourceKind`]).
// Four independent switches (`enabled`, `auto`, `create_new`,
// `close_when_done`) that a TOML table spells as booleans; an enum would
// not read better in the config file.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    /// Unique name, recorded in each task's `## Source` section.
    pub name: String,
    /// Source implementation.
    pub kind: SourceKind,
    /// `[forge.<name>]` block to use. Required by the forge-backed kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<String>,
    /// Command run by `llm-bridge`; must print a JSON array of items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Prompt file fed to the `llm-bridge` command. `~` is expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_file: Option<PathBuf>,
    /// Tags added to every task this source creates. Default: none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Status of tasks this source creates. Default: `workflow.default_status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// Whether `sync` runs this source. Default: `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether a bare `tasq sync` (and the TUI's sync-all key) includes
    /// this source. Default: `true`. With `false` it runs only when named
    /// with `--source` or picked in the TUI's source picker; `enabled`
    /// still decides whether it can run at all.
    #[serde(default = "default_true")]
    pub auto: bool,
    /// Create tasks for new items. Default: `true`. With `false` the source
    /// only updates tasks that already exist (the plan's `flag_only`).
    #[serde(default = "default_true")]
    pub create_new: bool,
    /// Log a note and mark the task done when its item is done (merged,
    /// closed, reassigned, approved). Default: `true`.
    #[serde(default = "default_true")]
    pub close_when_done: bool,
    /// Tag added to matched open tasks that lack it (`review-request`).
    /// Default: unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    /// Title template for new tasks: `{title}`, `{iid}` (number), `{project}`
    /// (`group/project` or `owner/repo`). Default per kind: `Review MR !{iid}:
    /// {title}` / `Review PR #{iid}: {title}` for review requests, `#{iid}:
    /// {title}` for work items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Work items: only those carrying one of these labels. Default: none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Work items: skip those carrying one of these labels. Default: none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_labels: Vec<String>,
    /// Only items from these projects (`group/project`, a group prefix with a
    /// trailing `/`, or `owner/repo`). Default: every project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Source implementations, by the `kind` key of a `[[source]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// GitLab merge requests where I am a requested reviewer. Needs `forge`.
    GitlabReviewRequests,
    /// GitLab issues and work items assigned to me. Needs `forge`.
    GitlabWorkItems,
    /// GitHub pull requests where I am a requested reviewer. Needs `forge`.
    GithubReviewRequests,
    /// GitHub issues assigned to me. Needs `forge`.
    GithubWorkItems,
    /// A command (typically an LLM) that prints items as JSON. Needs `command`.
    LlmBridge,
}

impl SourceKind {
    /// The forge API this kind talks to, if any.
    pub fn forge_kind(self) -> Option<ForgeKind> {
        match self {
            Self::GitlabReviewRequests | Self::GitlabWorkItems => Some(ForgeKind::Gitlab),
            Self::GithubReviewRequests | Self::GithubWorkItems => Some(ForgeKind::Github),
            Self::LlmBridge => None,
        }
    }

    /// The spelling used in config files, for messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitlabReviewRequests => "gitlab-review-requests",
            Self::GitlabWorkItems => "gitlab-work-items",
            Self::GithubReviewRequests => "github-review-requests",
            Self::GithubWorkItems => "github-work-items",
            Self::LlmBridge => "llm-bridge",
        }
    }
}

/// `[report]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReportConfig {
    /// `tasq summary` settings.
    pub summary: SummaryConfig,
}

/// `[report.summary]`: how the standup summary is produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SummaryConfig {
    /// `raw` prints the progress notes; `llm` pipes them through `command`.
    /// Default: `llm`.
    pub summarizer: Summarizer,
    /// Command that reads the rendered prompt (instructions plus the notes)
    /// on stdin and prints the summary. Default: `claude -p`.
    pub command: String,
    /// Model passed to the command as `--model <model>` when set.
    /// Default: unset (the command's own default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Prompt template overriding the built-in one (`{{day}}`, `{{date}}`
    /// and `{{notes}}` placeholders). `~` is expanded. Default: unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_file: Option<PathBuf>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            summarizer: Summarizer::Llm,
            command: "claude -p".to_owned(),
            model: None,
            prompt_file: None,
        }
    }
}

/// `[hooks]`: command lines run by the CLI around task events, the
/// out-of-process plugin surface of ADR-0006.
///
/// Each entry is one command line, split like a shell command (quotes
/// allowed, no shell) with `~` expanded in the program. The command gets a
/// JSON document on stdin (`{"schema": 1, "hook": "<name>", "task": {...}}`
/// plus event-specific fields) and `TASQ_HOOK`, `TASQ_TASK_ID` and
/// `TASQ_BIN` in its environment. Running them is the CLI's job; the core
/// only holds the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct HooksConfig {
    /// After `tasq create` wrote the task. A failure is a warning.
    /// Default: none.
    pub post_create: Vec<String>,
    /// After `tasq done` closed the task. A failure is a warning.
    /// Default: none.
    pub post_done: Vec<String>,
    /// Before `tasq next`/`tasq pick` start a session, with the resolved
    /// working directory and launcher. A non-zero exit aborts the launch.
    /// Default: none.
    pub pre_launch: Vec<String>,
}

impl HooksConfig {
    /// The configured hooks as `(name, commands)` pairs, in event order.
    pub fn entries(&self) -> [(&'static str, &[String]); 3] {
        [
            ("post-create", &self.post_create),
            ("post-done", &self.post_done),
            ("pre-launch", &self.pre_launch),
        ]
    }

    /// Whether any hook is configured.
    pub fn is_empty(&self) -> bool {
        self.entries()
            .iter()
            .all(|(_, commands)| commands.is_empty())
    }
}

/// Summary strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Summarizer {
    /// Print the progress notes verbatim.
    Raw,
    /// Distil them with `report.summary.command`.
    #[default]
    Llm,
}
