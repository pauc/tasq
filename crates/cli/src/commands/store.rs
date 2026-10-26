//! `tasq store info` and `tasq store lock`.

use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use serde_json::Value;
use tasq_core::clock::Clock;
use tasq_core::store::StoreInfo;
use tasq_store::lock::{StoreLock, lock_path};

use crate::app::{App, Provenance};
use crate::cli::StoreCommand;
use crate::error::{CliError, Result};
use crate::json;
use crate::plugins::{ENV_LOCKED, ENV_STORE_DIR, app_passthrough_env};

/// Runs a `store` subcommand.
pub fn run(app: &App, command: StoreCommand) -> Result<()> {
    match command {
        StoreCommand::Info => {
            let store = app.open_store()?;
            let info = store.describe();
            let provenance = app.store_provenance()?;
            let plugins = &app.config().plugins.enabled;
            if app.out.json_mode() {
                return app.out.json(&info_json(&info, &provenance, plugins));
            }
            app.out.print(&render_info(&info, &provenance, plugins))
        }
        StoreCommand::Lock { timeout, command } => lock(app, timeout, &command),
    }
}

/// `tasq store lock`: takes the store lock (waiting at most `timeout`
/// seconds for a live one), runs `argv` holding it and exits with its
/// code. It does not open the store: no plugin is told `store-open` (a
/// plugin's own job runs it) and the store need not exist yet.
fn lock(app: &App, timeout: Option<u64>, argv: &[String]) -> Result<()> {
    let dir = app.store_dir()?;
    let clock = app.clock()?;
    let path = lock_path(&dir);
    let held = StoreLock::acquire_within(&dir, clock.as_ref(), timeout.map(Duration::from_secs))
        .map_err(|e| {
            CliError::user(format!(
                "could not take the store lock {}: {e}",
                path.display()
            ))
        })?;
    let Some(held) = held else {
        return Err(CliError::user(format!(
            "the store lock {} is still held after {}s; {} was not run",
            path.display(),
            timeout.unwrap_or_default(),
            shell_words::join(argv)
        )));
    };
    let mut vars = app.env_vec();
    vars.extend(app_passthrough_env(app));
    vars.push((ENV_STORE_DIR.to_owned(), dir.display().to_string()));
    vars.push((ENV_LOCKED.to_owned(), "1".to_owned()));
    let status = run_locked(argv, &vars, &held, clock.as_ref());
    drop(held);
    exit_with(status?)
}

/// How often [`run_locked`] refreshes the lock: well inside
/// [`tasq_store::lock::STALE_AFTER`], so that a command that runs longer keeps it.
const REFRESH_EVERY: Duration = Duration::from_secs(30);

/// Whether [`run_locked`] refreshes the lock, `since` its last refresh.
fn refresh_due(since: Duration) -> bool {
    since >= REFRESH_EVERY
}

/// Runs `argv` with the terminal's stdin, stdout and stderr and `vars` as
/// its whole environment, and waits for it, refreshing `held` by `clock`
/// whenever [`refresh_due`] meanwhile.
// Reason: spawns and polls a process; the CLI tests cover it.
#[mutants::skip]
fn run_locked(
    argv: &[String],
    vars: &[(String, String)],
    held: &StoreLock,
    clock: &dyn Clock,
) -> Result<ExitStatus> {
    let (program, rest) = argv.split_first().expect("clap requires CMD");
    let mut child = Command::new(program)
        .args(rest)
        .env_clear()
        .envs(vars.iter().map(|(k, v)| (k, v)))
        .spawn()
        .map_err(|e| CliError::user(format!("could not run {program}: {e}")))?;
    let mut refreshed = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| CliError::Internal(anyhow::anyhow!("waiting for {program}: {e}")))?
        {
            return Ok(status);
        }
        if refresh_due(refreshed.elapsed()) {
            // A lock that cannot be touched only risks being taken for
            // stale; the command is already running.
            let _ = held.refresh(clock);
            refreshed = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `Ok` for a command that succeeded, else its exit code: the code it
/// exited with, or 128 plus the signal that ended it, as a shell reports
/// it.
fn exit_with(status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    let code = status
        .code()
        .or_else(|| signal.map(|signal| 128 + signal))
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1);
    Err(CliError::Silent(code))
}

