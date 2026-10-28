//! Layered loading of [`Config`] with provenance.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::{Table, Value};

use super::{Config, ConfigError, ForgeKind, SourceKind, expand_tilde};

/// Name of the global file inside the config directory.
pub const GLOBAL_FILE: &str = "config.toml";
/// Directory of profile files inside the config directory: the profile
/// `<name>` is `profiles/<name>.toml` (ADR-0031).
pub const PROFILES_DIR: &str = "profiles";
/// Directory of repository configs inside the config directory: each
/// `repos/*.toml` names one main checkout in `path` (ADR-0031).
pub const REPOS_DIR: &str = "repos";
/// Environment variable naming the config directory (ADR-0031).
pub const ENV_CONFIG_DIR: &str = "TASQ_CONFIG_DIR";
/// Environment variable selecting a profile (`profiles/<name>.toml`).
pub const ENV_PROFILE: &str = "TASQ_PROFILE";
/// Environment variable carrying `key=value` overrides, one per line, the
/// same way `--set` does. It is how `tasq` hands its `--set` flags to the
/// plugins and hooks it runs (see [`parse_env_set`]).
pub const ENV_SET: &str = "TASQ_SET";
/// Environment variable overriding `data_dir` (also what a launched session
/// receives, resolved and absolute).
pub const ENV_DATA_DIR: &str = "TASQ_DATA_DIR";
/// Environment variable overriding `store.name` (also what a launched
/// session receives).
pub const ENV_STORE_NAME: &str = "TASQ_STORE_NAME";

/// Environment variables that override single keys, as `(variable, key)`.
///
/// | Variable | Key |
/// |----------|-----|
/// | `TASQ_DATA_DIR` | `data_dir` |
/// | `TASQ_STORE_NAME` | `store.name` |
/// | `TASQ_DEFAULT_PROJECT` | `work.default_project` |
/// | `TASQ_LAUNCHER` | `launch.default` |
/// | `TASQ_LAUNCH_WINDOW` | `launch.window` |
/// | `TASQ_LAUNCH_ENV` | `launch.env` |
/// | `TASQ_LAUNCH_LOG` | `launch.log` |
/// | `TASQ_HERDR_PLACEMENT` | `launch.herdr.placement` |
/// | `TASQ_PAGER` | `ui.pager` |
/// | `TASQ_NO_OSC8` | `ui.no_osc8` |
/// | `TASQ_GLOW_STYLE` | `ui.glow_style` |
/// | `TASQ_WEEK_START` | `ui.week_start` |
/// | `TASQ_DUE_FORMAT` | `ui.due_format` |
/// | `TASQ_DETAIL_POSITION` | `ui.detail_position` |
/// | `TASQ_THEME` | `ui.theme.preset` |
/// | `TASQ_SUMMARIZER` | `report.summary.summarizer` |
/// | `TASQ_SUMMARY_MODEL` | `report.summary.model` |
/// | `TASQ_SUMMARY_COMMAND` | `report.summary.command` |
/// | `TASQ_SUMMARY_PROMPT_FILE` | `report.summary.prompt_file` |
/// | `TASQ_WORKTREES_ENABLED` | `worktrees_enabled` |
/// | `TASQ_WORKTREE_PATH` | `worktree.path` |
/// | `TASQ_WORKTREE_BRANCH` | `worktree.branch` |
/// | `TASQ_WORKTREE_BASE` | `worktree.base` |
/// | `TASQ_WORKTREE_ON_CREATE` | `worktree.on_create` |
/// | `TASQ_WORKTREE_ON_DONE` | `worktree.on_done` |
/// | `TASQ_WORKTREE_DELETE_BRANCH` | `worktree.delete_branch` |
///
/// Values are converted like `--set` values (see [`LoadOptions::overrides`]).
/// A variable set to the empty string is treated as unset. [`ENV_CONFIG_DIR`],
/// [`ENV_PROFILE`] and [`ENV_SET`] are not keys and are handled separately;
/// [`ENV_SET`] entries belong to the same layer as these variables and win
/// over them for the same key.
pub const ENV_KEYS: &[(&str, &str)] = &[
    (ENV_DATA_DIR, "data_dir"),
    (ENV_STORE_NAME, "store.name"),
    ("TASQ_DEFAULT_PROJECT", "work.default_project"),
    ("TASQ_LAUNCHER", "launch.default"),
    ("TASQ_LAUNCH_WINDOW", "launch.window"),
    ("TASQ_LAUNCH_ENV", "launch.env"),
    ("TASQ_LAUNCH_LOG", "launch.log"),
    ("TASQ_HERDR_PLACEMENT", "launch.herdr.placement"),
    ("TASQ_PAGER", "ui.pager"),
    ("TASQ_NO_OSC8", "ui.no_osc8"),
    ("TASQ_GLOW_STYLE", "ui.glow_style"),
    ("TASQ_WEEK_START", "ui.week_start"),
    ("TASQ_DUE_FORMAT", "ui.due_format"),
    ("TASQ_DETAIL_POSITION", "ui.detail_position"),
    ("TASQ_THEME", "ui.theme.preset"),
    ("TASQ_SUMMARIZER", "report.summary.summarizer"),
    ("TASQ_SUMMARY_MODEL", "report.summary.model"),
    ("TASQ_SUMMARY_COMMAND", "report.summary.command"),
    ("TASQ_SUMMARY_PROMPT_FILE", "report.summary.prompt_file"),
    ("TASQ_WORKTREES_ENABLED", "worktrees_enabled"),
    ("TASQ_WORKTREE_PATH", "worktree.path"),
    ("TASQ_WORKTREE_BRANCH", "worktree.branch"),
    ("TASQ_WORKTREE_BASE", "worktree.base"),
    ("TASQ_WORKTREE_ON_CREATE", "worktree.on_create"),
    ("TASQ_WORKTREE_ON_DONE", "worktree.on_done"),
    ("TASQ_WORKTREE_DELETE_BRANCH", "worktree.delete_branch"),
];

