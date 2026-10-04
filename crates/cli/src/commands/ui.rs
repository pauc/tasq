//! `tasq ui`: the full-screen terminal UI (plan Phase 8).
//!
//! The UI lives in `tasq-tui` and only knows `tasq-core`; this module
//! builds its model from the config, hands it the open store and the
//! clock, and implements the [`Host`]: the three actions that need the
//! outside world run as child processes while the UI has released the
//! terminal. The editor is `$VISUAL`, else `$EDITOR`, else `vi`; sessions
//! and syncs run `tasq pick <id>` and `tasq sync` through this same
//! binary, with the global flags passed on, so the TUI and the CLI cannot
//! disagree about what those commands do.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::model::TaskId;
use tasq_core::theme::Theme;
use tasq_tui::{Host, HostResult, Model};

use crate::app::App;
use crate::cli::GlobalArgs;
use crate::error::{CliError, Result};

/// Runs `ui`.
pub fn run(app: &App) -> Result<()> {
    if app.out.json_mode() {
        return Err(CliError::user("tasq ui has no --json output"));
    }
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(CliError::user("tasq ui needs a terminal"));
    }
    let theme = Theme::from_config(&app.config().ui);
    let model = Model::new(app.workflow(), theme, app.out.color());
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let exe = std::env::current_exe()
        .map_err(|e| CliError::Internal(anyhow::anyhow!("locating the tasq binary: {e}")))?;
    let mut host = CliHost {
        exe,
        global_args: global_args(&app.global),
        editor: editor_command(&app.opts.env),
    };
    tasq_tui::run(model, &mut store, clock.as_ref(), &mut host)?;
    Ok(())
}

/// The [`Host`] of the CLI: child processes on the released terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliHost {
    /// This binary.
    pub exe: PathBuf,
    /// The global flags to pass on to it.
    pub global_args: Vec<String>,
    /// The editor command (program and leading arguments).
    pub editor: Vec<String>,
}

impl Host for CliHost {
    fn edit(&mut self, id: &TaskId, file: &Path) -> HostResult {
        let (program, leading) = self
            .editor
            .split_first()
            .ok_or_else(|| "no editor configured (set $EDITOR)".to_owned())?;
        let mut argv: Vec<String> = leading.to_vec();
        argv.push(file.display().to_string());
        wait_for(Command::new(program).args(&argv), program)
            .map(|()| format!("[{id}] edited with {program}"))
    }

    fn launch(&mut self, id: &TaskId) -> HostResult {
        let mut args = self.global_args.clone();
        args.extend(["pick".to_owned(), id.to_string()]);
        wait_for(Command::new(&self.exe).args(&args), "tasq pick")
            .map(|()| format!("[{id}] session finished"))
    }

    fn sync(&mut self) -> HostResult {
        let mut args = self.global_args.clone();
        args.push("sync".to_owned());
        wait_for(Command::new(&self.exe).args(&args), "tasq sync")
            .map(|()| "sync finished".to_owned())
    }
}

/// Runs the command with the terminal and reports a non-zero exit.
fn wait_for(command: &mut Command, name: &str) -> std::result::Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(match status.code() {
            Some(code) => format!("{name} exited with {code}"),
            None => format!("{name} was killed by a signal"),
        }),
        Err(e) => Err(format!("could not run {name}: {e}")),
    }
}

/// The global flags that select the configuration, as arguments for a
/// child `tasq`: `--profile`, `--config` and every `--set`. Output flags
/// are left to the child, which has the terminal.
pub fn global_args(global: &GlobalArgs) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(profile) = &global.profile {
        args.extend(["--profile".to_owned(), profile.clone()]);
    }
    if let Some(config) = &global.config {
        args.extend(["--config".to_owned(), config.display().to_string()]);
    }
    for (key, value) in &global.set {
        args.extend(["--set".to_owned(), format!("{key}={value}")]);
    }
    args
}

/// `$VISUAL`, else `$EDITOR`, else `vi`, split like a shell would
/// (`EDITOR="code -w"` works); an unsplittable value is used whole.
pub fn editor_command(env: &BTreeMap<String, String>) -> Vec<String> {
    let chosen = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|name| env.get(*name))
        .map(|v| v.trim())
        .find(|v| !v.is_empty())
        .unwrap_or("vi");
    shell_words::split(chosen).unwrap_or_else(|_| vec![chosen.to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_flags_are_passed_on() {
        assert_eq!(global_args(&GlobalArgs::default()), Vec::<String>::new());
        let global = GlobalArgs {
            profile: Some("work".into()),
            config: Some("/etc/tasq.toml".into()),
            set: vec![("store.notebook".into(), "x".into())],
            json: true,
            no_color: true,
            ..GlobalArgs::default()
        };
        assert_eq!(
            global_args(&global),
            [
                "--profile",
                "work",
                "--config",
                "/etc/tasq.toml",
                "--set",
                "store.notebook=x"
            ]
        );
    }

    #[test]
    fn editor_selection() {
        let mut env = BTreeMap::new();
        assert_eq!(editor_command(&env), ["vi"]);
        env.insert("EDITOR".to_owned(), "nvim".to_owned());
        assert_eq!(editor_command(&env), ["nvim"]);
        env.insert("VISUAL".to_owned(), "code -w".to_owned());
        assert_eq!(editor_command(&env), ["code", "-w"]);
        env.insert("VISUAL".to_owned(), "  ".to_owned());
        assert_eq!(editor_command(&env), ["nvim"]);
        env.insert("VISUAL".to_owned(), "it's broken".to_owned());
        assert_eq!(editor_command(&env), ["it's broken"]);
    }

    #[test]
    fn host_reports_missing_programs() {
        let mut host = CliHost {
            exe: "/nonexistent/tasq".into(),
            global_args: vec!["--profile".into(), "x".into()],
            editor: vec!["/nonexistent/editor".into()],
        };
        let err = host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err();
        assert!(
            err.starts_with("could not run /nonexistent/editor: "),
            "{err}"
        );
        let err = host.launch(&TaskId::from(1)).unwrap_err();
        assert!(err.starts_with("could not run tasq pick: "), "{err}");
        let err = host.sync().unwrap_err();
        assert!(err.starts_with("could not run tasq sync: "), "{err}");
        host.editor.clear();
        assert_eq!(
            host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err(),
            "no editor configured (set $EDITOR)"
        );
    }

    #[test]
    fn host_reports_exit_codes() {
        let mut host = CliHost {
            exe: "/bin/sh".into(),
            global_args: vec!["-c".into()],
            editor: vec!["/bin/sh".into(), "-c".into(), "exit 3".into(), "sh".into()],
        };
        let err = host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err();
        assert_eq!(err, "/bin/sh exited with 3");
        // `sh -c pick 1` runs a command called `pick`, which does not exist.
        let err = host.launch(&TaskId::from(1)).unwrap_err();
        assert_eq!(err, "tasq pick exited with 127");
        host.editor = vec!["/bin/sh".into(), "-c".into(), "true".into(), "sh".into()];
        assert_eq!(
            host.edit(&TaskId::from(1), Path::new("/f")).unwrap(),
            "[1] edited with /bin/sh"
        );
    }
}
