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
    /// The file named by `--config` or `TASQ_CONFIG` does not exist.
    #[error("config file {file} does not exist (named by {named_by})")]
    MissingFile {
        /// The missing file.
        file: PathBuf,
        /// `--config` or `TASQ_CONFIG`.
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
        /// ``unknown field `notbook`, expected one of `kind`, `notebook`, `bookkeeper` ``.
        message: String,
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
    /// `--set` or an override named a key that does not exist.
    #[error("{origin}: unknown config key {key:?}")]
    UnknownKey {
        /// Where the key came from.
        origin: Origin,
        /// The key path as given.
        key: String,
    },
    /// The selected profile is not defined in any file.
    #[error(
        "profile {name:?} is not defined; available profiles: {}",
        list(available)
    )]
    UnknownProfile {
        /// The requested profile.
        name: String,
        /// Profiles defined across the loaded files.
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