/// Everything [`Config::load`] needs, injected so loading is pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadOptions {
    /// The working directory, absolute: a relative `--config-dir` or
    /// [`ENV_CONFIG_DIR`] starts here. Nothing is read from it.
    pub cwd: PathBuf,
    /// The user's home, for `~` expansion and the default config directory.
    /// `None` when `$HOME` is unset.
    pub home: Option<PathBuf>,
    /// The process environment (or any subset of it).
    pub env: BTreeMap<String, String>,
    /// Config directory (`--config-dir`). Wins over [`ENV_CONFIG_DIR`].
    pub config_dir: Option<PathBuf>,
    /// Profile to apply (`--profile`). Wins over [`ENV_PROFILE`].
    pub profile: Option<String>,
    /// The main checkout of the repository the worktree commands work on,
    /// absolute and already resolved by the caller. Selects the
    /// repository config whose `path` is exactly this directory; `None`
    /// reads no repository config.
    pub repo: Option<PathBuf>,
    /// Final overrides as `(key path, value)`, for example
    /// `("ui.no_osc8", "true")`. The value is converted to the type of the
    /// key: `true`/`false`/`yes`/`no`/`1`/`0`/`on`/`off` for booleans, a
    /// comma-separated list for arrays, text otherwise. Unknown keys are
    /// errors.
    pub overrides: Vec<(String, String)>,
}

impl LoadOptions {
    /// Options for `cwd` with no home, no environment and nothing else set.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            ..Self::default()
        }
    }

    /// Options from the real process: current directory, `$HOME` and the
    /// whole environment. `config_dir`, `profile` and `overrides` stay
    /// empty for the command line to fill.
    ///
    /// Reason: reads process state, nothing to assert in a unit test.
    #[mutants::skip]
    #[expect(
        clippy::disallowed_methods,
        reason = "the one place that reads the real process state"
    )]
    pub fn from_process() -> std::io::Result<Self> {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        Ok(Self {
            cwd: std::env::current_dir()?,
            home: env.get("HOME").map(PathBuf::from),
            env,
            ..Self::default()
        })
    }

    fn env_nonempty(&self, var: &str) -> Option<&str> {
        self.env
            .get(var)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

/// Where a layer came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// [`Config::default`].
    Defaults,
    /// A config file.
    File(PathBuf),
    /// A profile file, `profiles/<name>.toml` in the config directory.
    Profile {
        /// The profile name.
        name: String,
        /// The profile file.
        file: PathBuf,
    },
    /// A repository config, `repos/<any>.toml` in the config directory.
    Repo(PathBuf),
    /// `TASQ_*` environment variables.
    Env,
    /// Overrides passed in [`LoadOptions::overrides`].
    Overrides,
}

impl Origin {
    /// The file behind this origin, if it is one.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(p) | Self::Profile { file: p, .. } | Self::Repo(p) => Some(p),
            Self::Defaults | Self::Env | Self::Overrides => None,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Defaults => f.write_str("defaults"),
            Self::File(p) | Self::Repo(p) => write!(f, "{}", p.display()),
            Self::Profile { name, file } => write!(f, "profile {name} ({})", file.display()),
            Self::Env => f.write_str("env"),
            Self::Overrides => f.write_str("--set"),
        }
    }
}

/// One layer that contributed to the effective config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Where it came from.
    pub origin: Origin,
    /// Leaf key paths the layer set (`store.name`, `forge.gitlab.host`,
    /// `source`), sorted. Arrays and empty tables count as leaves.
    pub keys: Vec<String>,
}

/// The result of [`Config::load`]: the config plus where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The effective configuration.
    pub config: Config,
    /// Layers in application order, defaults first.
    pub layers: Vec<Layer>,
    /// The profile that was applied, if any.
    pub profile: Option<String>,
    /// Every profile in the config directory: the stems of
    /// `profiles/*.toml`, sorted.
    pub profiles: Vec<String>,
    /// The repository config that matched [`LoadOptions::repo`], if any.
    pub repo: Option<PathBuf>,
}

impl Loaded {
    /// Checks that every `plugins.enabled` entry can name a `tasq-<name>`
    /// command reached as `tasq <name>` (ADR-0006): not empty, no `/`, no
    /// whitespace, not `.` or `..`, no leading `-`, not one of `reserved` (the CLI's
    /// built-in commands), and listed once. [`Config::load`] checks all but
    /// `reserved`, which only the CLI knows.
    pub fn check_plugin_names(&self, reserved: &[String]) -> Result<(), ConfigError> {
        let enabled = &self.config.plugins.enabled;
        for (i, name) in enabled.iter().enumerate() {
            let reason = if name.is_empty() {
                "a plugin name must not be empty"
            } else if name.contains('/') {
                "a plugin name must not contain '/'"
            } else if name.contains(char::is_whitespace) {
                "a plugin name must not contain whitespace"
            } else if name == "." || name == ".." {
                // Its warnings file would be outside `.tasq/plugins/`.
                "a plugin name must not be '.' or '..'"
            } else if name.starts_with('-') {
                "a plugin name must not start with '-'"
            } else if reserved.contains(name) {
                "a plugin name must not be a built-in command"
            } else if enabled[..i].contains(name) {
                "a plugin name must not be listed twice"
            } else {
                continue;
            };
            return Err(ConfigError::BadPluginName {
                name: name.clone(),
                reason,
                origin: origin_of(self, "plugins.enabled"),
            });
        }
        Ok(())
    }

