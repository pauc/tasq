//! `tasq ui`: the full-screen terminal UI (plan Phase 8).
//!
//! The UI lives in `tasq-tui` and only knows `tasq-core`; this module
//! builds its model from the config, hands it the open store and the
//! clock, and implements the [`Host`]: the three actions that need the
//! outside world run as child processes while the UI has released the
//! terminal, and the `post-done` and `post-create` hooks run in-process
//! after the `d` and `c` keys, their warnings handed back for the status
//! bar (ADR-0010, ADR-0011). The editor is
//! `$VISUAL`, else `$EDITOR`, else `vi`; sessions and syncs run
//! `tasq pick <id>` and `tasq sync` through this same binary, with the
//! global flags passed on, so the TUI and the CLI cannot disagree about
//! what those commands do. A session in a new window (`Ctrl+Enter`,
//! `Shift+Enter`) is `tasq pick <id> --window [--no-focus]` with its
//! output captured, so the UI keeps the screen and shows the last line
//! (ADR-0012).

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

use tasq_core::config::Config;
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::theme::Theme;
use tasq_tui::{Host, HostResult, KeyMap, LaunchTarget, Model, SourceChoice};

use crate::app::App;
use crate::cli::GlobalArgs;
use crate::commands::create::{Asked, new_task_worktree};
use crate::commands::worktree::{self, Releasing};
use crate::commands::worktree_hooks::Approval;
use crate::error::{CliError, Result};
use crate::plugins::{self, Hook, HookEvent};

/// The UI draws on stdout and reads keys from stdin: both must be terminals.
fn interactive(stdout: bool, stdin: bool) -> bool {
    stdout && stdin
}

/// The UI's model for `app`'s configuration, with `keys` and `today`.
fn model(app: &App, keys: KeyMap, today: chrono::NaiveDate) -> Model {
    let ui = &app.config().ui;
    Model::new(app.workflow(), Theme::from_config(ui), app.out.color())
        .with_keys(keys)
        .with_today(today)
        .with_week_start(ui.week_start.weekday())
        .with_due_format(ui.due_format)
        .with_detail_position(ui.detail_position)
        .with_worktrees(app.worktrees())
        .with_home(app.opts.home.clone())
        .with_osc8(!ui.no_osc8)
        .with_view_pause(view_pager(&ui.pager).is_none())
        .with_default_status(app.config().workflow.default_status.clone())
        .with_default_project(Some(
            crate::commands::existing_dir(&app.opts.cwd).unwrap_or_else(|| app.opts.cwd.clone()),
        ))
        .with_sources(source_choices(app.config()))
}

/// The pager `v` runs `tasq view` with when `pager` is less: the same
/// command with `-+F -+X` last, so less stays open on a short task and
/// uses the alternate screen (cleared on `q`) whatever `pager` and `$LESS`
/// say, read before the command line. `None` for any other pager, which
/// is left alone (the UI then waits for a key after it).
fn view_pager(pager: &str) -> Option<String> {
    let argv = shell_words::split(pager).ok()?;
    let program = Path::new(argv.first()?).file_name()?;
    (program == "less").then(|| format!("{pager} -+F -+X"))
}

/// Runs `ui`.
pub fn run(app: &App) -> Result<()> {
    if app.out.json_mode() {
        return Err(CliError::user("tasq ui has no --json output"));
    }
    let keys = KeyMap::from_config(&app.config().ui.keys).map_err(|e| {
        match app.loaded.file_for(&e.config_key()) {
            Some(file) => CliError::user(format!("{}: {e}", file.display())),
            None => CliError::user(e.to_string()),
        }
    })?;
    if !interactive(
        std::io::stdout().is_terminal(),
        std::io::stdin().is_terminal(),
    ) {
        return Err(CliError::user("tasq ui needs a terminal"));
    }
    let clock = app.clock()?;
    let model = model(app, keys, clock.today());
    let mut store = app.open_store()?;
    let exe = std::env::current_exe()
        .map_err(|e| CliError::Internal(anyhow::anyhow!("locating the tasq binary: {e}")))?;
    let mut host = CliHost {
        app,
        exe,
        global_args: global_args(&app.global),
        editor: crate::editor::editor_command(&app.opts.env),
    };
    tasq_tui::run(model, &mut store, clock.as_ref(), &mut host)?;
    Ok(())
}

