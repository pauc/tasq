//! Configuration model and layered loading.
//!
//! [`Config`] is the effective configuration every other module reads.
//! Nothing here performs I/O behind your back: [`Config::load`] takes a
//! [`LoadOptions`] that names the working directory, the home directory and
//! the environment, so tests run it against temporary directories.
//!
//! # Layers
//!
//! Later layers win, key by key (a file that sets only `store.name`
//! leaves every other key alone):
//!
//! 1. built-in defaults ([`Config::default`]);
//! 2. the global file, `config.toml` in the config directory: the one named
//!    by `--config-dir` / `TASQ_CONFIG_DIR` (which must then exist), else
//!    `$XDG_CONFIG_HOME/tasq`, else `~/.config/tasq` (see [`config_dir`]);
//! 3. the selected profile, `profiles/<name>.toml` in the config directory
//!    (the same shape as `config.toml`), chosen with `--profile` or
//!    `TASQ_PROFILE`; a `[profile.<name>]` table in any file is an error;
//! 4. the repository config, for the worktree commands only: the file in
//!    `repos/` of the config directory whose `path` is the main checkout
//!    named by [`LoadOptions::repo`]; it holds `path` and `[worktree]`;
//! 5. `TASQ_*` environment variables (see [`ENV_KEYS`]);
//! 6. explicit overrides passed by the command line (`--set key=value`).
//!
//! Nothing is read from the working directory or from a repository
//! (ADR-0031).
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
//! Keys whose default is "unset" (`work.default_project`, `launch.log`,
//! `launch.claude.prompt_file`, `report.summary.model`,
//! `report.summary.prompt_file`) are simply absent. So is `data_dir`, whose
//! default depends on the environment: [`Config::load`] fills it in with
//! `$XDG_DATA_HOME/tasq/stores`, else `~/.local/share/tasq/stores`
//! ([`default_data_dir`]), and records it in the defaults layer.
//!
//! ```toml
//! worktrees_enabled = false
//!
//! [store]
//! name = "default"
//!
//! [workflow]
//! statuses = ["in-progress", "ready", "waiting", "blocked", "later"]
//! default_status = "ready"
//!
//! [work]
//!
//! [launch]
//! default = "claude"
//! window = "auto"
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
//! detail_position = "right"
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
//!
//! [plugins]
//! enabled = []
//! timeout = 2
//!
//! [plugin]
//!
//! [worktree]
//! path = "{{project_parent}}/{{project_name}}-{{branch_safe}}"
//! branch = "{{id}}-{{slug}}"
//! base = "origin/HEAD"
//! on_create = "auto"
//! on_done = "release"
//! delete_branch = "safe"
//! symlinks = []
//! copies = []
//! templates = []
//!
//! [worktree.variables]
//!
//! [worktree.hooks]
//! pre-create = []
//! post-create = []
//! pre-remove = []
//! post-remove = []
//! ```

mod error;
mod load;
mod path;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Status, Workflow};
use crate::theme::Preset;
use crate::worktree::lifecycle::DeleteBranch;

pub use self::error::ConfigError;
pub use self::load::{
    ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_KEYS, ENV_PROFILE, ENV_SET, ENV_STORE_NAME, GLOBAL_FILE,
    Layer, LoadOptions, Loaded, Origin, REPOS_DIR, RepoConfig, config_dir, default_data_dir,
    named_config_dir, parse_env_set, repo_configs,
};
pub use self::path::{contract_tilde, expand_tilde};

/// The effective configuration.
///
/// Build it with [`Config::load`] (layered files, profile and environment)
/// or start from [`Config::default`]. Unknown keys anywhere in the document
/// are errors, so a typo never silently falls back to a default.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The directory holding the stores, each a subdirectory named by
    /// `store.name` (ADR-0026). `~` is expanded. Default: unset here,
    /// `$XDG_DATA_HOME/tasq/stores` (else `~/.local/share/tasq/stores`)
    /// after [`Config::load`]; `None` after a load only when neither
    /// `$XDG_DATA_HOME` nor a home directory is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    /// Whether worktrees exist at all (ADR 0022, ADR 0031). Off hides every
    /// worktree command, field and section display. Default: `false`.
    pub worktrees_enabled: bool,
    /// Which store under `data_dir` the tasks live in.
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
    /// The plugins told about events (`[plugins]`, ADR 0030).
    pub plugins: PluginsConfig,
    /// Each plugin's own settings (`[plugin.<name>]`), passed to it as they
    /// are: tasq checks no key inside, so a value can be anything TOML
    /// holds.
    pub plugin: BTreeMap<String, toml::Table>,
    /// Worktrees per task, off unless `worktrees_enabled` (`[worktree]`,
    /// ADR 0022).
    pub worktree: WorktreeConfig,
}

