//! Out-of-process plugins (ADR-0006, option A).
//!
//! Three pieces, all built on the `--json` surface rather than on linking:
//!
//! - **Dispatch.** `tasq <name> [args...]`, where `<name>` is not a built-in
//!   command, runs an executable called `tasq-<name>` found on `PATH` with
//!   the remaining arguments verbatim ([`External::parse`], [`find`],
//!   [`exec`]). Built-in commands and their aliases always win.
//! - **Hooks.** `[hooks]` command lines run around CLI events
//!   ([`Hook`], [`run_hooks`]): after `create`, after `done`, before a
//!   session is launched. Each gets a JSON document on stdin.
//! - **Discovery.** `tasq plugins list` ([`discover`]).
//! - **Enabled plugins.** `[plugins] enabled` names the plugins told about
//!   events (ADR 0030); one that is not on `PATH` is a warning on every
//!   command that opens the store and a `doctor` failure ([`enabled`]).
//! - **Events.** Each installed enabled plugin runs as `tasq-<name> hook`
//!   with the event payload on stdin ([`fire`]); a failure or a timeout is
//!   a warning.
//!
//! Plugins and hooks call back into `tasq` through [`ENV_BIN`] and see the
//! same configuration because [`passthrough_env`] forwards `--profile`,
//! `--config` and `--set` as `TASQ_PROFILE`, `TASQ_CONFIG` and `TASQ_SET`.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use clap::CommandFactory;
use serde::Serialize;
use serde_json::Value;
use tasq_core::config::{ENV_CONFIG, ENV_PROFILE, ENV_SET, Loaded, expand_tilde};
use tasq_core::doctor::Check;
use tasq_core::launch::ENV_TASK_ID;
use tasq_core::model::Task;
use tasq_core::plugin::{Event, Payload, StoreRef};

use crate::app::App;
use crate::cli::Cli;
use crate::error::{CliError, Result};
use crate::json;

/// Environment variable holding the path of the running `tasq` binary, so
/// plugins and hooks call back into the same executable.
pub const ENV_BIN: &str = "TASQ_BIN";
/// Environment variable naming the hook being run (`post-create`, ...).
pub const ENV_HOOK: &str = "TASQ_HOOK";
/// Environment variable naming the event a plugin is told about
/// (`store-open`, ...).
pub const ENV_EVENT: &str = "TASQ_EVENT";
/// Environment variable holding the store directory, for the command
/// `tasq store lock` runs.
pub const ENV_STORE_DIR: &str = "TASQ_STORE_DIR";
/// Environment variable set to `1` for the command `tasq store lock` runs.
pub const ENV_LOCKED: &str = "TASQ_LOCKED";
/// The argument a plugin is run with for an event: `tasq-<name> hook`.
pub const HOOK_ARG: &str = "hook";
/// Prefix of plugin executables.
pub const PREFIX: &str = "tasq-";

/// Global flags that take a value, so that `--profile work tlogs` skips
/// `work` when looking for the command name.
const VALUE_FLAGS: &[&str] = &["--profile", "--config", "--set", "--color"];
/// Global flags without a value.
const BOOL_FLAGS: &[&str] = &["--json", "--no-color", "--no-pager", "--verbose"];

/// A command line that names a non-built-in command: what to run and the
/// global flags given before it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct External {
    /// The command name (`tlogs` for `tasq tlogs`).
    pub name: String,
    /// Everything after the name, verbatim.
    pub args: Vec<OsString>,
    /// `--profile` given before the name.
    pub profile: Option<String>,
    /// `--config` given before the name.
    pub config: Option<PathBuf>,
    /// `--set KEY=VALUE` flags given before the name, in order.
    pub set: Vec<String>,
}