/// The `store info` document: `"store"` holds the info, `"plugins"` the
/// enabled plugin names, `"data_dir"` and `"store_name"` are
/// `{value, from}`.
fn info_json(info: &StoreInfo, provenance: &Provenance, plugins: &[String]) -> Value {
    let setting = |value: Value, from: &str| serde_json::json!({ "value": value, "from": from });
    json::document([
        ("store", json::to_value(info)),
        ("plugins", Value::from(plugins)),
        (
            "data_dir",
            setting(
                Value::from(provenance.data_dir.display().to_string()),
                &provenance.data_dir_from,
            ),
        ),
        (
            "store_name",
            setting(Value::from(provenance.name.as_str()), &provenance.name_from),
        ),
    ])
}

/// The `store info` text; `plugins:` names the enabled plugins, `(none)`
/// when there are none.
pub fn render_info(info: &StoreInfo, provenance: &Provenance, plugins: &[String]) -> String {
    let plugins = if plugins.is_empty() {
        "(none)".to_owned()
    } else {
        plugins.join(", ")
    };
    format!(
        "store:       {}\nlocation:    {}\ndata_dir:    {} (from {})\nstore.name:  {} (from {})\ntasks:       {} (open and done)\nplugins:     {plugins}\n",
        info.name,
        info.location.display(),
        provenance.data_dir.display(),
        provenance.data_dir_from,
        provenance.name,
        provenance.name_from,
        info.task_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> StoreInfo {
        StoreInfo {
            name: "tasq".into(),
            location: "/data/work".into(),
            task_count: 6,
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            data_dir: "/data".into(),
            data_dir_from: "env".into(),
            name: "work".into(),
            name_from: "--set".into(),
        }
    }

    #[test]
    fn the_lock_is_refreshed_well_before_it_goes_stale() {
        assert_eq!(REFRESH_EVERY, Duration::from_secs(30));
        assert!(REFRESH_EVERY * 2 <= tasq_store::lock::STALE_AFTER);
        assert!(!refresh_due(Duration::ZERO));
        assert!(!refresh_due(Duration::from_millis(29_999)));
        assert!(refresh_due(Duration::from_secs(30)), "exactly the interval");
        assert!(refresh_due(Duration::from_secs(31)));
    }

    #[cfg(unix)]
    #[test]
    fn exit_with_passes_the_code_through() {
        use std::os::unix::process::ExitStatusExt;
        let code = |raw| match exit_with(ExitStatus::from_raw(raw)) {
            Ok(()) => 0,
            Err(e) => e.exit_code(),
        };
        assert_eq!(code(0), 0);
        assert_eq!(code(3 << 8), 3);
        assert_eq!(code(255 << 8), 255);
        assert_eq!(code(15), 128 + 15, "SIGTERM");
        assert_eq!(code(9), 128 + 9, "SIGKILL");
    }

    #[test]
    fn info_text() {
        let head = "store:       tasq\nlocation:    /data/work\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\n";
        assert_eq!(
            render_info(&info(), &provenance(), &["git".into(), "echo".into()]),
            format!("{head}plugins:     git, echo\n")
        );
        assert_eq!(
            render_info(&info(), &provenance(), &[]),
            format!("{head}plugins:     (none)\n")
        );
    }

    #[test]
    fn info_json_settings_and_plugins() {
        let value = info_json(&info(), &provenance(), &["git".into()]);
        assert_eq!(value["plugins"], serde_json::json!(["git"]));
        assert_eq!(
            value["data_dir"],
            serde_json::json!({"value": "/data", "from": "env"})
        );
        assert_eq!(
            value["store_name"],
            serde_json::json!({"value": "work", "from": "--set"})
        );
        assert_eq!(
            value["store"],
            serde_json::json!({"name": "tasq", "location": "/data/work", "task_count": 6})
        );
        let value = info_json(&info(), &provenance(), &[]);
        assert_eq!(value["plugins"], serde_json::json!([]));
    }
}
