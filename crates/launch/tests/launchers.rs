//! The launchers against fake `direnv`, `tmux` and `herdr` executables and
//! a recording fallback launcher. The Claude and shell launchers `exec`,
//! which a test cannot observe, so they are checked through `command()` and
//! `describe()`; a failed exec is tested alone in `exec_failure.rs`.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use tasq_core::config::{EnvStrategy, Placement};
use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher};
use tasq_core::model::{Session, Task, TaskId, Worktree};
use tasq_launch::process::Trace;
use tasq_launch::prompt::DEFAULT_TEMPLATE;
use tasq_launch::{
    ClaudeLauncher, EnvrcStatus, HerdrLauncher, TmuxLauncher, command_in, envrc_status,
    wrap_command,
};
use tempfile::TempDir;

/// A temp tree with `bin/` as the only `PATH` entry and `work/` as the
/// working directory.
struct Sandbox {
    root: TempDir,
    bin: PathBuf,
    work: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let work = root.path().join("work");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        Self { root, bin, work }
    }

    fn env(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut env = vec![("PATH".to_owned(), self.bin.display().to_string())];
        env.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        env
    }

    /// Installs an executable `name` whose argv (one line per call) is
    /// appended to `<root>/<name>.log` before `body` runs.
    fn fake(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let log = self.root.path().join(format!("{name}.log"));
        let script = self.bin.join(name);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\nprintf '%s\\n' \"$*\" >> '{}'\n{body}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..200 {
            if std::process::Command::new(&script)
                .env("FAKE_PROBE", "1")
                .output()
                .is_ok()
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("fake {name} never became runnable");
    }

    fn log(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join(format!("{name}.log")))
            .map(|s| s.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn ctx(&self) -> LaunchContext {
        let mut task = Task::new(TaskId::from(3), "Fix the login form validation");
        task.add_worktree(Worktree::on_branch("/wt/feature-a", "feature-a"));
        task.add_session(Session::new(
            tasq_core::clock::FixedClock::at("2026-10-04 10:20").0,
            "abc-123",
        ));
        LaunchContext {
            task,
            file: "/store/01ARZ3NDEKTSV4RRFFQ69G5FAV.md".into(),
            markdown:
                "---\nid: 3\nstatus: ready\npriority: B\n---\n# Fix the login form validation\n"
                    .into(),
            workdir: self.work.clone(),
            in_worktree: false,
            worktrees: true,
            env: vec![
                ("TASQ_TASK_ID".to_owned(), "3".to_owned()),
                ("TASQ_STORE_NAME".to_owned(), "home".to_owned()),
            ],
            statuses: vec!["in-progress".into(), "ready".into(), "waiting".into()],
            focus: true,
        }
    }
}

/// A launcher that records what it was asked to launch.
struct Recording {
    calls: Rc<RefCell<Vec<PathBuf>>>,
}

impl Launcher for Recording {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn describe(&self, _ctx: &LaunchContext) -> Result<Vec<String>, LaunchError> {
        Ok(vec!["recording".to_owned()])
    }

    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError> {
        self.calls.borrow_mut().push(ctx.workdir.clone());
        Ok(LaunchOutcome::Opened("recorded".to_owned()))
    }

    fn resume_hint(&self, session_id: &str) -> Option<String> {
        Some(format!("recorded {session_id}"))
    }
}

// ---------------------------------------------------------------------------
// direnv
// ---------------------------------------------------------------------------

#[test]
fn envrc_status_through_a_fake_direnv() {
    let sb = Sandbox::new();
    let env = sb.env(&[]);
    assert_eq!(envrc_status(&sb.work, &env), EnvrcStatus::NoDirenv);
    sb.fake(
        "direnv",
        "echo 'Found RC path x'; echo 'Found RC allowed true'",
    );
    assert_eq!(envrc_status(&sb.work, &env), EnvrcStatus::NoEnvrc);
    std::fs::write(sb.work.join(".envrc"), "export X=1\n").unwrap();
    assert_eq!(envrc_status(&sb.work, &env), EnvrcStatus::Allowed);
    assert_eq!(sb.log("direnv"), vec!["status"]);
    sb.fake("direnv", "echo 'Found RC allowed false'");
    assert_eq!(envrc_status(&sb.work, &env), EnvrcStatus::NotAllowed);
    sb.fake("direnv", "exit 1");
    assert_eq!(envrc_status(&sb.work, &env), EnvrcStatus::NotAllowed);
}