    /// The layer that decided `key`, a dotted path such as `store.name`.
    ///
    /// Also answers for prefixes (`store`) and for paths below a leaf
    /// (`source.0.name` when a layer set `source`). `None` means no layer,
    /// not even the defaults, set anything there: the key is unset or
    /// unknown.
    pub fn explain(&self, key: &str) -> Option<&Origin> {
        self.layers
            .iter()
            .rev()
            .find(|layer| layer.keys.iter().any(|k| covers(k, key)))
            .map(|layer| &layer.origin)
    }

    /// The file that set `key`, when it came from one.
    pub fn file_for(&self, key: &str) -> Option<&Path> {
        self.explain(key).and_then(Origin::path)
    }
}

/// Whether a recorded leaf `set` answers for the question `asked`: equal,
/// or one is a dotted prefix of the other.
fn covers(set: &str, asked: &str) -> bool {
    set == asked
        || set
            .strip_prefix(asked)
            .is_some_and(|rest| rest.starts_with('.'))
        || asked
            .strip_prefix(set)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// The config directory: the one named by `--config-dir` or
/// [`ENV_CONFIG_DIR`] (`~` expanded, relative to `cwd`), else
/// `$XDG_CONFIG_HOME/tasq`, else `~/.config/tasq`, else `None` when none is
/// known (ADR-0031).
pub fn config_dir(opts: &LoadOptions) -> Option<PathBuf> {
    named_config_dir(opts)
        .map(|(dir, _)| dir)
        .or_else(|| default_config_dir(opts))
}

/// The config directory named by `--config-dir` or [`ENV_CONFIG_DIR`],
/// with `~` expanded and made absolute against `cwd`, and which of the two
/// named it. `None` when neither names one (an empty variable is unset).
pub fn named_config_dir(opts: &LoadOptions) -> Option<(PathBuf, &'static str)> {
    let (dir, named_by) = match &opts.config_dir {
        Some(dir) => (dir.clone(), "--config-dir"),
        None => (
            PathBuf::from(opts.env_nonempty(ENV_CONFIG_DIR)?),
            ENV_CONFIG_DIR,
        ),
    };
    let dir = opts.cwd.join(expand_tilde(&dir, opts.home.as_deref()));
    Some((dir, named_by))
}

/// `$XDG_CONFIG_HOME/tasq`, else `~/.config/tasq`.
fn default_config_dir(opts: &LoadOptions) -> Option<PathBuf> {
    let base = match opts.env_nonempty("XDG_CONFIG_HOME") {
        Some(xdg) => PathBuf::from(xdg),
        None => opts.home.as_ref()?.join(".config"),
    };
    Some(base.join("tasq"))
}

/// The default `data_dir`: `$XDG_DATA_HOME/tasq/stores`, else
/// `~/.local/share/tasq/stores`, else `None` when neither is known
/// (ADR-0026).
pub fn default_data_dir(opts: &LoadOptions) -> Option<PathBuf> {
    let dir = match opts.env_nonempty("XDG_DATA_HOME") {
        Some(xdg) => PathBuf::from(xdg),
        None => opts.home.as_ref()?.join(".local").join("share"),
    };
    Some(dir.join("tasq").join("stores"))
}

/// Shape of one file: a partial [`Config`].
///
/// Mirrors the sections of [`Config`] so that each file is checked for
/// unknown keys and bad values with its own spans.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileSchema {
    data_dir: Option<PathBuf>,
    worktrees_enabled: bool,
    store: super::StoreConfig,
    workflow: super::WorkflowConfig,
    work: super::WorkConfig,
    launch: super::LaunchConfig,
    ui: super::UiConfig,
    forge: BTreeMap<String, super::ForgeConfig>,
    source: Vec<super::SourceConfig>,
    report: super::ReportConfig,
    hooks: super::HooksConfig,
    plugins: super::PluginsConfig,
    plugin: BTreeMap<String, Table>,
    worktree: super::WorktreeConfig,
}

impl Default for FileSchema {
    fn default() -> Self {
        let Config {
            data_dir,
            worktrees_enabled,
            store,
            workflow,
            work,
            launch,
            ui,
            forge,
            source,
            report,
            hooks,
            plugins,
            plugin,
            worktree,
        } = Config::default();
        Self {
            data_dir,
            worktrees_enabled,
            store,
            workflow,
            work,
            launch,
            ui,
            forge,
            source,
            report,
            hooks,
            plugins,
            plugin,
            worktree,
        }
    }
}

/// The `[profile.<name>]` tables of a file, with the span of each name.
/// Anything else, a `profile` that is not a table of tables included, is
/// left to [`FileSchema`].
#[derive(Deserialize)]
struct InlineProfiles {
    profile: BTreeMap<toml::Spanned<String>, Table>,
}

/// The `[[source]]` tables of a file as raw keys with their spans, for
/// [`check_source_keys`]. Only keys present in the file are seen, each
/// with its position. Anything that is not a list of tables is left to
/// [`FileSchema`].
#[derive(Deserialize)]
struct SourceKeys {
    #[serde(default)]
    source: Vec<BTreeMap<toml::Spanned<String>, Value>>,
}

