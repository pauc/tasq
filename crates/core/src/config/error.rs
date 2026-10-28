//! Configuration errors.

use std::path::PathBuf;

use thiserror::Error;

use super::Origin;

/// Why a configuration could not be loaded.
///
/// Messages name the file (or `env` / `--set`), the key and what was
/// expected, so they can be shown to the user as they are.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The directory named by `--config-dir` or `TASQ_CONFIG_DIR` does not
    /// exist (or is not a directory).
    #[error("config directory {dir} does not exist or is not a directory (named by {named_by})")]
    MissingDir {
        /// The missing directory, `~` expanded and absolute.
        dir: PathBuf,
        /// `--config-dir` or `TASQ_CONFIG_DIR`.
        named_by: &'static str,
    },
    /// A file exists but could not be read.
    #[error("cannot read config file {file}: {source}")]
    Read {
        /// The unreadable file.
        file: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A file is not valid TOML or not a valid config: unknown key, wrong
    /// type, misspelt status, unknown enum value. `message` comes from the
    /// parser and names what was expected.
    #[error("{file}:{line}:{column}: {message}")]
    Parse {
        /// The offending file.
        file: PathBuf,
        /// 1-based line.
        line: usize,
        /// 1-based column.
        column: usize,
        /// Parser message, for example
        /// ``unknown field `nmae`, expected one of `name`, ... ``.
        message: String,
    },
    /// A file holds a `[profile.<name>]` table: a profile is a file of
    /// its own now (ADR-0031).
    #[error(
        "{file}:{line}:{column}: profiles are files now: move [profile.{name}] to {}",
        target.display()
    )]
    InlineProfile {
        /// The file holding the table.
        file: PathBuf,
        /// 1-based line of the profile name.
        line: usize,
        /// 1-based column of the profile name.
        column: usize,
        /// The profile name.
        name: String,
        /// The profile file to move it to, `profiles/<name>.toml` in the
        /// config directory.
        target: PathBuf,
    },
    /// The profile file `profiles/<name>.toml` holds a `[profile.<name>]`
    /// table: the file is the profile, so its tables go without the prefix
    /// (ADR-0031).
    #[error(
        "{file}:{line}:{column}: a profile file is the profile itself: remove the \
         [profile.{name}] header and the profile.{name}. prefix of its tables"
    )]
    ProfileInProfile {
        /// The profile file.
        file: PathBuf,
        /// 1-based line of the profile name.
        line: usize,
        /// 1-based column of the profile name.
        column: usize,
        /// The profile name in the table header.
        name: String,
    },
    /// Two repository configs name the same main checkout (ADR-0031).
    #[error(
        "repository configs {} and {} both have path {}",
        first.display(),
        second.display(),
        path.display()
    )]
    DuplicateRepo {
        /// The first file, in name order.
        first: PathBuf,
        /// The second file.
        second: PathBuf,
        /// The main checkout both name, `~` expanded.
        path: PathBuf,
    },
    /// A value set by the environment or `--set` could not be used.
    #[error("{origin}: {key}={value:?}: expected {expected}")]
    InvalidValue {
        /// Where the value came from.
        origin: Origin,
        /// Config key path, such as `ui.no_osc8`.
        key: String,
        /// The rejected text.
        value: String,
        /// What would have been accepted.
        expected: String,
    },
    /// `--set` or [`super::ENV_SET`] named `worktree.enabled`, which moved
    /// to the top-level `worktrees_enabled` (ADR 0031).
    #[error(
        "{origin}: worktree.enabled moved to the top level: \
         set worktrees_enabled (docs/config.md, \"Worktrees\")"
    )]
    WorktreeEnabledMoved {
        /// Where the key came from.
        origin: Origin,
    },
    /// `--set` or an override named a key that does not exist.
    #[error("{origin}: unknown config key {key:?}")]
    UnknownKey {
        /// Where the key came from.
        origin: Origin,
        /// The key path as given.
        key: String,
    },
    /// The selected profile has no file in the config directory's
    /// `profiles/`.
    #[error(
        "profile {name:?} is not defined: no file {}; available profiles: {}",
        file.display(),
        if available.is_empty() { "none".to_owned() } else { list(available) }
    )]
    UnknownProfile {
        /// The requested profile.
        name: String,
        /// The file it would be, `profiles/<name>.toml` in the config
        /// directory.
        file: PathBuf,
        /// The profiles that have a file.
        available: Vec<String>,
    },
    /// The merged layers do not form a valid config. Each file is valid on
    /// its own, so this points at a conflict between layers (for example a
    /// profile that turns a table into a string).
    #[error("invalid configuration after merging layers: {message}")]
    Merge {
        /// Parser message, which names the key.
        message: String,
    },
    /// `workflow.default_status` is not one of `workflow.statuses`.
    #[error(
        "workflow.default_status = {status:?} (set by {origin}) is not in workflow.statuses [{}]",
        list(statuses)
    )]
    DefaultStatusNotInWorkflow {
        /// The default status.
        status: String,
        /// The statuses it is missing from.
        statuses: Vec<String>,
        /// Which layer set `workflow.default_status`.
        origin: Origin,
    },
    /// A worktree template names a variable it does not have.
    #[error("{key} (set by {origin}): {source}")]
    UnknownTemplateVariable {
        /// The key holding the template (`worktree.path`, ...).
        key: String,
        /// The name and what was available.
        source: crate::worktree::vars::UnknownVariable,
        /// Where the key was set.
        origin: Origin,
    },
    /// `store.name` is not one directory name: empty, `.`, `..` or with a
    /// `/` in it, so the store would not sit under `data_dir`.
    #[error("store.name = {name:?} (set by {origin}): {reason}")]
    BadStoreName {
        /// The name as written.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
        /// Where it was set.
        origin: Origin,
    },
    /// A `plugins.enabled` entry that cannot be a `tasq-<name>` command
    /// name: empty or with a `/` in it (ADR-0006).
    #[error("plugins.enabled: {name:?} (set by {origin}): {reason}")]
    BadPluginName {
        /// The name as written.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
        /// Where `plugins.enabled` was set.
        origin: Origin,
    },
    /// A `[worktree.variables]` name that cannot be used in a template.
    #[error("worktree.variables.{name} (set by {origin}): {reason}")]
    BadWorktreeVariable {
        /// The name as written.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
        /// Where it was set.
        origin: Origin,
    },
    /// `[worktree.ports]` that cannot hold one block, or runs past 65535.
    #[error(
        "worktree.ports (set by {origin}): base = {base}, span = {span}, block = {block}: block must be at least 1, span at least block, and base + span at most 65536"
    )]
    BadPorts {
        /// `worktree.ports.base`.
        base: u16,
        /// `worktree.ports.span`.
        span: u16,
        /// `worktree.ports.block`.
        block: u16,
        /// Where the table was set.
        origin: Origin,
    },
    /// A `[[source]]` lacks a key its `kind` needs.
    #[error("source {name:?} of kind {kind:?} requires `{field}` (set by {origin})")]
    MissingSourceField {
        /// `source.name`.
        name: String,
        /// `source.kind`.
        kind: String,
        /// The missing key.
        field: &'static str,
        /// Which layer set `source`.
        origin: Origin,
    },
    /// A `[[source]]` references a `[forge.<name>]` that does not exist or
    /// speaks the wrong API.
    #[error("source {name:?} references forge {forge:?} (set by {origin}): {reason}")]
    BadForgeReference {
        /// `source.name`.
        name: String,
        /// `source.forge`.
        forge: String,
        /// Why it is unusable.
        reason: String,
        /// Which layer set `source`.
        origin: Origin,
    },
    /// A `[forge.<name>]` block has no `kind` and the name is neither
    /// `gitlab` nor `github`.
    #[error("forge.{name}.kind is required (set by {origin}): expected \"gitlab\" or \"github\"")]
    ForgeKindRequired {
        /// The forge block name.
        name: String,
        /// Which layer set the block.
        origin: Origin,
    },
}

fn list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