// ---------------------------------------------------------------------------
// claude
// ---------------------------------------------------------------------------

fn claude(sb: &Sandbox, strategy: EnvStrategy) -> ClaudeLauncher {
    ClaudeLauncher {
        env: sb.env(&[]),
        strategy,
        template: DEFAULT_TEMPLATE.to_owned(),
        in_herdr: false,
    }
}

#[test]
fn claude_prompt_snapshot() {
    let sb = Sandbox::new();
    let launcher = claude(&sb, EnvStrategy::Inherit);
    let ctx = sb.ctx();
    let prompt = launcher.prompt(&ctx).unwrap();
    let normalized = prompt.replace(&sb.work.display().to_string(), "[WORK]");
    insta::assert_snapshot!(normalized);
    let herdr = ClaudeLauncher {
        in_herdr: true,
        ..launcher
    };
    assert!(
        herdr
            .prompt(&ctx)
            .unwrap()
            .contains("herdr workspace rename $HERDR_WORKSPACE_ID")
    );
    assert!(!prompt.contains("herdr workspace rename"));
}

#[test]
fn claude_prompt_without_worktrees_says_nothing_about_them() {
    let sb = Sandbox::new();
    let launcher = claude(&sb, EnvStrategy::Inherit);
    let ctx = LaunchContext {
        worktrees: false,
        ..sb.ctx()
    };
    let prompt = launcher.prompt(&ctx).unwrap();
    assert!(!prompt.to_lowercase().contains("worktree"), "{prompt}");
    assert!(
        prompt.contains("tasq project 3 <path>. Track Claude sessions with:"),
        "{prompt}"
    );
    let on = launcher.prompt(&sb.ctx()).unwrap();
    assert!(on.contains("/wt/feature-a (`feature-a`)"), "{on}");
    assert!(
        on.contains("tasq worktree set 3 <branch>, then create it with: tasq worktree up 3, which runs git worktree add. Track Claude sessions"),
        "{on}"
    );
    assert!(!on.contains("--create"), "{on}");
}

#[test]
fn claude_command_wraps_with_direnv_only_when_allowed() {
    let sb = Sandbox::new();
    let ctx = sb.ctx();
    let launcher = claude(&sb, EnvStrategy::Direnv);
    // No direnv on PATH: plain claude.
    let (argv, warning) = launcher.command(&ctx).unwrap();
    assert_eq!(argv[0], "claude");
    assert_eq!(argv.len(), 2);
    assert_eq!(warning, None);
    // Allowed .envrc: wrapped.
    std::fs::write(sb.work.join(".envrc"), "").unwrap();
    sb.fake("direnv", "echo 'Found RC allowed true'");
    let (argv, warning) = launcher.command(&ctx).unwrap();
    assert_eq!(
        &argv[..4],
        &["direnv", "exec", &sb.work.display().to_string(), "claude"]
    );
    assert_eq!(warning, None);
    // Not allowed: plain, with the warning.
    sb.fake("direnv", "echo 'Found RC allowed false'");
    let (argv, warning) = launcher.command(&ctx).unwrap();
    assert_eq!(argv[0], "claude");
    assert_eq!(
        warning.as_deref(),
        Some(
            format!(
                "{}/.envrc is not allowed by direnv; run: direnv allow {}",
                sb.work.display(),
                sb.work.display()
            )
            .as_str()
        )
    );
    // Inherit never wraps even when allowed.
    sb.fake("direnv", "echo 'Found RC allowed true'");
    let inherit = claude(&sb, EnvStrategy::Inherit);
    let (argv, _) = inherit.command(&ctx).unwrap();
    assert_eq!(argv[0], "claude");
    // The task-less form the CLI uses for `sync --interactive`: the same
    // wrapping around a literal prompt.
    assert_eq!(
        command_in(
            &sb.work,
            "/tasq:sync".to_owned(),
            EnvStrategy::Direnv,
            &sb.env(&[])
        ),
        (
            vec![
                "direnv".to_owned(),
                "exec".to_owned(),
                sb.work.display().to_string(),
                "claude".to_owned(),
                "/tasq:sync".to_owned(),
            ],
            None
        )
    );
    assert_eq!(
        command_in(
            &sb.work,
            "/tasq:sync".to_owned(),
            EnvStrategy::Inherit,
            &sb.env(&[])
        ),
        (vec!["claude".to_owned(), "/tasq:sync".to_owned()], None)
    );
    assert_eq!(
        wrap_command(
            EnvStrategy::Inherit,
            EnvrcStatus::Allowed,
            &sb.work,
            vec!["x".into()]
        ),
        vec!["x"]
    );
}