/// The error a `close_when_done` key gets: the switch became one key per
/// reason (ADR 0032).
const CLOSE_WHEN_DONE: &str = "`close_when_done` was replaced by `done_when_closed`, \
     `done_when_merged`, `done_when_approved` (review requests) and `done_when_unassigned` \
     (work items); see docs/sources.md";

/// Checks the keys of each `[[source]]` in `text`, in file order, before
/// [`FileSchema`] sees them: `close_when_done` gets [`CLOSE_WHEN_DONE`]
/// at the key, where `deny_unknown_fields` would only say "unknown field",
/// and a [`super::SourceConfig`] key that the source's kind does not read
/// (`labels` on review requests, `drafts` on work items, `forge` on the
/// LLM bridge) is an error at the key naming the source and the kind. The
/// kind check needs a `name` and a `kind` that parse; without them, and
/// for keys no kind has, [`FileSchema`] reports as it would anyway.
fn check_source_keys(path: &Path, text: &str) -> Result<(), ConfigError> {
    let Ok(SourceKeys { source }) = toml::from_str::<SourceKeys>(text) else {
        return Ok(());
    };
    for keys in source {
        let mut keys: Vec<_> = keys.into_iter().collect();
        keys.sort_by_key(|(key, _)| key.span().start);
        let value_of = |wanted: &str| {
            keys.iter()
                .find(|(key, _)| key.get_ref() == wanted)
                .map(|(_, value)| value)
        };
        let kind = value_of("kind").and_then(|v| v.clone().try_into::<SourceKind>().ok());
        let name = value_of("name").and_then(Value::as_str);
        for (key, _) in &keys {
            let at = key.span().start;
            let key = key.get_ref();
            if key == "close_when_done" {
                return Err(error_at(path, text, at, CLOSE_WHEN_DONE));
            }
            if let (Some(kind), Some(name)) = (kind, name)
                && SourceKind::is_key(key)
                && !kind.uses(key)
            {
                let message = format!(
                    "source {name:?}: `{key}` does not apply to kind {}",
                    kind.as_str()
                );
                return Err(error_at(path, text, at, &message));
            }
        }
    }
    Ok(())
}

/// Shape of a repository config: the main checkout it applies to and the
/// `[worktree]` settings it sets.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoFileSchema {
    path: toml::Spanned<PathBuf>,
    #[serde(default)]
    #[expect(dead_code, reason = "only checked here; the table is merged")]
    worktree: super::WorktreeConfig,
}

/// `worktrees_enabled` in a repository config, with the span of its
/// value: the switch is not a repository setting.
#[derive(Deserialize)]
struct RepoSwitch {
    worktrees_enabled: toml::Spanned<Value>,
}

/// The old `worktree.enabled` in a repository config; see [`RepoSwitch`].
#[derive(Deserialize)]
struct RepoEnabled {
    worktree: RepoEnabledWorktree,
}

/// See [`RepoEnabled`].
#[derive(Deserialize)]
struct RepoEnabledWorktree {
    enabled: toml::Spanned<Value>,
}

/// Splits the value of [`ENV_SET`] into `(key, value)` pairs: one
/// `key=value` per line, blank lines ignored, the key trimmed. A line
/// without `=` or with an empty key is an error naming the line.
pub fn parse_env_set(text: &str) -> Result<Vec<(String, String)>, ConfigError> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match line.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => {
                pairs.push((key.trim().to_owned(), value.to_owned()));
            }
            _ => {
                return Err(ConfigError::InvalidValue {
                    origin: Origin::Env,
                    key: ENV_SET.to_owned(),
                    value: line.to_owned(),
                    expected: "KEY=VALUE".to_owned(),
                });
            }
        }
    }
    Ok(pairs)
}

/// A parsed file: its path and its config table.
struct ParsedFile {
    path: PathBuf,
    table: Table,
}

/// What a config file is, for the error a `[profile.<name>]` table in it
/// gets: the file to move it to in the config directory, unless the file
/// is that profile already.
enum FileKind<'a> {
    /// `config.toml` in this config directory.
    Global(&'a Path),
    /// `profiles/<name>.toml` in this config directory.
    Profile {
        /// The config directory.
        config_dir: &'a Path,
        /// The profile the file is.
        name: &'a str,
    },
}

/// Parses and validates `text` as a config file named `path`, of `kind`.
fn parse_file(path: &Path, text: &str, kind: &FileKind) -> Result<ParsedFile, ConfigError> {
    let to_error = |e: toml::de::Error| parse_error(path, text, &e);
    if let Ok(InlineProfiles { profile }) = toml::from_str(text)
        && let Some(name) = profile.keys().min_by_key(|name| name.span().start)
    {
        let (line, column) = line_col(text, name.span().start);
        let file = path.to_path_buf();
        let name = name.get_ref().clone();
        return Err(match kind {
            FileKind::Profile { name: own, .. } if *own == name => ConfigError::ProfileInProfile {
                file,
                line,
                column,
                name,
            },
            FileKind::Global(config_dir) | FileKind::Profile { config_dir, .. } => {
                ConfigError::InlineProfile {
                    target: config_dir.join(PROFILES_DIR).join(format!("{name}.toml")),
                    file,
                    line,
                    column,
                    name,
                }
            }
        });
    }
    check_source_keys(path, text)?;
    // Typed first: `deny_unknown_fields` and the newtype validators report
    // with spans, which the generic table cannot do.
    toml::from_str::<FileSchema>(text).map_err(to_error)?;
    let table: Table = toml::from_str(text).map_err(to_error)?;
    Ok(ParsedFile {
        path: path.to_path_buf(),
        table,
    })
}