impl External {
    /// Reads `argv` (program name first) up to its first positional
    /// argument. Returns `None` when there is none, when it is a built-in
    /// command, or when an unknown flag precedes it (clap then reports it).
    pub fn parse(argv: &[OsString]) -> Option<Self> {
        let builtin = builtin_names();
        let mut external = Self::default();
        let mut rest = argv.iter().skip(1);
        let mut only_positionals = false;
        while let Some(arg) = rest.next() {
            let text = arg.to_string_lossy();
            if only_positionals || !text.starts_with('-') || text == "-" {
                if builtin.iter().any(|b| *b == text) {
                    return None;
                }
                external.name = text.into_owned();
                external.args = rest.cloned().collect();
                return Some(external);
            }
            if text == "--" {
                only_positionals = true;
                continue;
            }
            let (flag, inline) = match text.split_once('=') {
                Some((f, v)) => (f.to_owned(), Some(v.to_owned())),
                None => (text.clone().into_owned(), None),
            };
            if VALUE_FLAGS.contains(&flag.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => rest.next()?.to_string_lossy().into_owned(),
                };
                match flag.as_str() {
                    "--profile" => external.profile = Some(value),
                    "--config" => external.config = Some(PathBuf::from(value)),
                    "--set" => external.set.push(value),
                    _ => {}
                }
            } else if BOOL_FLAGS.contains(&flag.as_str()) && inline.is_none()
                || is_verbose_short(&text)
            {
                // Not forwarded: output flags belong to tasq itself.
            } else {
                return None;
            }
        }
        None
    }
}

/// `-v`, `-vv`, ...
fn is_verbose_short(text: &str) -> bool {
    text.len() >= 2 && text.starts_with('-') && text[1..].chars().all(|c| c == 'v')
}

/// The names of every built-in command (plus `help`).
fn builtin_names() -> Vec<String> {
    let command = Cli::command();
    let mut names: Vec<String> = command
        .get_subcommands()
        .flat_map(|c| {
            std::iter::once(c.get_name().to_owned()).chain(c.get_all_aliases().map(str::to_owned))
        })
        .collect();
    names.push("help".to_owned());
    names
}

/// A plugin executable found on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plugin {
    /// The command name (`tlogs` for `tasq-tlogs`).
    pub name: String,
    /// The executable.
    pub path: PathBuf,
}

/// The `tasq-<name>` executable in the first `PATH` entry holding one.
pub fn find(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let file = format!("{PREFIX}{name}");
    std::env::split_paths(path?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(&file))
        .find(|candidate| is_executable(candidate))
}