/// The enabled `[[source]]` blocks as the entries of the TUI's source
/// picker (`S`): name, kind and whether a bare `tasq sync` runs them.
pub fn source_choices(config: &Config) -> Vec<SourceChoice> {
    config
        .source
        .iter()
        .filter(|s| s.enabled)
        .map(|s| SourceChoice {
            name: s.name.clone(),
            kind: s.kind.as_str().to_owned(),
            auto: s.auto,
        })
        .collect()
}

/// The [`Host`] of the CLI: child processes on the released terminal, and
/// the hooks of `app` after a close or a create.
#[derive(Debug)]
pub struct CliHost<'a> {
    /// The configuration the hooks come from.
    pub app: &'a App,
    /// This binary.
    pub exe: PathBuf,
    /// The global flags to pass on to it.
    pub global_args: Vec<String>,
    /// The editor command (program and leading arguments).
    pub editor: Vec<String>,
}

impl Host for CliHost<'_> {
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

    fn launch(&mut self, id: &TaskId, target: LaunchTarget) -> HostResult {
        let mut args = self.global_args.clone();
        args.extend(["pick".to_owned(), id.to_string()]);
        match target {
            LaunchTarget::Here => wait_for(Command::new(&self.exe).args(&args), "tasq pick")
                .map(|()| format!("[{id}] session finished")),
            LaunchTarget::Window { focus } => {
                args.push("--window".to_owned());
                if !focus {
                    args.push("--no-focus".to_owned());
                }
                capture(Command::new(&self.exe).args(&args), "tasq pick")
            }
        }
    }

    fn view(&mut self, id: &TaskId) -> HostResult {
        let mut args = self.global_args.clone();
        if let Some(pager) = view_pager(&self.app.config().ui.pager) {
            args.extend(["--set".to_owned(), format!("ui.pager={pager}")]);
        }
        args.extend(["view".to_owned(), id.to_string()]);
        wait_for(Command::new(&self.exe).args(&args), "tasq view")
            .map(|()| format!("[{id}] viewed"))
    }

    fn sync(&mut self, sources: &[String]) -> HostResult {
        let mut args = self.global_args.clone();
        args.push("sync".to_owned());
        for name in sources {
            args.extend(["--source".to_owned(), name.clone()]);
        }
        wait_for(Command::new(&self.exe).args(&args), "tasq sync")
            .map(|()| "sync finished".to_owned())
    }

    /// The same `post-done` hooks as `tasq done`, with the same document,
    /// then the same release of its worktrees, never asking about hooks
    /// (the UI owns the terminal): unapproved ones are skipped.
    fn after_done(&mut self, task: &Task) -> std::result::Result<(), String> {
        let hooks = self.hooks(Hook::PostDone, task);
        if !self.app.worktrees() {
            return hooks;
        }
        let mut warnings: Vec<String> = hooks.err().into_iter().collect();
        let mut collect = |event| {
            if let HookEvent::Warning(text) = event {
                warnings.push(text);
            }
        };
        let how = Releasing::closing();
        if let Err(e) = worktree::release(self.app, task, how, Approval::Skip, &mut collect) {
            warnings.push(format!("worktrees not released: {e}"));
        }
        if warnings.is_empty() {
            Ok(())
        } else {
            Err(warnings.join("; "))
        }
    }

    /// The worktree `tasq create` would declare: on `branch` when one was
    /// typed, else as `worktree.on_create` decides.
    fn worktree(
        &mut self,
        task: &Task,
        branch: Option<&str>,
    ) -> std::result::Result<Option<Worktree>, String> {
        let asked = branch.map_or(Asked::OnCreate, Asked::Branch);
        new_task_worktree(self.app, task, asked).map_err(|e| format!("no worktree: {e}"))
    }

    /// The same `post-create` hooks as `tasq create`, with the same document.
    fn after_create(&mut self, task: &Task) -> std::result::Result<(), String> {
        self.hooks(Hook::PostCreate, task)
    }
}