/// A parser error in the file `path` holding `text`, at its line and
/// column.
fn parse_error(path: &Path, text: &str, e: &toml::de::Error) -> ConfigError {
    error_at(path, text, e.span().map_or(0, |s| s.start), e.message())
}

/// A [`ConfigError::Parse`] with `message` at byte `offset` of `text`, the
/// contents of the file `path`.
fn error_at(path: &Path, text: &str, offset: usize, message: &str) -> ConfigError {
    let (line, column) = line_col(text, offset);
    ConfigError::Parse {
        file: path.to_path_buf(),
        line,
        column,
        message: message.to_owned(),
    }
}

/// 1-based line and column of byte `offset` in `text`.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rfind('\n').map_or(offset, |nl| offset - nl - 1) + 1;
    (line, column)
}

/// Merges `layer` into `base`: tables recurse, everything else (including
/// arrays such as `[[source]]`) replaces.
fn deep_merge(base: &mut Table, layer: Table) {
    for (key, value) in layer {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(b)), Value::Table(l)) => deep_merge(b, l),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Leaf key paths of `table`, sorted.
fn leaf_keys(table: &Table) -> Vec<String> {
    fn walk(table: &Table, prefix: &str, out: &mut Vec<String>) {
        for (key, value) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                Value::Table(t) if !t.is_empty() => walk(t, &path, out),
                _ => out.push(path),
            }
        }
    }
    let mut out = Vec::new();
    walk(table, "", &mut out);
    out.sort();
    out
}

/// A table with every optional key present, used to type and recognise
/// `--set`/env keys. Map-valued keys hold one `<name>` wildcard entry.
fn template() -> Table {
    let mut config = Config {
        data_dir: Some(PathBuf::from("~")),
        ..Config::default()
    };
    config.work.default_project = Some(PathBuf::from("~"));
    config.worktree.ports = Some(super::PortsConfig {
        base: 0,
        span: 0,
        block: 0,
    });
    config
        .worktree
        .variables
        .insert("<name>".to_owned(), String::new());
    config.launch.log = Some(PathBuf::from("~"));
    config.launch.claude.prompt_file = Some(PathBuf::from("~"));
    config.report.summary.model = Some(String::new());
    config.report.summary.prompt_file = Some(PathBuf::from("~"));
    config.ui.colors.insert("<name>".to_owned(), String::new());
    config
        .ui
        .theme
        .colors
        .insert("<name>".to_owned(), String::new());
    config
        .ui
        .keys
        .insert("<name>".to_owned(), super::KeySpec::Many(Vec::new()));
    config.forge.insert(
        "<name>".to_owned(),
        super::ForgeConfig {
            kind: Some(ForgeKind::Gitlab),
            url: Some(String::new()),
            host: Some(String::new()),
            token_cmd: Some(String::new()),
        },
    );
    Table::try_from(config).expect("Config serialises to a TOML table")
}

/// Looks `key` up in the template, taking `<name>` entries as wildcards.
fn template_lookup<'a>(mut table: &'a Table, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.').peekable();
    loop {
        let part = parts.next()?;
        let value = table.get(part).or_else(|| table.get("<name>"))?;
        if parts.peek().is_none() {
            return Some(value);
        }
        match value {
            Value::Table(t) => table = t,
            _ => return None,
        }
    }
}

/// Converts the text of an env var or `--set` value to the type `key` has.
/// A `plugin.<name>.<key>` is not in the template: it takes the type the
/// layers merged so far (`below`) gave it, and is a string when none set it.
fn coerce(
    template: &Table,
    below: &Table,
    origin: &Origin,
    key: &str,
    raw: &str,
) -> Result<Value, ConfigError> {
    let plugin_key = key
        .strip_prefix("plugin.")
        .is_some_and(|rest| rest.contains('.'));
    let shape = if plugin_key {
        match template_lookup(below, key) {
            Some(shape) => shape,
            None => return Ok(Value::String(raw.to_owned())),
        }
    } else if key == "worktree.enabled" {
        return Err(ConfigError::WorktreeEnabledMoved {
            origin: origin.clone(),
        });
    } else {
        template_lookup(template, key).ok_or_else(|| ConfigError::UnknownKey {
            origin: origin.clone(),
            key: key.to_owned(),
        })?
    };
    let invalid = |expected: &str| ConfigError::InvalidValue {
        origin: origin.clone(),
        key: key.to_owned(),
        value: raw.to_owned(),
        expected: expected.to_owned(),
    };
    match shape {
        Value::Boolean(_) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(Value::Boolean(true)),
            "0" | "false" | "no" | "off" => Ok(Value::Boolean(false)),
            _ => Err(invalid("true or false")),
        },
        Value::Integer(_) => raw
            .trim()
            .parse()
            .map(Value::Integer)
            .map_err(|_| invalid("an integer")),
        Value::Float(_) => raw
            .trim()
            .parse()
            .map(Value::Float)
            .map_err(|_| invalid("a number")),
        Value::Array(_) => Ok(Value::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_owned()))
                .collect(),
        )),
        Value::Table(_) => Err(invalid("a single value, not a table")),
        _ => Ok(Value::String(raw.to_owned())),
    }
}