/// Every `tasq-*` executable on `PATH`, sorted by name; for a name found
/// in several entries the first one wins, as it does when run.
pub fn discover(path: Option<&OsStr>) -> Vec<Plugin> {
    let mut found: Vec<Plugin> = Vec::new();
    let Some(path) = path else {
        return found;
    };
    for dir in std::env::split_paths(path).filter(|d| !d.as_os_str().is_empty()) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name
                .to_str()
                .and_then(|f| f.strip_prefix(PREFIX))
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            if found.iter().any(|p| p.name == name) || !is_executable(&entry.path()) {
                continue;
            }
            found.push(Plugin {
                name: name.to_owned(),
                path: entry.path(),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// An enabled plugin (`[plugins] enabled`) and its executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Enabled {
    /// The plugin name (`git` for `tasq-git`).
    pub name: String,
    /// `tasq-<name>` as found on `PATH`; `None` when it is not installed.
    pub path: Option<PathBuf>,
}

impl Enabled {
    /// The warning for a plugin that is not installed, `None` when it is.
    pub fn missing(&self) -> Option<String> {
        self.path.is_none().then(|| not_on_path(&self.name))
    }

    /// The plugin's `doctor` check, named after its executable: `Ok` with
    /// the path, `Fail` when it is not installed.
    pub fn check(&self) -> Check {
        let executable = format!("{PREFIX}{}", self.name);
        match &self.path {
            Some(path) => Check::ok(
                &executable,
                format!("plugin {} is {}", self.name, path.display()),
            ),
            None => Check::fail(
                &executable,
                not_on_path(&self.name),
                format!("install {executable} or remove it from [plugins] enabled"),
            ),
        }
    }
}

/// What an enabled plugin missing from `PATH` is reported as.
fn not_on_path(name: &str) -> String {
    format!("plugin {name} is enabled but {PREFIX}{name} is not on PATH")
}

/// The enabled plugins, in `names` order, each with its executable on
/// `path`.
pub fn enabled(names: &[String], path: Option<&OsStr>) -> Vec<Enabled> {
    names
        .iter()
        .map(|name| Enabled {
            name: name.clone(),
            path: find(name, path),
        })
        .collect()
}

/// Checks the `plugins.enabled` names, built-in command names included
/// (see [`Loaded::check_plugin_names`]).
pub fn check_names(loaded: &Loaded) -> Result<()> {
    if loaded.config.plugins.enabled.is_empty() {
        return Ok(());
    }
    Ok(loaded.check_plugin_names(&builtin_names())?)
}

/// A regular file the current user may execute.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The variables a plugin or hook needs to see the same configuration as
/// the `tasq` that ran it: [`ENV_BIN`], and [`ENV_PROFILE`],
/// [`ENV_CONFIG`] and [`ENV_SET`] when a profile, a config file or
/// overrides are in effect (`set` is appended to any `TASQ_SET` already
/// in `env`, one entry per line).
pub fn passthrough_env(
    bin: Option<&Path>,
    profile: Option<&str>,
    config: Option<&Path>,
    set: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Some(bin) = bin {
        vars.push((ENV_BIN.to_owned(), bin.display().to_string()));
    }
    if let Some(profile) = profile {
        vars.push((ENV_PROFILE.to_owned(), profile.to_owned()));
    }
    if let Some(config) = config {
        let absolute = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf());
        vars.push((ENV_CONFIG.to_owned(), absolute.display().to_string()));
    }
    let mut lines: Vec<String> = env(ENV_SET)
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    lines.extend(set.iter().cloned());
    if !lines.is_empty() {
        vars.push((ENV_SET.to_owned(), lines.join("\n")));
    }
    vars
}

/// The running binary, when the OS can tell.
pub fn current_bin() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Replaces the process with the plugin at `path`, with the inherited
/// environment plus [`passthrough_env`]. Returns only when that fails.
///
/// Not unit-tested: a test observes the plugin, not this function.
pub fn exec(path: &Path, external: &External) -> CliError {
    let mut command = Command::new(path);
    command.args(&external.args);
    let vars = passthrough_env(
        current_bin().as_deref(),
        external.profile.as_deref(),
        external.config.as_deref(),
        &external.set,
        &|name| std::env::var(name).ok(),
    );
    for (k, v) in vars {
        command.env(k, v);
    }
    let error = {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.exec()
        }
        #[cfg(not(unix))]
        {
            match command.status() {
                Ok(status) => std::process::exit(status.code().unwrap_or(1)),
                Err(e) => e,
            }
        }
    };
    CliError::user(format!(
        "could not run plugin {} ({}): {error}",
        external.name,
        path.display()
    ))
}

/// The CLI events hooks can attach to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    /// `tasq create` wrote a task.
    PostCreate,
    /// `tasq done` closed a task.
    PostDone,
    /// `tasq next`/`pick` is about to start a session.
    PreLaunch,
}

impl Hook {
    /// The `[hooks]` key and the `hook` field of the document.
    pub fn name(self) -> &'static str {
        match self {
            Self::PostCreate => "post-create",
            Self::PostDone => "post-done",
            Self::PreLaunch => "pre-launch",
        }
    }

    /// Whether a failing command aborts the operation (`pre-launch`) or is
    /// only reported (the `post-*` hooks, whose event already happened).
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::PreLaunch)
    }

    /// The configured command lines for this hook.
    pub fn commands(self, app: &App) -> &[String] {
        let hooks = &app.config().hooks;
        match self {
            Self::PostCreate => &hooks.post_create,
            Self::PostDone => &hooks.post_done,
            Self::PreLaunch => &hooks.pre_launch,
        }
    }
}

/// `{"schema": 1, "hook": <name>, "task": <task>, <extra>...}`; the task
/// has no `worktrees` when they are disabled (see [`json::task`]).
pub fn hook_document(hook: Hook, task: &Task, worktrees: bool, extra: &[(&str, Value)]) -> Value {
    let mut fields = vec![
        ("hook", Value::from(hook.name())),
        ("task", json::task(task, worktrees)),
    ];
    fields.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
    json::document(fields)
}