impl CliHost<'_> {
    /// Runs the `hook` command lines on `task`; what the CLI would print
    /// as warnings comes back as the `Err`, one per failed command, joined
    /// with `; `. A hook's stdout (`-v` material on the CLI) has nowhere
    /// to go in the UI and is dropped.
    fn hooks(&self, hook: Hook, task: &Task) -> std::result::Result<(), String> {
        let mut warnings = Vec::new();
        plugins::run_hooks_with(self.app, hook, task, &[], &mut |event| {
            if let HookEvent::Warning(text) = event {
                warnings.push(text);
            }
        })
        .map_err(|e| e.to_string())?;
        if warnings.is_empty() {
            Ok(())
        } else {
            Err(warnings.join("; "))
        }
    }
}

/// Runs the command without the terminal and reports its last line:
/// stdout's on success (the launcher's "Opened ..." outcome), stderr's
/// (the CLI's error) on a non-zero exit.
fn capture(command: &mut Command, name: &str) -> std::result::Result<String, String> {
    let output = command
        .output()
        .map_err(|e| format!("could not run {name}: {e}"))?;
    let last_line = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_owned)
    };
    if output.status.success() {
        Ok(last_line(&output.stdout).unwrap_or_else(|| format!("{name} finished")))
    } else {
        let exit = match output.status.code() {
            Some(code) => format!("{name} exited with {code}"),
            None => format!("{name} was killed by a signal"),
        };
        Err(last_line(&output.stderr).map_or(exit, |line| {
            line.trim_start_matches("tasq: error: ").to_owned()
        }))
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

#[cfg(test)]
mod tests {
    use tasq_core::config::{Config, LoadOptions};

    use super::*;

    #[test]
    fn the_ui_needs_both_stdout_and_stdin_on_a_terminal() {
        assert!(interactive(true, true));
        assert!(!interactive(true, false));
        assert!(!interactive(false, true));
        assert!(!interactive(false, false));
    }
    use crate::output::Output;

    /// An app whose only configuration is `.tasq.toml` in `dir` with `text`.
    fn app_in(dir: &Path, text: &str) -> App {
        std::fs::write(dir.join(".tasq.toml"), text).unwrap();
        let mut opts = LoadOptions::new(dir);
        opts.home = Some(dir.to_path_buf());
        let loaded = Config::load(&opts).unwrap();
        let global = GlobalArgs::default();
        let out = Output::new(&global, &loaded.config.ui, &opts.env, false);
        App::new(global, opts, loaded, out)
    }

    #[test]
    fn the_model_follows_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let m = model(&app_in(dir.path(), ""), KeyMap::default(), today);
        assert!(m.osc8, "links are on by default");
        assert!(!m.view_pauses, "less is kept open, q returns");
        assert_eq!(m.home.as_deref(), Some(dir.path()));
        assert_eq!(m.today, today);
        let app = app_in(dir.path(), "[ui]\nno_osc8 = true\npager = \"bat -p\"\n");
        let m = model(&app, KeyMap::default(), today);
        assert!(!m.osc8);
        assert!(m.view_pauses, "another pager may quit at once");
    }

    #[test]
    fn view_keeps_less_open_on_the_alternate_screen() {
        assert_eq!(
            view_pager("less -RFX").as_deref(),
            Some("less -RFX -+F -+X")
        );
        assert_eq!(view_pager("less").as_deref(), Some("less -+F -+X"));
        assert_eq!(
            view_pager("/usr/bin/less -R").as_deref(),
            Some("/usr/bin/less -R -+F -+X")
        );
        for other in [
            "bat -p --paging=always",
            "lesspipe",
            "cat",
            "",
            "'unbalanced",
        ] {
            assert_eq!(view_pager(other), None, "{other}");
        }
    }

    fn host(app: &App) -> CliHost<'_> {
        CliHost {
            app,
            exe: "/nonexistent/tasq".into(),
            global_args: vec!["--profile".into(), "x".into()],
            editor: vec!["/nonexistent/editor".into()],
        }
    }

    #[test]
    fn source_choices_are_the_enabled_sources_with_their_auto_flag() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(
            dir.path(),
            "[[source]]\nname = \"a\"\nkind = \"llm-bridge\"\ncommand = \"a\"\n\
             [[source]]\nname = \"b\"\nkind = \"gitlab-work-items\"\nforge = \"gl\"\nauto = false\n\
             [[source]]\nname = \"c\"\nkind = \"llm-bridge\"\ncommand = \"c\"\nenabled = false\n\
             [forge.gl]\nkind = \"gitlab\"\nhost = \"gl.test\"\n",
        );
        let choices = source_choices(app.config());
        assert_eq!(
            choices,
            vec![
                SourceChoice {
                    name: "a".into(),
                    kind: "llm-bridge".into(),
                    auto: true
                },
                SourceChoice {
                    name: "b".into(),
                    kind: "gitlab-work-items".into(),
                    auto: false
                },
            ],
            "disabled sources are not offered"
        );
        assert_eq!(source_choices(app_in(dir.path(), "").config()), Vec::new());
    }

    #[test]
    fn sync_passes_the_picked_sources_as_source_flags() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(dir.path(), "");
        let log = dir.path().join("args.log");
        let exe = dir.path().join("tasq");
        std::fs::write(
            &exe,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut host = host(&app);
        host.exe = exe;
        assert_eq!(host.sync(&[]).unwrap(), "sync finished");
        assert_eq!(
            host.sync(&["gitlab".to_owned(), "inbox".to_owned()])
                .unwrap(),
            "sync finished"
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "--profile x sync\n--profile x sync --source gitlab --source inbox\n"
        );
    }

    /// A repository at `<dir>/proj` with worktree `b` at `<dir>/proj-b`,
    /// an app with worktrees on (`git` from this process's `PATH`), and
    /// task 8 closed with that worktree declared.
    fn released(dir: &Path) -> (App, Task, PathBuf) {
        let project = dir.join("proj");
        let wt = dir.join("proj-b");
        std::fs::create_dir(&project).unwrap();
        let git = |cwd: &Path, args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        git(&project, &["init", "-q", "-b", "main"]);
        git(&project, &["commit", "-q", "--allow-empty", "-m", "init"]);
        git(
            &project,
            &["worktree", "add", "-q", "-b", "b", wt.to_str().unwrap()],
        );
        let mut app = app_in(dir, "[worktree]\nenabled = true\n");
        for key in ["PATH", "HOME"] {
            if let Ok(value) = std::env::var(key) {
                app.opts.env.insert(key.to_owned(), value);
            }
        }
        let mut task = closed_task();
        task.project = Some(project);
        task.add_worktree(Worktree::on_branch(&wt, "b"));
        (app, task, wt)
    }

    #[test]
    fn after_done_releases_the_worktrees_without_asking() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let (app, task, wt) = released(&dir);
        std::fs::write(wt.join("scratch"), "x").unwrap();
        assert_eq!(
            host(&app).after_done(&task),
            Err(format!(
                "kept worktree {}: it has uncommitted or untracked changes; \
                 tasq worktree release 8 --force removes it",
                wt.display()
            ))
        );
        std::fs::remove_file(wt.join("scratch")).unwrap();
        // Hooks of the repository's own file are never asked about here.
        std::fs::write(
            dir.join("proj").join(".git").join("info").join("exclude"),
            ".tasq.toml\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("proj").join(".tasq.toml"),
            "[worktree.hooks]\npre-remove = [\"/bin/false\"]\n",
        )
        .unwrap();
        let warning = host(&app).after_done(&task).unwrap_err();
        assert!(
            warning.starts_with("hooks from ") && warning.contains("were skipped"),
            "{warning}"
        );
        assert!(!wt.exists(), "removed");
        assert_eq!(host(&app).after_done(&task), Ok(()), "nothing left");
        let off = app_in(&dir, "");
        assert_eq!(host(&off).after_done(&task), Ok(()), "disabled: untouched");
    }

    fn closed_task() -> Task {
        let mut task = Task::new(TaskId::from(8), "Hooked");
        task.done = true;
        task
    }

    #[test]
    fn after_done_without_hooks_runs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(dir.path(), "");
        assert_eq!(host(&app).after_done(&closed_task()), Ok(()));
    }

    #[test]
    fn after_done_runs_the_post_done_hooks_and_returns_the_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("hooks.log");
        let config = r#"[hooks]