#[test]
fn claude_describe_and_hint() {
    let sb = Sandbox::new();
    let launcher = claude(&sb, EnvStrategy::Inherit);
    let ctx = sb.ctx();
    let lines = launcher.describe(&ctx).unwrap();
    assert_eq!(lines[0], format!("cd {}", sb.work.display()));
    assert_eq!(
        lines[1],
        "TASQ_TASK_ID=3 TASQ_STORE_NAME=home exec claude \"<prompt below>\""
    );
    assert_eq!(lines[2], "");
    assert_eq!(lines[3], "--- prompt ---");
    assert!(lines[4].starts_with("Work with me on my next task: todo [3]"));
    assert_eq!(
        launcher.resume_hint("abc").as_deref(),
        Some("claude --resume abc")
    );
    assert_eq!(launcher.name(), "claude");
    let bad = ClaudeLauncher {
        template: "{{nope}}".to_owned(),
        ..launcher
    };
    assert_eq!(
        bad.describe(&ctx).unwrap_err(),
        LaunchError::Template("unknown placeholder {{nope}}".into())
    );
}

// ---------------------------------------------------------------------------
// tmux
// ---------------------------------------------------------------------------

#[test]
fn tmux_opens_a_window_through_the_fake() {
    let sb = Sandbox::new();
    sb.fake("tmux", "exit 0");
    let trace = sb.root.path().join("launch.log");
    let launcher = TmuxLauncher {
        env: sb.env(&[("TMUX", "/tmp/tmux-1000/default,1,0")]),
        trace: Trace {
            stderr: false,
            file: Some(trace.clone()),
        },
    };
    let ctx = sb.ctx();
    let outcome = launcher.launch(&ctx).unwrap();
    assert_eq!(
        outcome,
        LaunchOutcome::Opened(format!(
            "opened tmux window \"3 Fix the login\" in {}",
            sb.work.display()
        ))
    );
    assert_eq!(
        sb.log("tmux"),
        vec![format!(
            "new-window -c {} -n 3 Fix the login -e TASQ_TASK_ID=3 -e TASQ_STORE_NAME=home",
            sb.work.display()
        )]
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap(),
        format!(
            "$ tmux new-window -c {} -n '3 Fix the login' -e 'TASQ_TASK_ID=3' -e 'TASQ_STORE_NAME=home'\n  ok\n",
            sb.work.display()
        )
    );
    sb.fake("tmux", "echo 'no server running' >&2; exit 1");
    assert_eq!(
        launcher.launch(&ctx).unwrap_err(),
        LaunchError::Tool {
            tool: "tmux".into(),
            message: "no server running".into()
        }
    );
}

// ---------------------------------------------------------------------------
// herdr
// ---------------------------------------------------------------------------

fn herdr(
    sb: &Sandbox,
    default_project: Option<&Path>,
) -> (HerdrLauncher, Rc<RefCell<Vec<PathBuf>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let launcher = HerdrLauncher {
        env: sb.env(&[("HERDR_ENV", "1")]),
        default_project: default_project.map(Path::to_path_buf),
        placement: Placement::Auto,
        fallback: Box::new(Recording {
            calls: Rc::clone(&calls),
        }),
        prompt: Box::new(|ctx| Ok(format!("prompt for {}", ctx.task.id))),
        trace: Trace::default(),
        sleep: Box::new(|_| {}),
    };
    (launcher, calls)
}

/// A fake herdr answering each subcommand; `$1 $2` is the subcommand pair.
const HERDR_BODY: &str = r#"case "$1 $2" in
  "worktree list") echo "$HERDR_WORKTREES" ;;
  "workspace create") echo '{"ok":true,"workspace":{"workspace_id":"ws-new","panes":[{"pane_id":"pane-new"}]}}' ;;
  "tab create") echo '{"tab_id":"tab-7","pane_id":"pane-7"}' ;;
  "agent start") [ "$3" = "task-3" ] && [ -n "$HERDR_FIRST_START_FAILS" ] && exit 1; exit 0 ;;
  *) exit 0 ;;