/// The environment layer: [`ENV_KEYS`] variables, then [`ENV_SET`]
/// entries (which win for the same key), coerced to the key's type.
fn env_layer(opts: &LoadOptions, template: &Table, below: &Table) -> Result<Table, ConfigError> {
    let mut env_table = Table::new();
    for (var, key) in ENV_KEYS {
        if let Some(raw) = opts.env_nonempty(var) {
            set_leaf(
                &mut env_table,
                key,
                coerce(template, below, &Origin::Env, key, raw)?,
            );
        }
    }
    if let Some(text) = opts.env_nonempty(ENV_SET) {
        for (key, raw) in parse_env_set(text)? {
            set_leaf(
                &mut env_table,
                &key,
                coerce(template, below, &Origin::Env, &key, &raw)?,
            );
        }
    }
    Ok(env_table)
}

/// Sets the leaf `key` in `table`, creating intermediate tables.
fn set_leaf(table: &mut Table, key: &str, value: Value) {
    let mut parts = key.split('.').peekable();
    let mut current = table;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current.insert(part.to_owned(), value);
            return;
        }
        let entry = current
            .entry(part.to_owned())
            .or_insert_with(|| Value::Table(Table::new()));
        if !matches!(entry, Value::Table(_)) {
            *entry = Value::Table(Table::new());
        }
        let Value::Table(next) = entry else {
            unreachable!("just made it a table")
        };
        current = next;
    }
}

/// Reads a file into a string, naming the file on failure.
#[expect(
    clippy::disallowed_methods,
    reason = "config loading reads the filesystem from core; known exception to the no-I/O rule"
)]
fn read_file(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        file: path.to_path_buf(),
        source,
    })
}

/// The config directory, checked, and its global file when there is one.
#[expect(
    clippy::disallowed_methods,
    reason = "config loading reads the filesystem from core; known exception to the no-I/O rule"
)]
fn config_paths(opts: &LoadOptions) -> Result<(Option<PathBuf>, Option<PathBuf>), ConfigError> {
    let dir = match named_config_dir(opts) {
        Some((dir, named_by)) if !dir.is_dir() => {
            return Err(ConfigError::MissingDir { dir, named_by });
        }
        Some((dir, _)) => Some(dir),
        None => default_config_dir(opts),
    };
    let global = dir
        .as_ref()
        .map(|dir| dir.join(GLOBAL_FILE))
        .filter(|p| p.is_file());
    Ok((dir, global))
}

/// A repository config: the file in [`REPOS_DIR`] and the main checkout
/// its `path` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoConfig {
    /// The file, `repos/<any>.toml` in the config directory.
    pub file: PathBuf,
    /// Its `path`, `~` expanded.
    pub path: PathBuf,
}

/// Every repository config in the config directory, in file name order.
/// Fails like [`Config::load`] on a broken file or a duplicate `path`.
pub fn repo_configs(opts: &LoadOptions) -> Result<Vec<RepoConfig>, ConfigError> {
    let (dir, _) = config_paths(opts)?;
    let Some(dir) = dir else {
        return Ok(Vec::new());
    };
    Ok(repo_files(&dir, opts)?
        .into_iter()
        .map(|(main, file)| RepoConfig {
            file: file.path,
            path: main,
        })
        .collect())
}

/// The repository config in `dir`'s [`REPOS_DIR`] whose `path` is `repo`,
/// as its path and its table without `path`. Every file is parsed, so a
/// mistake or a duplicate `path` fails whichever repository is asked about.
fn repo_file(
    dir: &Path,
    repo: &Path,
    opts: &LoadOptions,
) -> Result<Option<ParsedFile>, ConfigError> {
    Ok(repo_files(dir, opts)?
        .into_iter()
        .find_map(|(main, file)| (main == *repo).then_some(file)))
}

/// The `*.toml` files in `dir`, sorted. None when it is absent.
#[expect(
    clippy::disallowed_methods,
    reason = "config loading reads the filesystem from core; known exception to the no-I/O rule"
)]
fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
        .collect();
    paths.sort();
    paths
}

/// Every repository config in `dir`'s [`REPOS_DIR`], parsed, with the main
/// checkout it names, in file name order. Two files with one `path` fail.
fn repo_files(dir: &Path, opts: &LoadOptions) -> Result<Vec<(PathBuf, ParsedFile)>, ConfigError> {
    let paths = toml_files(&dir.join(REPOS_DIR));
    let mut mains: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    let mut files = Vec::new();
    for path in paths {
        let (main, table) = parse_repo_file(&path, &read_file(&path)?, opts.home.as_deref())?;
        if let Some(first) = mains.get(&main) {
            return Err(ConfigError::DuplicateRepo {
                first: first.clone(),
                second: path,
                path: main,
            });
        }
        mains.insert(main.clone(), path.clone());
        files.push((main, ParsedFile { path, table }));
    }
    Ok(files)
}

/// Parses and validates `text` as the repository config `path`: its main
/// checkout (`~` expanded) and its table without `path`.
fn parse_repo_file(
    path: &Path,
    text: &str,
    home: Option<&Path>,
) -> Result<(PathBuf, Table), ConfigError> {
    let switch = toml::from_str::<RepoSwitch>(text)
        .map(|s| s.worktrees_enabled)
        .or_else(|_| toml::from_str::<RepoEnabled>(text).map(|r| r.worktree.enabled));
    if let Ok(value) = switch {
        return Err(error_at(
            path,
            text,
            value.span().start,
            "worktrees_enabled is not a repository setting: \
             set it in config.toml or a profile (docs/config.md, \"Worktrees\")",
        ));
    }
    let schema: RepoFileSchema = toml::from_str(text).map_err(|e| parse_error(path, text, &e))?;
    let main = expand_tilde(schema.path.get_ref(), home);
    if main.is_relative() {
        return Err(error_at(
            path,
            text,
            schema.path.span().start,
            "path must be absolute or start with ~/, the main checkout of a repository",
        ));
    }
    let mut table: Table = toml::from_str(text).map_err(|e| parse_error(path, text, &e))?;
    table.remove("path");
    Ok((main, table))
}

