//! What the plugin asks `tasq`: the store (`tasq store info --json`) and
//! its own `[plugin.git]` table (`tasq config show --json`), both as
//! `docs/json.md` describes them.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::process::{self, Env};

/// Why `tasq` could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TasqError {
    /// The command failed; its message.
    #[error("tasq {command} failed: {message}")]
    Failed {
        /// The arguments after `tasq`.
        command: String,
        /// What it printed, trimmed.
        message: String,
    },
    /// The command printed something that is not the documented answer.
    #[error("tasq {command} answered something else: {message}")]
    Unexpected {
        /// The arguments after `tasq`.
        command: String,
        /// Why it was not understood.
        message: String,
    },
}

/// The store tasq resolved for the command that ran the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// `store.name` from the configuration (`default`).
    pub name: String,
    /// The store directory, absolute. It may not exist yet.
    pub location: PathBuf,
}

/// `tasq`, run with the environment the plugin was started with, so that it
/// resolves the same store, profile and overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tasq {
    /// The binary: `TASQ_BIN`, else `tasq` on `PATH`.
    pub bin: PathBuf,
    /// Its environment.
    pub env: Env,
    /// Its working directory.
    pub cwd: PathBuf,
}

#[derive(Deserialize)]
struct StoreInfo {
    store: Location,
    store_name: Value,
}

#[derive(Deserialize)]
struct Location {
    location: PathBuf,
}

#[derive(Deserialize)]
struct Value {
    value: String,
}

#[derive(Deserialize)]
struct ConfigShow {
    config: Config,
}

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    plugin: serde_json::Map<String, serde_json::Value>,
}

impl Tasq {
    /// The store the command resolves.
    pub fn store(&self) -> Result<Store, TasqError> {
        let info: StoreInfo = self.ask(&["store", "info", "--json"])?;
        Ok(Store {
            name: info.store_name.value,
            location: info.store.location,
        })
    }

    /// The `[plugin.git]` table, empty when there is none.
    pub fn plugin_config(&self) -> Result<serde_json::Map<String, serde_json::Value>, TasqError> {
        let show: ConfigShow = self.ask(&["config", "show", "--json"])?;
        Ok(match show.config.plugin.get("git") {
            Some(serde_json::Value::Object(table)) => table.clone(),
            _ => serde_json::Map::new(),
        })
    }

    fn ask<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<T, TasqError> {
        let command = args.join(" ");
        let out = process::run(&self.bin, args, &self.cwd, &self.env);
        if !out.success {
            return Err(TasqError::Failed {
                command,
                message: out.message(),
            });
        }
        serde_json::from_str(&out.stdout).map_err(|err| TasqError::Unexpected {
            command,
            message: err.to_string(),
        })
    }
}

/// The `tasq` to call: `TASQ_BIN` when set (tasq sets it for its plugins),
/// else `tasq` on `PATH`.
pub fn bin(tasq_bin: Option<&Path>) -> PathBuf {
    tasq_bin.map_or_else(|| PathBuf::from("tasq"), Path::to_path_buf)
}