esac"#;

#[test]
fn herdr_creates_a_workspace_and_starts_the_agent() {
    let sb = Sandbox::new();
    sb.fake("herdr", HERDR_BODY);
    let (launcher, calls) = herdr(&sb, None);
    let ctx = sb.ctx();
    let outcome = launcher.launch(&ctx).unwrap();
    assert_eq!(
        outcome,
        LaunchOutcome::Opened(
            "Opened herdr workspace ws-new (\"Fix the login\") with agent task-3".into()
        )
    );
    let work = sb.work.display().to_string();
    assert_eq!(
        sb.log("herdr"),
        vec![
            format!("worktree list --cwd {work}"),
            format!(
                "workspace create --label Fix the login --cwd {work} --env TASQ_TASK_ID=3 --env TASQ_STORE_NAME=home --no-focus"
            ),
            // The view switches before the agent's readiness wait.
            "workspace focus ws-new".to_owned(),
            "agent start task-3 --kind claude --pane pane-new --timeout 90000".to_owned(),
            "agent prompt task-3 prompt for 3".to_owned(),
            "agent focus task-3".to_owned(),
        ]
    );
    assert_eq!(calls.borrow().len(), 0, "no fallback");
    assert_eq!(launcher.resume_hint("s").as_deref(), Some("recorded s"));
    assert_eq!(launcher.name(), "herdr");
}

#[test]
fn herdr_reuses_the_holding_workspace_as_a_tab_and_retries_the_agent_name() {
    let sb = Sandbox::new();
    sb.fake("herdr", HERDR_BODY);
    let (mut launcher, _) = herdr(&sb, None);
    let work = sb.work.display().to_string();
    launcher.env.push((
        "HERDR_WORKTREES".to_owned(),
        format!("{{\"worktrees\":[{{\"path\":\"{work}\",\"open_workspace_id\":\"ws-held\"}}]}}"),
    ));
    launcher
        .env
        .push(("HERDR_FIRST_START_FAILS".to_owned(), "1".to_owned()));
    let ctx = sb.ctx();
    let outcome = launcher.launch(&ctx).unwrap();
    let agent = format!("task-3-{}", std::process::id());
    assert_eq!(
        outcome,
        LaunchOutcome::Opened(format!(
            "Opened herdr workspace ws-held (\"Fix the login\") with agent {agent}"
        ))
    );
    assert_eq!(
        sb.log("herdr"),
        vec![
            format!("worktree list --cwd {work}"),
            format!(
                "tab create --workspace ws-held --cwd {work} --label Fix the login --env TASQ_TASK_ID=3 --env TASQ_STORE_NAME=home --no-focus"
            ),
            "tab focus tab-7".to_owned(),
            "workspace focus ws-held".to_owned(),
            "agent start task-3 --kind claude --pane pane-7 --timeout 90000".to_owned(),
            format!("agent start {agent} --kind claude --pane pane-7 --timeout 90000"),
            format!("agent prompt {agent} prompt for 3"),
            format!("agent focus {agent}"),
        ]
    );
}

#[test]
fn herdr_without_focus_opens_in_the_background() {
    let sb = Sandbox::new();
    sb.fake("herdr", HERDR_BODY);
    let (launcher, _) = herdr(&sb, None);
    let mut ctx = sb.ctx();
    ctx.focus = false;
    assert_eq!(
        launcher.launch(&ctx).unwrap(),
        LaunchOutcome::Opened(
            "Opened herdr workspace ws-new (\"Fix the login\") with agent task-3 in the background"
                .into()
        )
    );
    let log = sb.log("herdr");
    assert_eq!(log.len(), 4, "{log:?}");
    assert!(log[3].starts_with("agent prompt task-3"), "{log:?}");
    assert!(!log.iter().any(|l| l.contains("focus ")), "{log:?}");
    let plan = launcher.describe(&ctx).unwrap();
    assert!(
        plan.contains(&"(no focus change: the session opens in the background)".to_owned()),
        "{plan:?}"
    );
    assert!(
        !plan.iter().any(|l| l.starts_with("herdr workspace focus")),
        "{plan:?}"
    );
}