/// The profile names in `dir`'s [`PROFILES_DIR`]: the stems of its
/// `*.toml` files, sorted. None when the directory is absent.
fn profile_names(dir: &Path) -> Vec<String> {
    toml_files(&dir.join(PROFILES_DIR))
        .into_iter()
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_owned))
        .collect()
}

impl Config {
    /// Loads the effective configuration (see the [module docs](super) for
    /// the layer order) and records where every key came from.
    pub fn load(opts: &LoadOptions) -> Result<Loaded, ConfigError> {
        let mut merged = Table::try_from(Self::default()).expect("Config serialises to a table");
        if let Some(dir) = default_data_dir(opts) {
            merged.insert(
                "data_dir".to_owned(),
                Value::String(dir.to_string_lossy().into_owned()),
            );
        }
        let mut layers = vec![Layer {
            origin: Origin::Defaults,
            keys: leaf_keys(&merged),
        }];

        let (dir, global) = config_paths(opts)?;
        // Without a config directory, errors still name `profiles/x.toml`.
        let base = dir.clone().unwrap_or_default();
        if let Some(path) = global {
            let file = parse_file(&path, &read_file(&path)?, &FileKind::Global(&base))?;
            layers.push(Layer {
                origin: Origin::File(file.path),
                keys: leaf_keys(&file.table),
            });
            deep_merge(&mut merged, file.table);
        }

        let profiles = dir.as_deref().map(profile_names).unwrap_or_default();
        let profile = opts
            .profile
            .clone()
            .or_else(|| opts.env_nonempty(ENV_PROFILE).map(str::to_owned));
        if let Some(name) = &profile {
            let path = base.join(PROFILES_DIR).join(format!("{name}.toml"));
            if !profiles.contains(name) {
                return Err(ConfigError::UnknownProfile {
                    name: name.clone(),
                    file: path,
                    available: profiles,
                });
            }
            let kind = FileKind::Profile {
                config_dir: &base,
                name,
            };
            let file = parse_file(&path, &read_file(&path)?, &kind)?;
            layers.push(Layer {
                origin: Origin::Profile {
                    name: name.clone(),
                    file: file.path,
                },
                keys: leaf_keys(&file.table),
            });
            deep_merge(&mut merged, file.table);
        }

        let mut repo = None;
        if let (Some(dir), Some(main)) = (&dir, &opts.repo)
            && let Some(file) = repo_file(dir, main, opts)?
        {
            layers.push(Layer {
                origin: Origin::Repo(file.path.clone()),
                keys: leaf_keys(&file.table),
            });
            deep_merge(&mut merged, file.table);
            repo = Some(file.path);
        }

        let template = template();
        let env_table = env_layer(opts, &template, &merged)?;
        if !env_table.is_empty() {
            layers.push(Layer {
                origin: Origin::Env,
                keys: leaf_keys(&env_table),
            });
            deep_merge(&mut merged, env_table);
        }

        let mut override_table = Table::new();
        for (key, raw) in &opts.overrides {
            set_leaf(
                &mut override_table,
                key,
                coerce(&template, &merged, &Origin::Overrides, key, raw)?,
            );
        }
        if !override_table.is_empty() {
            layers.push(Layer {
                origin: Origin::Overrides,
                keys: leaf_keys(&override_table),
            });
            deep_merge(&mut merged, override_table);
        }

        let config: Self = merged
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Merge {
                message: e.message().to_owned(),
            })?;
        let mut loaded = Loaded {
            config,
            layers,
            profile,
            profiles,
            repo,
        };
        finalize(&mut loaded, opts.home.as_deref())?;
        check_store_name(&loaded)?;
        loaded.check_plugin_names(&[])?;
        Ok(loaded)
    }
}