/// What a hook command had to say while [`run_hooks_with`] ran it.
pub use tasq_core::worktree::hooks::HookEvent;

/// Runs every command configured for `hook` with `document` on stdin,
/// printing each command's output at `-v` and each failure as a warning.
/// A command that fails (or cannot start) is a user error for a blocking
/// hook and a warning otherwise; the remaining commands still run in the
/// non-blocking case.
pub fn run_hooks(app: &App, hook: Hook, task: &Task, extra: &[(&str, Value)]) -> Result<()> {
    run_hooks_with(app, hook, task, extra, &mut |event| match event {
        HookEvent::Output(text) => app.out.verbose(&text),
        HookEvent::Warning(text) => app.out.warn(&text),
    })
}

/// [`run_hooks`] with the reporting left to `report`, called in the order
/// things happen. The TUI uses it: inside the alternate screen a warning
/// printed on stderr would scribble over the UI, so it collects them for
/// the status bar instead.
pub fn run_hooks_with(
    app: &App,
    hook: Hook,
    task: &Task,
    extra: &[(&str, Value)],
    report: &mut dyn FnMut(HookEvent),
) -> Result<()> {
    let commands = hook.commands(app);
    if commands.is_empty() {
        return Ok(());
    }
    let document = serde_json::to_string(&hook_document(hook, task, app.worktrees(), extra))
        .map_err(|e| CliError::Internal(anyhow::anyhow!("serialising hook document: {e}")))?;
    let mut vars = app_passthrough_env(app);
    vars.push((ENV_HOOK.to_owned(), hook.name().to_owned()));
    vars.push((ENV_TASK_ID.to_owned(), task.id.to_string()));
    for line in commands {
        let argv = hook_argv(line, app.opts.home.as_deref())?;
        match run_command(&argv, &document, &vars, None) {
            Ok(output) => {
                if !output.trim().is_empty() {
                    report(HookEvent::Output(format!(
                        "{} hook {line:?}: {}",
                        hook.name(),
                        output.trim()
                    )));
                }
            }
            Err(message) if hook.is_blocking() => {
                return Err(CliError::user(format!(
                    "{} hook {line:?} failed: {message}",
                    hook.name()
                )));
            }
            Err(message) => report(HookEvent::Warning(format!(
                "{} hook {line:?} failed: {message}",
                hook.name()
            ))),
        }
    }
    Ok(())
}

/// [`passthrough_env`] for `app`'s flags and environment.
pub fn app_passthrough_env(app: &App) -> Vec<(String, String)> {
    passthrough_env(
        current_bin().as_deref(),
        app.loaded.profile.as_deref(),
        app.global.config.as_deref(),
        &app.global
            .set
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>(),
        &|name| app.opts.env.get(name).cloned(),
    )
}