#[test]
fn herdr_workspace_placement_never_looks_for_a_holding_workspace() {
    let sb = Sandbox::new();
    sb.fake("herdr", HERDR_BODY);
    let (mut launcher, _) = herdr(&sb, None);
    launcher.placement = Placement::Workspace;
    let work = sb.work.display().to_string();
    launcher.env.push((
        "HERDR_WORKTREES".to_owned(),
        format!("{{\"worktrees\":[{{\"path\":\"{work}\",\"open_workspace_id\":\"ws-held\"}}]}}"),
    ));
    let ctx = sb.ctx();
    assert_eq!(
        launcher.launch(&ctx).unwrap(),
        LaunchOutcome::Opened(
            "Opened herdr workspace ws-new (\"Fix the login\") with agent task-3".into()
        )
    );
    let log = sb.log("herdr");
    assert!(log[0].starts_with("workspace create"), "{log:?}");
    assert!(
        !log.iter().any(|l| l.starts_with("worktree list")),
        "{log:?}"
    );
    let plan = launcher.describe(&ctx).unwrap();
    assert!(
        !plan.iter().any(|l| l.contains("worktree list")),
        "{plan:?}"
    );
}

#[test]
fn herdr_tab_placement_uses_the_holding_workspace_else_the_current_one() {
    let sb = Sandbox::new();
    sb.fake("herdr", HERDR_BODY);
    let (mut launcher, _) = herdr(&sb, Some(&sb.work));
    launcher.placement = Placement::Tab;
    let work = sb.work.display().to_string();
    launcher
        .env
        .push(("HERDR_WORKSPACE_ID".to_owned(), "ws-current".to_owned()));
    let ctx = sb.ctx();
    // No workspace holds the directory (the fake prints an empty list), so
    // the tab goes into the current workspace, even in the default project.
    let outcome = launcher.launch(&ctx).unwrap();
    assert_eq!(
        outcome,
        LaunchOutcome::Opened(
            "Opened herdr workspace ws-current (\"Fix the login\") with agent task-3".into()
        )
    );
    assert_eq!(
        sb.log("herdr")[..2],
        [
            format!("worktree list --cwd {work}"),
            format!(
                "tab create --workspace ws-current --cwd {work} --label Fix the login --env TASQ_TASK_ID=3 --env TASQ_STORE_NAME=home --no-focus"
            ),
        ]
    );
    let plan = launcher.describe(&ctx).unwrap();
    assert!(
        plan[0].ends_with("(a new tab in the workspace holding it, else in the current one)"),
        "{plan:?}"
    );
    // A holding workspace wins over the current one.
    let sb2 = Sandbox::new();
    sb2.fake("herdr", HERDR_BODY);
    let (mut launcher, _) = herdr(&sb2, None);
    launcher.placement = Placement::Tab;
    let work2 = sb2.work.display().to_string();
    launcher.env.push((
        "HERDR_WORKTREES".to_owned(),
        format!("{{\"worktrees\":[{{\"path\":\"{work2}\",\"open_workspace_id\":\"ws-held\"}}]}}"),
    ));
    launcher
        .env
        .push(("HERDR_WORKSPACE_ID".to_owned(), "ws-current".to_owned()));
    launcher.launch(&sb2.ctx()).unwrap();
    assert!(
        sb2.log("herdr")[1].starts_with("tab create --workspace ws-held "),
        "{:?}",
        sb2.log("herdr")
    );
}

#[test]
fn herdr_skips_the_lookup_in_the_default_project_and_falls_back_without_a_pane() {
    let sb = Sandbox::new();
    sb.fake("herdr", "case \"$1 $2\" in \"workspace create\") echo '{\"error\":\"no server\"}' ;; *) exit 0 ;; esac");
    let (launcher, calls) = herdr(&sb, Some(&sb.work));
    let ctx = sb.ctx();
    assert_eq!(
        launcher.launch(&ctx).unwrap(),
        LaunchOutcome::Opened("recorded".into())
    );
    assert_eq!(calls.borrow().as_slice(), std::slice::from_ref(&sb.work));
    let log = sb.log("herdr");
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with("workspace create"), "{log:?}");
}