post-create = ["/nonexistent/other-hook"]
post-done = [
    "/bin/sh -c 'echo \"$TASQ_HOOK $TASQ_TASK_ID\" > @LOG@; cat >> @LOG@'",
    "/bin/sh -c 'echo nope >&2; exit 4'",
    "/bin/sh -c 'echo chatter'",
    "/nonexistent/hook --flag",
]
"#
        .replace("@LOG@", &log.display().to_string());
        let app = app_in(dir.path(), &config);
        let err = host(&app).after_done(&closed_task()).unwrap_err();
        // Both failures, in order, the successful ones silent.
        assert!(
            err.starts_with(concat!(
                "post-done hook \"/bin/sh -c 'echo nope >&2; exit 4'\" failed: exit status 4: nope; ",
                "post-done hook \"/nonexistent/hook --flag\" failed: could not run /nonexistent/hook: "
            )),
            "{err}"
        );
        // The document and the environment are the ones `tasq done` sends.
        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(
            logged.starts_with("post-done 8\n{\"hook\":\"post-done\",\"schema\":1,\"task\":{"),
            "{logged}"
        );
        assert!(logged.contains("\"done\":true"), "{logged}");
        assert!(logged.contains("\"title\":\"Hooked\""), "{logged}");

        // Underneath, the runner reports in order: a silent success is no
        // event, stdout is `Output`, a failure is `Warning`.
        let mut events = Vec::new();
        plugins::run_hooks_with(&app, Hook::PostDone, &closed_task(), &[], &mut |e| {
            events.push(e);
        })
        .unwrap();
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(matches!(&events[0], HookEvent::Warning(w) if w.ends_with("exit status 4: nope")));
        assert_eq!(
            events[1],
            HookEvent::Output("post-done hook \"/bin/sh -c 'echo chatter'\": chatter".into())
        );
        assert!(matches!(&events[2], HookEvent::Warning(w) if w.contains("/nonexistent/hook")));
    }

    #[test]
    fn worktree_declares_as_tasq_create_does() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("app");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let plain = root.join("plain");
        std::fs::create_dir(&plain).unwrap();
        let app = app_in(&root, "[worktree]\nenabled = true\n");
        let mut task = Task::new(TaskId::from(9), "Fresh start");
        task.project = Some(repo.clone());
        let at = |name: &str| root.join(name);
        assert_eq!(
            host(&app).worktree(&task, None),
            Ok(Some(Worktree::on_branch(
                at("app-9_fresh_start"),
                "9-fresh-start"
            )))
        );
        assert_eq!(
            host(&app).worktree(&task, Some("feat/x")),
            Ok(Some(Worktree::on_branch(at("app-feat_x"), "feat/x")))
        );
        task.project = Some(plain);
        assert_eq!(host(&app).worktree(&task, None), Ok(None), "not a checkout");
        assert_eq!(
            host(&app).worktree(&task, Some("b")),
            Ok(Some(Worktree::on_branch(at("plain-b"), "b"))),
            "a typed branch always declares"
        );
        task.project = None;
        assert_eq!(host(&app).worktree(&task, None), Ok(None));
        assert_eq!(
            host(&app).worktree(&task, Some("b")),
            Err(
                "no worktree: task 9 tracks no project and work.default_project is unset; \
                 set one with tasq project 9 <path>"
                    .into()
            )
        );
        let off = app_in(&root, "");
        task.project = Some(repo);
        assert_eq!(host(&off).worktree(&task, Some("b")), Ok(None), "disabled");
    }

    #[test]
    fn after_create_runs_the_post_create_hooks_only() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("hooks.log");
        let config = r#"[hooks]