/// `[store]`: which store the tasks live in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoreConfig {
    /// The store's directory name under `data_dir`: one path component,
    /// neither empty nor `.`/`..`, no `/` ([`Config::load`] checks).
    /// Default: `default`.
    pub name: String,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            name: "default".to_owned(),
        }
    }
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
    /// Removed with ADR 0022; setting it is an error naming `[worktree]`.
    #[serde(skip_serializing, deserialize_with = "removed_for_worktree")]
    worktree_manager: Removed,
    /// Removed with ADR 0022; setting it is an error naming `[worktree]`.
    #[serde(skip_serializing, deserialize_with = "removed_for_worktree")]
    worktree_command: Removed,
}

/// A key that is not accepted (removed, or not implemented yet). It
/// deserialises only from its absence, so a file that sets it fails with the
/// file, line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Removed;

fn removed_for_worktree<'de, D: serde::Deserializer<'de>>(_: D) -> Result<Removed, D::Error> {
    Err(serde::de::Error::custom(
        "work.worktree_manager and work.worktree_command were removed; \
         worktrees are configured under [worktree] (docs/config.md, \"Worktrees\")",
    ))
}

fn moved_to_worktrees_enabled<'de, D: serde::Deserializer<'de>>(_: D) -> Result<Removed, D::Error> {
    Err(serde::de::Error::custom(
        "worktree.enabled moved to the top level: \
         set worktrees_enabled above any table (docs/config.md, \"Worktrees\")",
    ))
}

fn not_yet_copy_ignored<'de, D: serde::Deserializer<'de>>(_: D) -> Result<Removed, D::Error> {
    Err(serde::de::Error::custom(
        "worktree.copy_ignored is not implemented yet; \
         list the files under worktree.copies (docs/config.md, \"Worktrees\")",
    ))
}

/// `[worktree]`: worktrees per task (ADR 0022, `docs/config.md`).
///
/// Templates use `{{name}}` placeholders; the variables are listed in
/// `docs/config.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeConfig {
    /// Moved to [`Config::worktrees_enabled`] (ADR 0031); setting it is an
    /// error naming the new key.
    #[serde(skip_serializing, deserialize_with = "moved_to_worktrees_enabled")]
    pub(crate) enabled: Removed,
    /// Where a declared worktree lives, rendered when it is declared.
    /// Default: `{{project_parent}}/{{project_name}}-{{branch_safe}}`.
    pub path: String,
    /// The branch of a worktree declared without one. Default:
    /// `{{id}}-{{slug}}`.
    pub branch: String,
    /// The ref a new branch starts from. Default: `origin/HEAD`.
    pub base: String,
    /// Whether `tasq create` declares a worktree without `--worktree`.
    /// Default: `auto`.
    pub on_create: OnCreate,
    /// What closing a task does with its worktrees. Default: `release`.
    pub on_done: OnDone,
    /// Whether a release deletes branches. Default: `safe`.
    pub delete_branch: DeleteBranch,
    /// Paths in the project linked into a new worktree. Default: none.
    pub symlinks: Vec<String>,
    /// Paths in the project copied into a new worktree. Default: none.
    pub copies: Vec<String>,
    /// Not implemented yet (WT-303); setting it is an error.
    #[serde(skip_serializing, deserialize_with = "not_yet_copy_ignored")]
    pub(crate) copy_ignored: Removed,
    /// Files rendered into a new worktree. Default: none.
    pub templates: Vec<TemplateFile>,
    /// Extra template variables. Default: none.
    pub variables: BTreeMap<String, String>,
    /// The `{{port}}` block. Default: unset (no `{{port}}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<PortsConfig>,
    /// Commands run around creating and removing a worktree.
    pub hooks: WorktreeHooks,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            enabled: Removed,
            path: "{{project_parent}}/{{project_name}}-{{branch_safe}}".to_owned(),
            branch: "{{id}}-{{slug}}".to_owned(),
            base: "origin/HEAD".to_owned(),
            on_create: OnCreate::default(),
            on_done: OnDone::default(),
            delete_branch: DeleteBranch::Safe,
            symlinks: Vec::new(),
            copies: Vec::new(),
            copy_ignored: Removed,
            templates: Vec::new(),
            variables: BTreeMap::new(),
            ports: None,
            hooks: WorktreeHooks::default(),
        }
    }
}