#[test]
fn herdr_traces_its_calls_and_warnings_to_the_log_file() {
    let sb = Sandbox::new();
    sb.fake(
        "herdr",
        "case \"$1 $2\" in \"workspace create\") echo 'error: server not running' >&2; exit 1 ;; *) exit 0 ;; esac",
    );
    let (mut launcher, calls) = herdr(&sb, Some(&sb.work));
    let log = sb.root.path().join("launch.log");
    launcher.trace = Trace {
        stderr: false,
        file: Some(log.clone()),
    };
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap(),
        LaunchOutcome::Opened("recorded".into())
    );
    assert_eq!(calls.borrow().len(), 1, "fell back");
    let work = sb.work.display().to_string();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!(
            "$ herdr workspace create --label 'Fix the login' --cwd {work} --env 'TASQ_TASK_ID=3' \
             --env 'TASQ_STORE_NAME=home' --no-focus\n  failed\n  stderr:\n    error: server not running\n\
             warning: herdr could not open a pane (herdr workspace create failed: error: server not running); \
             opening in the current pane with recording\n"
        )
    );
}

#[test]
fn herdr_warns_when_a_focus_step_fails() {
    let sb = Sandbox::new();
    sb.fake(
        "herdr",
        &HERDR_BODY.replace(
            "  *) exit 0 ;;",
            "  \"workspace focus\") echo 'no such workspace' >&2; exit 1 ;;\n  *) exit 0 ;;",
        ),
    );
    let (mut launcher, _) = herdr(&sb, Some(&sb.work));
    let log = sb.root.path().join("launch.log");
    launcher.trace = Trace {
        stderr: false,
        file: Some(log.clone()),
    };
    launcher.launch(&sb.ctx()).unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("\nwarning: herdr workspace focus ws-new failed: no such workspace\n"),
        "{text}"
    );
    assert!(
        text.ends_with("$ herdr agent focus task-3\n  ok\n"),
        "{text}"
    );
}

#[test]
fn herdr_agent_failures_and_absence() {
    let sb = Sandbox::new();
    sb.fake(
        "herdr",
        "case \"$1 $2\" in \"workspace create\") echo '{\"workspace_id\":\"w\",\"pane_id\":\"p\"}' ;; \"agent start\") echo 'agent did not come up' >&2; exit 1 ;; *) exit 0 ;; esac",
    );
    let (launcher, _) = herdr(&sb, None);
    let ctx = sb.ctx();
    assert_eq!(
        launcher.launch(&ctx).unwrap_err(),
        LaunchError::Tool {
            tool: "herdr agent start".into(),
            message: format!(
                "task-3: agent did not come up; then task-3-{}: agent did not come up",
                std::process::id()
            )
        }
    );
    sb.fake(
        "herdr",
        "case \"$1 $2\" in \"workspace create\") echo '{\"workspace_id\":\"w\",\"pane_id\":\"p\"}' ;; \"agent prompt\") exit 2 ;; *) exit 0 ;; esac",
    );
    assert!(matches!(
        launcher.launch(&ctx).unwrap_err(),
        LaunchError::Tool { ref tool, .. } if tool == "herdr agent prompt"
    ));
    let (outside, _) = herdr(&sb, None);
    let outside = HerdrLauncher {
        env: sb.env(&[]),
        ..outside
    };
    assert_eq!(
        outside.describe(&ctx).unwrap_err(),
        LaunchError::Unavailable {
            launcher: "herdr".into(),
            reason: "not inside herdr (HERDR_ENV is unset)".into()
        }
    );
}

/// A fake herdr whose `agent start` answers follow `$HERDR_START_SCRIPT`
/// (one word per call: `busy`, `taken` or `ok`; `$HERDR_START_DEFAULT`
/// past its end), counting calls in `$HERDR_COUNT`, and whose
/// `pane process-info` prints `$HERDR_PROCESS_INFO`.
const HERDR_RACE_BODY: &str = r#"case "$1 $2" in
  "workspace create") echo '{"workspace_id":"ws-new","pane_id":"pane-new"}' ;;
  "agent start")
    n=0; [ -f "$HERDR_COUNT" ] && read -r n < "$HERDR_COUNT"; n=$((n+1)); echo "$n" > "$HERDR_COUNT"
    i=0; step=$HERDR_START_DEFAULT; for w in $HERDR_START_SCRIPT; do i=$((i+1)); [ "$i" = "$n" ] && step=$w; done
    case "$step" in
      busy) printf '{"error":{"code":"agent_pane_busy","message":"agent target pane %s is not an available shell"},"id":"cli:agent:start"}\n' "$7" >&2; exit 1 ;;
      taken) echo "error: agent $3 already exists" >&2; exit 1 ;;
      *) exit 0 ;;
    esac ;;
  "pane process-info") echo "$HERDR_PROCESS_INFO" ;;
  *) exit 0 ;;