/// Tells every installed enabled plugin in `plugins` about `event`, one
/// at a time in that order: `tasq-<name> hook` with the payload on stdin,
/// `app`'s environment (and nothing else) plus [`passthrough_env`] and
/// [`ENV_EVENT`], cut off with its process group once `plugins.timeout`
/// seconds have passed and its output is still open. What a plugin prints
/// is `report`ed as output; a plugin that fails, cannot start or times out
/// is a warning naming it and the event, never an error. A plugin missing
/// from `PATH` is skipped (its warning is [`Enabled::missing`]'s).
///
/// A `tasq` run while [`ENV_EVENT`] is set (by a plugin, during an event)
/// tells no plugin: one that reads the store through `$TASQ_BIN` would
/// otherwise wake itself without end.
pub fn fire(
    app: &App,
    plugins: &[Enabled],
    event: Event,
    report: &mut dyn FnMut(HookEvent),
) -> Result<()> {
    if app.opts.env.get(ENV_EVENT).is_some_and(|v| !v.is_empty()) {
        return Ok(());
    }
    let installed: Vec<(&str, &Path)> = plugins
        .iter()
        .filter_map(|p| Some((p.name.as_str(), p.path.as_deref()?)))
        .collect();
    if installed.is_empty() {
        return Ok(());
    }
    let location = app.store_dir()?;
    let store = StoreRef {
        name: &app.config().store.name,
        location: &location,
    };
    let mut vars = app.env_vec();
    vars.extend(app_passthrough_env(app));
    vars.push((ENV_EVENT.to_owned(), event.name().to_owned()));
    let timeout = Duration::from_secs(app.config().plugins.timeout);
    for (name, path) in installed {
        let payload = Payload::new(event, store, app.config().plugin.get(name));
        let outcome = serde_json::to_string(&payload)
            .map_err(|e| format!("could not write the payload: {e}"))
            .and_then(|document| {
                let argv = [path.display().to_string(), HOOK_ARG.to_owned()];
                run_command(&argv, &document, &vars, Some(timeout))
            });
        match outcome {
            Ok(output) if output.trim().is_empty() => {}
            Ok(output) => report(HookEvent::Output(format!(
                "plugin {name}: {}: {}",
                event.name(),
                output.trim()
            ))),
            Err(message) => report(HookEvent::Warning(format!(
                "plugin {name}: {} event failed: {message}",
                event.name()
            ))),
        }
    }
    Ok(())
}

/// Splits a hook command line into argv (no shell) and expands `~` in the
/// program.
pub fn hook_argv(line: &str, home: Option<&Path>) -> Result<Vec<String>> {
    let mut argv = shell_words::split(line)
        .map_err(|e| CliError::user(format!("hook command {line:?}: {e}")))?;
    match argv.first_mut() {
        None => Err(CliError::user("hook command must not be empty")),
        Some(program) => {
            *program = expand_tilde(Path::new(program), home).display().to_string();
            Ok(argv)
        }
    }
}

/// Runs `argv` with `input` on stdin. Without a `timeout` (hooks), the
/// child inherits the environment plus `vars` and is waited for. With one
/// (plugin events), its environment is `vars` alone, it leads its own
/// process group, and the group is killed if the child is still running,
/// or its output still open, once `timeout` has passed. `Ok` carries
/// stdout; `Err` carries stderr (else stdout) trimmed, the OS error when
/// the program could not start, or the timeout.
///
/// Not unit-tested: nothing to assert without a process.
fn run_command(
    argv: &[String],
    input: &str,
    vars: &[(String, String)],
    timeout: Option<Duration>,
) -> std::result::Result<String, String> {
    let (program, rest) = argv.split_first().ok_or("empty command")?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if timeout.is_some() {
        command.env_clear();
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
    }
    for (k, v) in vars {
        command.env(k, v);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    // Threads, so that a child that neither reads its stdin nor drains its
    // output cannot block us past the timeout.
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_owned();
        // The child may exit without reading; that is its business.
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    // A timeout too large for the clock is no deadline.
    let deadline = timeout.and_then(|limit| Instant::now().checked_add(limit));
    let timed_out = |child: &mut Child| {
        kill_group(child);
        let _ = child.wait();
        // The readers are left behind: something outside the group may
        // still hold the pipes open.
        format!("timed out after {}s", timeout.unwrap_or_default().as_secs())
    };
    let status = match deadline {
        None => child.wait().map_err(|e| e.to_string())?,
        Some(deadline) => {
            let Some(status) = wait_until(&mut child, deadline).map_err(|e| e.to_string())? else {
                return Err(timed_out(&mut child));
            };
            status
        }
    };
    // A background job that inherited the pipes keeps them open after the
    // child exits; the event is not over until they close.
    let (Some(stdout), Some(stderr)) = (receive(&stdout, deadline), receive(&stderr, deadline))
    else {
        return Err(timed_out(&mut child));
    };
    if status.success() {
        return Ok(stdout);
    }
    let message = if stderr.trim().is_empty() {
        stdout.trim().to_owned()
    } else {
        stderr.trim().to_owned()
    };
    Err(match status.code() {
        Some(code) if message.is_empty() => format!("exit status {code}"),
        Some(code) => format!("exit status {code}: {message}"),
        None if message.is_empty() => "killed by a signal".to_owned(),
        None => format!("killed by a signal: {message}"),
    })
}

/// Reads `pipe` to the end on a thread; the text arrives on the channel.
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> Receiver<String> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = send.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    receive
}