/// `worktree.on_create`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnCreate {
    /// Declare a worktree for every task whose project is a git checkout.
    #[default]
    Auto,
    /// Declare one only with `--worktree`.
    Never,
}

/// `worktree.on_done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnDone {
    /// Remove clean worktrees and delete their branches when nothing is lost.
    #[default]
    Release,
    /// Leave worktrees alone.
    Keep,
}

/// One `[[worktree.templates]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFile {
    /// The template, relative to the config file that declares it.
    pub source: PathBuf,
    /// Where the rendered file goes, relative to the worktree.
    pub target: PathBuf,
}

/// `[worktree.ports]`: `{{port}}` is `base` plus a stable per-branch offset
/// below `span`, a multiple of `block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortsConfig {
    /// First port of the range.
    pub base: u16,
    /// Size of the range.
    pub span: u16,
    /// Ports reserved per worktree.
    pub block: u16,
}

/// `[worktree.hooks]`: command lines run around creating and removing a
/// worktree. `pre-create` and `post-remove` run in the main checkout,
/// `post-create` and `pre-remove` in the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct WorktreeHooks {
    /// Before `git worktree add`; a failure aborts. Default: none.
    pub pre_create: Vec<String>,
    /// After the worktree is provisioned; a failure is a warning.
    /// Default: none.
    pub post_create: Vec<String>,
    /// Before `git worktree remove`; a failure keeps the worktree.
    /// Default: none.
    pub pre_remove: Vec<String>,
    /// After the worktree is removed; a failure is a warning. Default: none.
    pub post_remove: Vec<String>,
}

/// `[launch]`: how `next` and `pick` open a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchConfig {
    /// Launcher name: `shell`, `claude`, `tmux`, `herdr`, or a plugin's.
    /// Not validated here because launchers are registered by other crates.
    /// Default: `claude`.
    pub default: String,
    /// Launcher for a session in a new window (`--window`, the TUI's
    /// `Ctrl+Enter` and `Shift+Enter`): `herdr`, `tmux`, or `auto` for
    /// whichever of the two the current terminal runs in. Default: `auto`.
    pub window: String,
    /// Where the session's environment comes from. Default: `direnv`.
    pub env: EnvStrategy,
    /// File every launch appends a trace to: the commands the launcher
    /// ran (`herdr`, `tmux`) with their output, and its warnings. `~` is
    /// expanded. Default: unset (no file; `-v` prints the trace instead).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<PathBuf>,
    /// Settings of the Claude Code launcher.
    pub claude: ClaudeLaunchConfig,
    /// Settings of the herdr launcher.
    pub herdr: HerdrLaunchConfig,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            default: "claude".to_owned(),
            window: "auto".to_owned(),
            env: EnvStrategy::Direnv,
            log: None,
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
    /// Disable OSC 8 hyperlinks in `view` and in the `ui` detail. Default:
    /// `false`.
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
    /// Where the TUI shows the selected task's detail (`Right`):
    /// `right` of the list or at the `bottom`, under it (ADR-0021).
    /// Default: `right`.
    pub detail_position: DetailPosition,
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

/// Where the TUI's detail pane goes (`ui.detail_position`, ADR-0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetailPosition {
    /// Beside the list, from 100 columns.
    #[default]
    Right,
    /// Under the list, from 20 rows.
    Bottom,
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
            detail_position: DetailPosition::Right,
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
    /// Command whose stdout is the API token (`glab config get token --host <host>`,
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
#[allow(
    clippy::struct_excessive_bools,
    reason = "five independent switches that a TOML table spells as booleans; an enum would not read better in the config file"
)]
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
    /// Review requests: skip merge requests whose author has one of these
    /// usernames (exact, `dependabot`). Default: none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_authors: Vec<String>,
    /// Review requests: whether draft merge requests become tasks. Default:
    /// `false` (skipped until marked ready).
    #[serde(default)]
    pub drafts: bool,
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

/// `[plugins]`: the plugins tasq runs on events (ADR 0030).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsConfig {
    /// Plugin names, each run as `tasq-<name>` from `PATH`, in this order.
    /// A name is a command name: not empty, no `/`. Default: none.
    pub enabled: Vec<String>,
    /// Seconds a plugin may take over one event before it is killed and
    /// reported as a warning. Default: 2.
    pub timeout: u64,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            timeout: 2,
        }
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