esac"#;

const PROCESS_INFO: &str = r#"{"id":"cli:pane:process_info","result":{"process_info":{"foreground_process_group_id":42,"foreground_processes":[{"argv":["gitstatusd"],"name":"gitstatusd","pid":42}],"pane_id":"pane-new","shell_pid":7},"type":"pane_process_info"}}"#;

/// The herdr launcher over [`HERDR_RACE_BODY`] in the default project (no
/// `worktree list`), recording every sleep and tracing to `launch.log`.
fn racing_herdr(
    sb: &Sandbox,
    script: &str,
    default: &str,
) -> (HerdrLauncher, Rc<RefCell<Vec<Duration>>>, PathBuf) {
    sb.fake("herdr", HERDR_RACE_BODY);
    let (mut launcher, _) = herdr(sb, Some(&sb.work));
    let count = sb.root.path().join("start.count");
    launcher.env.extend([
        ("HERDR_COUNT".to_owned(), count.display().to_string()),
        ("HERDR_START_SCRIPT".to_owned(), script.to_owned()),
        ("HERDR_START_DEFAULT".to_owned(), default.to_owned()),
        ("HERDR_PROCESS_INFO".to_owned(), PROCESS_INFO.to_owned()),
    ]);
    let slept = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&slept);
    launcher.sleep = Box::new(move |d| record.borrow_mut().push(d));
    let log = sb.root.path().join("launch.log");
    launcher.trace = Trace {
        stderr: false,
        file: Some(log.clone()),
    };
    (launcher, slept, log)
}

#[test]
fn herdr_retries_agent_start_while_the_pane_shell_starts() {
    let sb = Sandbox::new();
    let (launcher, slept, log) = racing_herdr(&sb, "busy busy busy ok", "ok");
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap(),
        LaunchOutcome::Opened(
            "Opened herdr workspace ws-new (\"Fix the login\") with agent task-3".into()
        )
    );
    assert_eq!(*slept.borrow(), vec![Duration::from_millis(100); 3]);
    let starts: Vec<String> = sb
        .log("herdr")
        .into_iter()
        .filter(|l| l.starts_with("agent start"))
        .collect();
    assert_eq!(starts.len(), 4, "{starts:?}");
    assert!(
        starts
            .iter()
            .all(|l| l.starts_with("agent start task-3 --kind")),
        "{starts:?}"
    );
    // Only the attempt that got through is traced, plus the wait.
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(text.matches("$ herdr agent start").count(), 1, "{text}");
    assert!(
        text.ends_with(
            "$ herdr workspace focus ws-new\n  ok\n\
             $ herdr agent start task-3 --kind claude --pane pane-new --timeout 90000\n  ok\n\
             waited 300 ms for the pane's shell (3 agent_pane_busy replies)\n\
             $ herdr agent prompt task-3 'prompt for 3'\n  ok\n\
             $ herdr agent focus task-3\n  ok\n"
        ),
        "{text}"
    );
}

#[test]
fn herdr_start_without_a_wait_leaves_no_note() {
    let sb = Sandbox::new();
    let (launcher, slept, log) = racing_herdr(&sb, "ok", "ok");
    launcher.launch(&sb.ctx()).unwrap();
    assert_eq!(*slept.borrow(), Vec::new());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(!text.contains("waited"), "{text}");
    assert_eq!(text.matches("$ herdr agent start").count(), 1, "{text}");
}