/// What [`drain`] read, waiting until `deadline` at most; `None` when the
/// pipe is still open then.
fn receive(text: &Receiver<String>, deadline: Option<Instant>) -> Option<String> {
    match deadline {
        None => Some(text.recv().unwrap_or_default()),
        Some(deadline) => {
            match text.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(text) => Some(text),
                Err(RecvTimeoutError::Disconnected) => Some(String::new()),
                Err(RecvTimeoutError::Timeout) => None,
            }
        }
    }
}

/// Waits for `child` to exit until `deadline`; `None` when it is still
/// running then.
// Reason: polls a live process; a mutant that stops waiting hangs the tests.
#[mutants::skip]
fn wait_until(child: &mut Child, deadline: Instant) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Kills `child` and the rest of the process group it leads.
// Reason: wraps kill(2); a mutant that kills nothing hangs the tests.
#[mutants::skip]
fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    {
        use rustix::process::{Pid, Signal, kill_process_group};
        let _ = kill_process_group(Pid::from_child(child), Signal::KILL);
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        std::iter::once("tasq")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn external_is_the_first_positional_that_is_not_a_builtin() {
        let ext = External::parse(&argv(&["tlogs", "last", "week", "--json"])).unwrap();
        assert_eq!(ext.name, "tlogs");
        assert_eq!(ext.args, argv(&["last", "week", "--json"])[1..].to_vec());
        assert_eq!(ext.profile, None);
        for builtin in ["list", "create", "store", "plugins", "help", "ui"] {
            assert_eq!(External::parse(&argv(&[builtin, "x"])), None, "{builtin}");
        }
        assert_eq!(External::parse(&argv(&[])), None);
        assert_eq!(External::parse(&argv(&["--json"])), None);
        // Any other word is a candidate, a status included.
        assert_eq!(External::parse(&argv(&["ready"])).unwrap().name, "ready");
        assert_eq!(External::parse(&argv(&["ls", "x"])), None);
    }

    #[test]
    fn global_flags_before_the_name_are_collected() {
        let ext = External::parse(&argv(&[
            "--profile",
            "work",
            "--config=/c.toml",
            "--set",
            "a=1",
            "--set=b=2",
            "--json",
            "-vv",
            "--color",
            "never",
            "--no-pager",
            "tlogs",
            "--profile",
            "not-ours",
        ]))
        .unwrap();
        assert_eq!(ext.name, "tlogs");
        assert_eq!(ext.profile.as_deref(), Some("work"));
        assert_eq!(ext.config, Some(PathBuf::from("/c.toml")));
        assert_eq!(ext.set, vec!["a=1".to_owned(), "b=2".to_owned()]);
        assert_eq!(
            ext.args,
            vec![OsString::from("--profile"), OsString::from("not-ours")]
        );
        // A value flag at the end has no name after it.
        assert_eq!(External::parse(&argv(&["--profile"])), None);
        // Unknown flags are clap's to report.
        assert_eq!(External::parse(&argv(&["--nope", "tlogs"])), None);
        assert_eq!(External::parse(&argv(&["-h", "tlogs"])), None);
        assert_eq!(External::parse(&argv(&["--json=yes", "tlogs"])), None);
        // `--` ends the flags.
        let ext = External::parse(&argv(&["--", "tlogs", "x"])).unwrap();
        assert_eq!((ext.name.as_str(), ext.args.len()), ("tlogs", 1));
        assert_eq!(External::parse(&argv(&["--", "list"])), None);
    }

    #[cfg(unix)]
    fn executable(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn find_and_discover_follow_path_order_and_skip_non_executables() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let a_tlogs = executable(a.path(), "tasq-tlogs");
        executable(b.path(), "tasq-tlogs");
        let b_zed = executable(b.path(), "tasq-zed");
        std::fs::write(a.path().join("tasq-plain"), "not executable").unwrap();
        std::fs::write(a.path().join("tasq-"), "").unwrap();
        std::fs::create_dir(a.path().join("tasq-dir")).unwrap();
        executable(a.path(), "unrelated");
        let path =
            std::env::join_paths([Path::new("/nonexistent"), Path::new(""), a.path(), b.path()])
                .unwrap();

        assert_eq!(find("tlogs", Some(&path)), Some(a_tlogs.clone()));
        assert_eq!(find("zed", Some(&path)), Some(b_zed.clone()));
        assert_eq!(find("plain", Some(&path)), None);
        assert_eq!(find("tlogs", None), None);
        assert_eq!(
            discover(Some(&path)),
            vec![
                Plugin {
                    name: "tlogs".to_owned(),
                    path: a_tlogs
                },
                Plugin {
                    name: "zed".to_owned(),
                    path: b_zed
                },
            ]
        );
        assert_eq!(discover(None), Vec::new());
    }

    #[test]
    fn passthrough_env_forwards_what_is_in_effect() {
        let none = |_: &str| None;
        assert_eq!(passthrough_env(None, None, None, &[], &none), Vec::new());
        let vars = passthrough_env(
            Some(Path::new("/usr/bin/tasq")),
            Some("work"),
            Some(Path::new("/nonexistent/c.toml")),
            &["a=1".to_owned(), "b=2".to_owned()],
            &|name| (name == ENV_SET).then(|| "z=0\n".to_owned()),
        );
        assert_eq!(
            vars,
            vec![
                (ENV_BIN.to_owned(), "/usr/bin/tasq".to_owned()),
                (ENV_PROFILE.to_owned(), "work".to_owned()),
                (ENV_CONFIG.to_owned(), "/nonexistent/c.toml".to_owned()),
                (ENV_SET.to_owned(), "z=0\na=1\nb=2".to_owned()),
            ]
        );
        // An empty TASQ_SET in the environment is not forwarded.
        let vars = passthrough_env(None, None, None, &[], &|_| Some("  ".to_owned()));
        assert_eq!(vars, Vec::new());
    }

    #[test]
    fn hook_names_and_documents() {
        assert_eq!(Hook::PostCreate.name(), "post-create");
        assert_eq!(Hook::PostDone.name(), "post-done");
        assert_eq!(Hook::PreLaunch.name(), "pre-launch");
        assert!(Hook::PreLaunch.is_blocking());
        assert!(!Hook::PostCreate.is_blocking() && !Hook::PostDone.is_blocking());
        let task = Task::new(tasq_core::model::TaskId::from(7), "Seven");
        let doc = hook_document(
            Hook::PreLaunch,
            &task,
            true,
            &[("workdir", Value::from("/w"))],
        );
        assert_eq!(doc["schema"], 1);
        assert_eq!(doc["hook"], "pre-launch");
        assert_eq!(doc["task"]["id"], "7");
        assert_eq!(doc["task"]["worktrees"], Value::Array(Vec::new()));
        assert_eq!(doc["workdir"], "/w");
        let off = hook_document(Hook::PostDone, &task, false, &[]);
        assert_eq!(off["task"].get("worktrees"), None);
    }

    #[test]
    fn hook_argv_splits_and_expands_the_program() {
        let home = Path::new("/home/me");
        assert_eq!(
            hook_argv("~/bin/notify --quiet 'two words'", Some(home)).unwrap(),
            vec![
                "/home/me/bin/notify".to_owned(),
                "--quiet".to_owned(),
                "two words".to_owned()
            ]
        );
        assert_eq!(
            hook_argv("tasq-notify", None).unwrap(),
            vec!["tasq-notify".to_owned()]
        );
        assert_eq!(
            hook_argv("", None).unwrap_err().to_string(),
            "hook command must not be empty"
        );
        assert!(
            hook_argv("unterminated 'quote", None)
                .unwrap_err()
                .to_string()
                .starts_with("hook command \"unterminated 'quote\":")
        );
    }
}