/// Checks `[worktree]`: user variables are template names that do not
/// shadow a built-in, and every template only names variables its scope
/// has (ADR 0022). Checked whether or not worktrees are enabled, so turning
/// them on never surfaces an old mistake.
fn check_worktree_templates(loaded: &Loaded) -> Result<(), ConfigError> {
    use crate::worktree::hooks::Kind;
    use crate::worktree::vars::{Scope, check, is_builtin};
    let worktree = &loaded.config.worktree;
    for name in worktree.variables.keys() {
        let reason = if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            "a variable name is letters, digits and underscores"
        } else if is_builtin(name) {
            "a built-in variable has this name"
        } else {
            continue;
        };
        return Err(ConfigError::BadWorktreeVariable {
            name: name.clone(),
            reason,
            origin: origin_of(loaded, &format!("worktree.variables.{name}")),
        });
    }
    let ports = worktree.ports.is_some();
    let hooks = Kind::ALL.map(|kind| {
        (
            format!("worktree.hooks.{}", kind.key()),
            Scope::Worktree,
            kind.lines(&worktree.hooks),
        )
    });
    let templates = [
        (
            "worktree.branch".to_owned(),
            Scope::Branch,
            std::slice::from_ref(&worktree.branch),
        ),
        (
            "worktree.path".to_owned(),
            Scope::Path,
            std::slice::from_ref(&worktree.path),
        ),
    ];
    for (key, scope, texts) in templates.into_iter().chain(hooks) {
        for text in texts {
            check(text, scope, ports, &worktree.variables).map_err(|source| {
                ConfigError::UnknownTemplateVariable {
                    origin: origin_of(loaded, &key),
                    key: key.clone(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

/// Checks that `store.name` is one directory name, so the store is a
/// subdirectory of `data_dir` (ADR-0026: a mistyped name makes a harmless
/// empty directory there and nothing else).
fn check_store_name(loaded: &Loaded) -> Result<(), ConfigError> {
    let name = &loaded.config.store.name;
    let reason = if name.is_empty() {
        "a store name must not be empty"
    } else if name == "." || name == ".." {
        "a store name must not be '.' or '..'"
    } else if name.contains('/') {
        "a store name must not contain '/'"
    } else {
        return Ok(());
    };
    Err(ConfigError::BadStoreName {
        name: name.clone(),
        reason,
        origin: origin_of(loaded, "store.name"),
    })
}

/// The layer that set `key`, the defaults when none did (a cross-key rule
/// that fails on a default value blames the defaults).
fn origin_of(loaded: &Loaded, key: &str) -> Origin {
    loaded.explain(key).cloned().unwrap_or(Origin::Defaults)
}

/// Expands `~`, fills in forge defaults and checks cross-key rules.
fn finalize(loaded: &mut Loaded, home: Option<&Path>) -> Result<(), ConfigError> {
    let config = &mut loaded.config;
    if let Some(p) = &config.data_dir {
        config.data_dir = Some(expand_tilde(p, home));
    }
    if let Some(p) = &config.work.default_project {
        config.work.default_project = Some(expand_tilde(p, home));
    }
    if let Some(p) = &config.launch.log {
        config.launch.log = Some(expand_tilde(p, home));
    }
    if let Some(p) = &config.launch.claude.prompt_file {
        config.launch.claude.prompt_file = Some(expand_tilde(p, home));
    }
    if let Some(p) = &config.report.summary.prompt_file {
        config.report.summary.prompt_file = Some(expand_tilde(p, home));
    }
    for source in &mut config.source {
        if let Some(p) = &source.prompt_file {
            source.prompt_file = Some(expand_tilde(p, home));
        }
    }

    if let Some(ports) = config.worktree.ports
        && (ports.block == 0
            || ports.span < ports.block
            || u32::from(ports.base) + u32::from(ports.span) > u32::from(u16::MAX) + 1)
    {
        return Err(ConfigError::BadPorts {
            base: ports.base,
            span: ports.span,
            block: ports.block,
            origin: origin_of(loaded, "worktree.ports.base"),
        });
    }

    let statuses = config.workflow.statuses.clone();
    if !statuses.contains(&config.workflow.default_status) {
        return Err(ConfigError::DefaultStatusNotInWorkflow {
            status: config.workflow.default_status.to_string(),
            statuses: statuses.iter().map(ToString::to_string).collect(),
            origin: origin_of(loaded, "workflow.default_status"),
        });
    }

    let forge_names: Vec<String> = loaded.config.forge.keys().cloned().collect();
    for name in forge_names {
        let origin = origin_of(loaded, &format!("forge.{name}"));
        let forge = loaded
            .config
            .forge
            .get_mut(&name)
            .expect("key from this map");
        let Some(kind) = forge.kind.or_else(|| ForgeKind::from_name(&name)) else {
            return Err(ConfigError::ForgeKindRequired { name, origin });
        };
        forge.kind = Some(kind);
        forge
            .host
            .get_or_insert_with(|| kind.default_host().to_owned());
    }

    let source_origin = origin_of(loaded, "source");
    for source in &loaded.config.source {
        let missing = |field: &'static str| ConfigError::MissingSourceField {
            name: source.name.clone(),
            kind: source.kind.as_str().to_owned(),
            field,
            origin: source_origin.clone(),
        };
        match source.kind.forge_kind() {
            None => {
                if source.command.is_none() {
                    return Err(missing("command"));
                }
            }
            Some(wanted) => {
                let Some(forge_name) = &source.forge else {
                    return Err(missing("forge"));
                };
                let bad = |reason: String| ConfigError::BadForgeReference {
                    name: source.name.clone(),
                    forge: forge_name.clone(),
                    reason,
                    origin: source_origin.clone(),
                };
                let Some(forge) = loaded.config.forge.get(forge_name) else {
                    return Err(bad(format!("no [forge.{forge_name}] block is defined")));
                };
                if forge.kind != Some(wanted) {
                    return Err(bad(format!(
                        "source kind {:?} needs a {} forge, but its kind is {}",
                        source.kind.as_str(),
                        wanted.as_str(),
                        forge.kind.map_or("unset", ForgeKind::as_str),
                    )));
                }
            }
        }
    }
    check_worktree_templates(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No config key is an integer today; the arm exists for the first one
    /// that is, so it is exercised against a hand-built template.
    #[test]
    fn coerce_handles_integers() {
        let mut template = Table::new();
        template.insert("limit".to_owned(), Value::Integer(0));
        let origin = Origin::Overrides;
        assert_eq!(
            coerce(&template, &Table::new(), &origin, "limit", " 42 ").unwrap(),
            Value::Integer(42)
        );
        let err = coerce(&template, &Table::new(), &origin, "limit", "many").unwrap_err();
        assert_eq!(
            err.to_string(),
            "--set: limit=\"many\": expected an integer"
        );
    }
}