#[test]
fn herdr_names_the_foreground_process_when_the_pane_stays_busy() {
    let sb = Sandbox::new();
    let (launcher, slept, log) = racing_herdr(&sb, "", "busy");
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap_err(),
        LaunchError::Tool {
            tool: "herdr agent start".into(),
            message: "task-3: agent target pane pane-new is not an available shell after 5000 ms; \
                      foreground: gitstatusd (pid 42)"
                .into()
        }
    );
    assert_eq!(*slept.borrow(), vec![Duration::from_millis(100); 50]);
    let calls = sb.log("herdr");
    assert_eq!(
        calls
            .iter()
            .filter(|l| l.starts_with("agent start task-3 --kind"))
            .count(),
        51
    );
    // The second name is for a taken name, not a busy pane.
    assert!(
        !calls.iter().any(|l| l.starts_with("agent start task-3-")),
        "{calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|l| l.starts_with("pane process-info"))
            .collect::<Vec<_>>(),
        vec!["pane process-info --pane pane-new"]
    );
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(text.matches("$ herdr agent start").count(), 1, "{text}");
    assert!(
        text.contains(&format!(
            "$ herdr agent start task-3 --kind claude --pane pane-new --timeout 90000\n  failed\n  stderr:\n    \
             {{\"error\":{{\"code\":\"agent_pane_busy\",\"message\":\"agent target pane pane-new is not an available shell\"}},\"id\":\"cli:agent:start\"}}\n\
             waited 5000 ms for the pane's shell (50 agent_pane_busy replies)\n\
             $ herdr pane process-info --pane pane-new\n  ok\n  stdout:\n    {PROCESS_INFO}\n"
        )),
        "{text}"
    );
}

#[test]
fn herdr_busy_pane_error_says_when_process_info_fails() {
    let sb = Sandbox::new();
    let (mut launcher, _, _) = racing_herdr(&sb, "", "busy");
    launcher.env.retain(|(k, _)| k != "HERDR_PROCESS_INFO");
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap_err(),
        LaunchError::Tool {
            tool: "herdr agent start".into(),
            message: "task-3: agent target pane pane-new is not an available shell after 5000 ms; \
                      foreground: process-info failed: "
                .into()
        }
    );
}

#[test]
fn herdr_wait_budget_is_shared_across_both_names() {
    let sb = Sandbox::new();
    // The first name waits twice, is taken; the second waits once more.
    let (launcher, slept, log) = racing_herdr(&sb, "busy busy taken busy ok", "ok");
    let agent = format!("task-3-{}", std::process::id());
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap(),
        LaunchOutcome::Opened(format!(
            "Opened herdr workspace ws-new (\"Fix the login\") with agent {agent}"
        ))
    );
    assert_eq!(*slept.borrow(), vec![Duration::from_millis(100); 3]);
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.contains(
            "$ herdr agent start task-3 --kind claude --pane pane-new --timeout 90000\n  failed\n  \
             stderr:\n    error: agent task-3 already exists\n\
             waited 200 ms for the pane's shell (2 agent_pane_busy replies)\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "$ herdr agent start {agent} --kind claude --pane pane-new --timeout 90000\n  ok\n\
             waited 100 ms for the pane's shell (1 agent_pane_busy replies)\n"
        )),
        "{text}"
    );

    // One wait on the first name leaves 49 for the second.
    let sb = Sandbox::new();
    let (launcher, slept, _) = racing_herdr(&sb, "busy taken", "busy");
    assert_eq!(
        launcher.launch(&sb.ctx()).unwrap_err(),
        LaunchError::Tool {
            tool: "herdr agent start".into(),
            message: format!(
                "task-3: error: agent task-3 already exists; then {agent}: agent target pane \
                 pane-new is not an available shell after 4900 ms; foreground: gitstatusd (pid 42)"
            )
        }
    );
    assert_eq!(*slept.borrow(), vec![Duration::from_millis(100); 50]);
}

#[test]
fn herdr_describe_lists_the_steps() {
    let sb = Sandbox::new();
    let (launcher, _) = herdr(&sb, None);
    let lines = launcher.describe(&sb.ctx()).unwrap();
    assert!(lines[0].starts_with("herdr worktree list --cwd"));
    assert!(lines[1].starts_with("herdr workspace create --label \"Fix the login\""));
    assert_eq!(
        lines[3],
        "herdr agent start task-3 --kind claude --pane <pane> --timeout 90000   \
         (retried every 100 ms, up to 5 s, while the pane's shell starts)"
    );
    assert!(lines.iter().any(|l| l == "--- prompt ---"));
    assert_eq!(lines.last().map(String::as_str), Some("prompt for 3"));
    let (in_default, _) = herdr(&sb, Some(&sb.work));
    let lines = in_default.describe(&sb.ctx()).unwrap();
    assert!(lines[0].starts_with("herdr workspace create"));
}
