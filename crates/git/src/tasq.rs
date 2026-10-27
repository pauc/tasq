//! What the plugin asks `tasq`: the store (`tasq store info --json`), its
//! own `[plugin.git]` table (`tasq config show --json`), a task's title
//! (`tasq view <id> --json`) and the duplicate ids (`tasq doctor --json`),
//! as `docs/json.md` describes them; and it runs a command under the store
//! lock (`tasq store lock -- <cmd>`).

use std::ffi::{OsStr, OsString};
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
struct View {
    task: Title,
}

#[derive(Deserialize)]
struct Title {
    title: String,
}

#[derive(Deserialize)]
struct Doctor {
    checks: Vec<Check>,
}

/// One check of `tasq doctor --json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Check {
    /// Its name: `ids` for the store's duplicate ids.
    pub name: String,
    /// `ok`, `warn` or `fail`.
    pub status: String,
    /// What it found.
    pub detail: String,
    /// What to do about it.
    #[serde(default)]
    pub fix: Option<String>,
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
    /// The `tasq` the job asks about the store at `store`: `bin` with
    /// `env`, run in `cwd`, the directory the job started in, or in the
    /// store when that one is gone (a `tasq done` that removed the
    /// worktree it ran in). The store is pinned (`TASQ_DATA_DIR`,
    /// `TASQ_STORE_NAME`), so a `.tasq.toml` of either directory cannot
    /// point it at another store.
    pub fn for_store(bin: PathBuf, env: Env, cwd: Option<PathBuf>, store: &Path) -> Self {
        let mut env: Env = env
            .into_iter()
            .filter(|(key, _)| key != "TASQ_DATA_DIR" && key != "TASQ_STORE_NAME")
            .collect();
        if let (Some(data_dir), Some(name)) = (store.parent(), store.file_name()) {
            env.push(("TASQ_DATA_DIR".into(), data_dir.into()));
            env.push(("TASQ_STORE_NAME".into(), name.into()));
        }
        Self {
            bin,
            env,
            cwd: cwd.unwrap_or_else(|| store.to_owned()),
        }
    }

    /// The store the command resolves.
    pub fn store(&self) -> Result<Store, TasqError> {
        self.store_info(&["store", "info", "--json"])
    }

    /// The store `name` (`tasq --set store.name=<name>`), or the one the
    /// command resolves when `None`.
    pub fn store_named(&self, name: Option<&str>) -> Result<Store, TasqError> {
        let Some(name) = name else {
            return self.store();
        };
        let set = format!("store.name={name}");
        self.store_info(&["--set", &set, "store", "info", "--json"])
    }

    fn store_info(&self, args: &[&str]) -> Result<Store, TasqError> {
        let info: StoreInfo = self.ask(args)?;
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

    /// The title of task `id`.
    pub fn title(&self, id: &str) -> Result<String, TasqError> {
        let view: View = self.ask(&["view", id, "--json"])?;
        Ok(view.task.title)
    }

    /// The store's `ids` failures (an id claimed by more than one file)
    /// from `tasq doctor --json`; `None` when it gives no answer. Doctor
    /// exits 1 whenever a check fails, so only its output counts.
    pub fn id_failures(&self) -> Option<Vec<Check>> {
        let out = process::run(&self.bin, &["doctor", "--json"], &self.cwd, &self.env);
        let doctor: Doctor = serde_json::from_str(&out.stdout).ok()?;
        Some(
            doctor
                .checks
                .into_iter()
                .filter(|c| c.name == "ids" && c.status == "fail")
                .collect(),
        )
    }

    /// Runs `cmd` under the store lock (`tasq store lock -- <cmd>`); the
    /// error is what `cmd`, or tasq, printed.
    pub fn locked(&self, cmd: &[&OsStr]) -> Result<(), String> {
        let mut args: Vec<OsString> = ["store", "lock", "--"].map(OsString::from).to_vec();
        args.extend(cmd.iter().map(OsString::from));
        let out = process::run(&self.bin, &args, &self.cwd, &self.env);
        if out.success {
            Ok(())
        } else {
            Err(out.message())
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jobs_tasq_is_pinned_to_its_store() {
        let env: Env = [
            ("HOME", "/home/me"),
            ("TASQ_STORE_NAME", "work"),
            ("TASQ_DATA_DIR", "/elsewhere"),
            ("TASQ_SET", "ui.pager=less"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .to_vec();
        let store = Path::new("/data/stores/default");
        let pinned = [
            ("HOME", "/home/me"),
            ("TASQ_SET", "ui.pager=less"),
            ("TASQ_DATA_DIR", "/data/stores"),
            ("TASQ_STORE_NAME", "default"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .to_vec();
        let here = Tasq::for_store("tasq".into(), env.clone(), Some("/proj".into()), store);
        assert_eq!(
            here,
            Tasq {
                bin: "tasq".into(),
                env: pinned.clone(),
                cwd: "/proj".into(),
            }
        );
        let gone = Tasq::for_store("tasq".into(), env, None, store);
        assert_eq!(gone.cwd, store);
        assert_eq!(gone.env, pinned);
    }
}