post-done = ["/nonexistent/other-hook"]
post-create = [
    "/bin/sh -c 'echo \"$TASQ_HOOK $TASQ_TASK_ID\" > @LOG@; cat >> @LOG@'",
    "/bin/sh -c 'echo nope >&2; exit 4'",
]
"#
        .replace("@LOG@", &log.display().to_string());
        let app = app_in(dir.path(), &config);
        let task = Task::new(TaskId::from(9), "Fresh");
        let err = host(&app).after_create(&task).unwrap_err();
        assert_eq!(
            err,
            "post-create hook \"/bin/sh -c 'echo nope >&2; exit 4'\" failed: exit status 4: nope"
        );
        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(
            logged.starts_with("post-create 9\n{\"hook\":\"post-create\",\"schema\":1,\"task\":{"),
            "{logged}"
        );
        assert!(logged.contains("\"title\":\"Fresh\""), "{logged}");
        // Without hooks there is nothing to report.
        let app = app_in(dir.path(), "");
        assert_eq!(host(&app).after_create(&task), Ok(()));
    }

    #[test]
    fn after_done_reports_a_bad_hook_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(
            dir.path(),
            "[hooks]\npost-done = [\"unterminated 'quote\"]\n",
        );
        let err = host(&app).after_done(&closed_task()).unwrap_err();
        assert!(
            err.starts_with("hook command \"unterminated 'quote\": "),
            "{err}"
        );
    }

    #[test]
    fn config_flags_are_passed_on() {
        assert_eq!(global_args(&GlobalArgs::default()), Vec::<String>::new());
        let global = GlobalArgs {
            profile: Some("work".into()),
            config: Some("/etc/tasq.toml".into()),
            set: vec![("store.name".into(), "x".into())],
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
                "store.name=x"
            ]
        );
    }

    #[test]
    fn host_reports_missing_programs() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(dir.path(), "");
        let mut host = host(&app);
        let err = host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err();
        assert!(
            err.starts_with("could not run /nonexistent/editor: "),
            "{err}"
        );
        let err = host
            .launch(&TaskId::from(1), LaunchTarget::Here)
            .unwrap_err();
        assert!(err.starts_with("could not run tasq pick: "), "{err}");
        let err = host
            .launch(&TaskId::from(1), LaunchTarget::Window { focus: true })
            .unwrap_err();
        assert!(err.starts_with("could not run tasq pick: "), "{err}");
        let err = host.sync(&["inbox".to_owned()]).unwrap_err();
        assert!(err.starts_with("could not run tasq sync: "), "{err}");
        host.editor.clear();
        assert_eq!(
            host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err(),
            "no editor configured (set $EDITOR)"
        );
    }

    #[test]
    fn window_launches_are_captured_and_report_the_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(dir.path(), "");
        // `sh -c '<script>' pick 1 --window [--no-focus]`: the script sees
        // the arguments the host would give `tasq`.
        let script = |body: &str| CliHost {
            app: &app,
            exe: "/bin/sh".into(),
            global_args: vec!["-c".into(), body.into(), "sh".into()],
            editor: Vec::new(),
        };
        let mut host = script(
            "echo \"args: $*\"; echo; echo 'Opened herdr workspace w1 (\"x\") with agent task-1'",
        );
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: true }),
            Ok("Opened herdr workspace w1 (\"x\") with agent task-1".into())
        );
        let mut host = script("echo \"args: $*\"");
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: false }),
            Ok("args: pick 1 --window --no-focus".into())
        );
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: true }),
            Ok("args: pick 1 --window".into())
        );
        let mut host = script("exit 0");
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: true }),
            Ok("tasq pick finished".into())
        );
        let mut host = script("echo 'tasq: error: launch.window: auto: no window' >&2; exit 2");
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: true }),
            Err("launch.window: auto: no window".into())
        );
        let mut host = script("exit 3");
        assert_eq!(
            host.launch(&TaskId::from(1), LaunchTarget::Window { focus: true }),
            Err("tasq pick exited with 3".into())
        );
    }

    #[test]
    fn host_reports_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_in(dir.path(), "");
        let mut host = CliHost {
            app: &app,
            exe: "/bin/sh".into(),
            global_args: vec!["-c".into()],
            editor: vec!["/bin/sh".into(), "-c".into(), "exit 3".into(), "sh".into()],
        };
        let err = host.edit(&TaskId::from(1), Path::new("/f")).unwrap_err();
        assert_eq!(err, "/bin/sh exited with 3");
        // `sh -c pick 1` runs a command called `pick`, which does not exist.
        let err = host
            .launch(&TaskId::from(1), LaunchTarget::Here)
            .unwrap_err();
        assert_eq!(err, "tasq pick exited with 127");
        host.editor = vec!["/bin/sh".into(), "-c".into(), "true".into(), "sh".into()];
        assert_eq!(
            host.edit(&TaskId::from(1), Path::new("/f")).unwrap(),
            "[1] edited with /bin/sh"
        );
    }
}
