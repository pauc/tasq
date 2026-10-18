//! The herdr launcher (plan T-405): a workspace (or a tab in the workspace
//! already holding the directory, as `launch.herdr.placement` says), a
//! Claude agent started in its pane, the prompt pasted in, focus moved
//! there unless the context asks otherwise. Falls back to another launcher
//! in the current pane when herdr cannot create the pane.

use std::fmt::{self, Debug};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use tasq_core::config::Placement;
use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher, short_label};

use crate::env::{envrc_status, envrc_warning};
use crate::process::{Finished, Trace, env_var, run};

/// Environment variable herdr sets inside its panes.
pub const ENV_HERDR: &str = "HERDR_ENV";
/// Environment variable naming the workspace of the current pane; where a
/// `tab` placement lands when no workspace holds the task's directory.
pub const ENV_HERDR_WORKSPACE: &str = "HERDR_WORKSPACE_ID";
/// Readiness timeout passed to `herdr agent start`, in milliseconds.
pub const AGENT_TIMEOUT_MS: &str = "90000";
/// herdr's error code when the pane's foreground is not an idle shell.
pub const PANE_BUSY: &str = "agent_pane_busy";
/// Pause between `agent start` attempts while the new pane's shell starts.
pub const SHELL_WAIT_INTERVAL: Duration = Duration::from_millis(100);
/// Attempts after the first one, so the shell gets up to 5 s to reach its
/// prompt (zsh with rc files, direnv and gitstatus takes 100-300 ms here).
pub const SHELL_WAIT_RETRIES: u32 = 50;

/// Renders the prompt pasted into the agent.
pub type PromptFn = Box<dyn Fn(&LaunchContext) -> Result<String, LaunchError>>;

/// Opens the session in herdr.
pub struct HerdrLauncher {
    /// Environment `herdr` runs with and `HERDR_ENV` is read from.
    pub env: Vec<(String, String)>,
    /// `work.default_project`: a session there never looks for a holding
    /// workspace (the script's `$workdir != $DEFAULT_WORKTREE`).
    pub default_project: Option<PathBuf>,
    /// `launch.herdr.placement`: what a new window is.
    pub placement: Placement,
    /// Renders the prompt and takes over when herdr cannot open a pane.
    pub fallback: Box<dyn Launcher>,
    /// Builds the prompt pasted into the agent.
    pub prompt: PromptFn,
    /// Records every `herdr` call and warning (`-v`, `launch.log`).
    pub trace: Trace,
    /// Waits between `agent start` attempts (tests record it instead).
    pub sleep: Box<dyn Fn(Duration)>,
}

/// The pane a session opens in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pane {
    workspace: String,
    /// Set when the pane is a new tab in an existing workspace.
    tab: Option<String>,
    id: String,
}

impl Debug for HerdrLauncher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HerdrLauncher")
            .field("default_project", &self.default_project)
            .field("placement", &self.placement)
            .field("fallback", &self.fallback.name())
            .field("trace", &self.trace)
            .finish_non_exhaustive()
    }
}

/// JSON values in `text`, which may hold several concatenated objects
/// (herdr prints one per command).
fn values(text: &str) -> Vec<Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .filter_map(Result::ok)
        .collect()
}

/// The first string under `key` anywhere in `value`, depth first.
fn find_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get(key) {
                return Some(s);
            }
            map.values().find_map(|v| find_string(v, key))
        }
        Value::Array(items) => items.iter().find_map(|v| find_string(v, key)),
        _ => None,
    }
}

/// The script's `herdr_field`: the first string value of `key` in herdr's
/// JSON output.
pub fn field(text: &str, key: &str) -> Option<String> {
    values(text)
        .iter()
        .find_map(|v| find_string(v, key))
        .map(str::to_owned)
}

/// Whether `herdr agent start` refused because the pane's foreground is not
/// an idle shell yet: `error.code` is [`PANE_BUSY`] on either stream.
pub fn pane_busy(out: &Finished) -> bool {
    !out.success
        && [&out.stdout, &out.stderr]
            .iter()
            .any(|text| field(text, "code").as_deref() == Some(PANE_BUSY))
}

/// The line to show for a failed herdr call: the `message` of its JSON
/// error when it printed one, else the usual stderr-or-stdout text.
pub fn herdr_message(out: &Finished) -> String {
    [&out.stderr, &out.stdout]
        .iter()
        .find_map(|text| field(text, "message"))
        .unwrap_or_else(|| out.message())
}

/// What owns the pane's foreground, from `herdr pane process-info`:
/// `<name> (pid N)`, marked as the pane's shell when it is `shell_pid`.
pub fn foreground_process(text: &str) -> Option<String> {
    values(text).iter().find_map(|v| {
        let info = v.pointer("/result/process_info")?;
        let front = info.pointer("/foreground_processes/0")?;
        let name = front.get("name")?.as_str()?;
        let pid = front.get("pid")?.as_u64()?;
        let shell = info.get("shell_pid").and_then(Value::as_u64);
        Some(if shell == Some(pid) {
            format!("{name} (pid {pid}, the pane's shell)")
        } else {
            format!("{name} (pid {pid})")
        })
    })
}

/// The `open_workspace_id` of the worktree entry whose `path` is `path`
/// (the script's `workspace_holding_path`), from `herdr worktree list`.
pub fn workspace_holding(text: &str, path: &str) -> Option<String> {
    fn walk(value: &Value, path: &str) -> Option<String> {
        match value {
            Value::Object(map) => {
                if map.get("path").and_then(Value::as_str) == Some(path)
                    && let Some(Value::String(ws)) = map.get("open_workspace_id")
                    && !ws.is_empty()
                {
                    return Some(ws.clone());
                }
                map.values().find_map(|v| walk(v, path))
            }
            Value::Array(items) => items.iter().find_map(|v| walk(v, path)),
            _ => None,
        }
    }
    values(text).iter().find_map(|v| walk(v, path))
}

impl HerdrLauncher {
    fn check(&self) -> Result<(), LaunchError> {
        if env_var(&self.env, ENV_HERDR).is_some() {
            Ok(())
        } else {
            Err(LaunchError::Unavailable {
                launcher: "herdr".to_owned(),
                reason: format!("not inside herdr ({ENV_HERDR} is unset)"),
            })
        }
    }

    fn herdr(&self, args: &[&str]) -> Finished {
        let out = run("herdr", args, None, &self.env, &[]);
        self.trace.command("herdr", args, &out);
        out
    }

    /// Runs a `herdr ... focus` step, warning when it fails: the session
    /// still opened, only the view did not move.
    fn focus(&self, args: &[&str]) {
        let out = self.herdr(args);
        if !out.success {
            self.trace.warn(&format!(
                "herdr {} failed: {}",
                args.join(" "),
                out.message()
            ));
        }
    }

    fn env_args(ctx: &LaunchContext) -> Vec<String> {
        ctx.env
            .iter()
            .flat_map(|(k, v)| ["--env".to_owned(), format!("{k}={v}")])
            .collect()
    }

    /// Whether a workspace holding the directory is looked for: never for
    /// a `workspace` placement, always for `tab`, and for `auto` unless the
    /// directory is `work.default_project`.
    fn looks_up_workspace(&self, ctx: &LaunchContext) -> bool {
        match self.placement {
            Placement::Workspace => false,
            Placement::Tab => true,
            Placement::Auto => self.default_project.as_deref() != Some(ctx.workdir.as_path()),
        }
    }

    /// The workspace a tab goes into, if any: the one holding the
    /// directory, else, for a `tab` placement, the current one.
    fn tab_workspace(&self, ctx: &LaunchContext, workdir: &str) -> Option<String> {
        if !self.looks_up_workspace(ctx) {
            return None;
        }
        let out = self.herdr(&["worktree", "list", "--cwd", workdir]);
        workspace_holding(&out.stdout, workdir).or_else(|| match self.placement {
            Placement::Tab => env_var(&self.env, ENV_HERDR_WORKSPACE).map(str::to_owned),
            Placement::Auto | Placement::Workspace => None,
        })
    }

    /// Finds or creates the pane, or says why herdr did not give one.
    fn open_pane(&self, ctx: &LaunchContext, label: &str) -> Result<Pane, String> {
        let workdir = ctx.workdir.display().to_string();
        let holding = self.tab_workspace(ctx, &workdir);
        let env_args = Self::env_args(ctx);
        let env_refs: Vec<&str> = env_args.iter().map(String::as_str).collect();
        match holding {
            None => {
                let mut args = vec!["workspace", "create", "--label", label, "--cwd", &workdir];
                args.extend(env_refs);
                args.push("--no-focus");
                let out = self.herdr(&args);
                match (
                    field(&out.stdout, "workspace_id"),
                    field(&out.stdout, "pane_id"),
                ) {
                    (Some(workspace), Some(id)) => Ok(Pane {
                        workspace,
                        tab: None,
                        id,
                    }),
                    _ => Err(no_pane("workspace create", &out)),
                }
            }
            Some(ws) => {
                let mut args = vec![
                    "tab",
                    "create",
                    "--workspace",
                    ws.as_str(),
                    "--cwd",
                    &workdir,
                    "--label",
                    label,
                ];
                args.extend(env_refs);
                args.push("--no-focus");
                let out = self.herdr(&args);
                match field(&out.stdout, "pane_id") {
                    Some(id) => Ok(Pane {
                        workspace: ws,
                        tab: field(&out.stdout, "tab_id"),
                        id,
                    }),
                    None => Err(no_pane("tab create", &out)),
                }
            }
        }
    }

    /// `herdr agent start` on `pane`, first as `task-<id>`, then as
    /// `task-<id>-<pid>` when that name is taken. The pane was created
    /// milliseconds ago and herdr refuses with [`PANE_BUSY`] until its shell
    /// reaches the prompt, so a busy reply is retried every
    /// [`SHELL_WAIT_INTERVAL`], [`SHELL_WAIT_RETRIES`] times in all. Only the
    /// last attempt of each name is traced, plus a note of the wait; a pane
    /// still busy at the end names its foreground process in the error.
    fn start_agent(&self, ctx: &LaunchContext, pane: &str) -> Result<String, LaunchError> {
        let first = format!("task-{}", ctx.task.id);
        let second = format!("task-{}-{}", ctx.task.id, std::process::id());
        let mut errors = Vec::new();
        let mut retries = 0;
        for name in [&first, &second] {
            let args = [
                "agent",
                "start",
                name,
                "--kind",
                "claude",
                "--pane",
                pane,
                "--timeout",
                AGENT_TIMEOUT_MS,
            ];
            let mut out = run("herdr", &args, None, &self.env, &[]);
            // A bounded range, not a counter in a `loop`: the wait is shared
            // by both names, and no mutation of it can spin forever.
            let mut waits = 0;
            for _ in retries..SHELL_WAIT_RETRIES {
                if !pane_busy(&out) {
                    break;
                }
                (self.sleep)(SHELL_WAIT_INTERVAL);
                out = run("herdr", &args, None, &self.env, &[]);
                waits += 1;
            }
            retries += waits;
            self.trace.command("herdr", &args, &out);
            let waited = SHELL_WAIT_INTERVAL * waits;
            if waits > 0 {
                self.trace.note(&format!(
                    "waited {} ms for the pane's shell ({waits} {PANE_BUSY} replies)",
                    waited.as_millis()
                ));
            }
            if out.success {
                return Ok(name.clone());
            }
            if pane_busy(&out) {
                let info = self.herdr(&["pane", "process-info", "--pane", pane]);
                let foreground = foreground_process(&info.stdout)
                    .unwrap_or_else(|| format!("process-info failed: {}", herdr_message(&info)));
                errors.push(format!(
                    "{name}: {} after {} ms; foreground: {foreground}",
                    herdr_message(&out),
                    waited.as_millis()
                ));
                break;
            }
            errors.push(format!("{name}: {}", herdr_message(&out)));
        }
        Err(LaunchError::Tool {
            tool: "herdr agent start".to_owned(),
            message: errors.join("; then "),
        })
    }
}

/// Why `herdr <what>` gave no pane: its error, or the reply it gave
/// without the ids.
fn no_pane(what: &str, out: &Finished) -> String {
    if out.success {
        format!(
            "herdr {what} replied without the pane's ids: {}",
            out.stdout.trim()
        )
    } else {
        format!("herdr {what} failed: {}", out.message())
    }
}

impl Launcher for HerdrLauncher {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn describe(&self, ctx: &LaunchContext) -> Result<Vec<String>, LaunchError> {
        self.check()?;
        let label = short_label(&ctx.task.title);
        let workdir = ctx.workdir.display();
        let env_args = Self::env_args(ctx).join(" ");
        let mut lines = Vec::new();
        if self.looks_up_workspace(ctx) {
            lines.push(match self.placement {
                Placement::Tab => format!(
                    "herdr worktree list --cwd {workdir}   (a new tab in the workspace holding it, else in the current one)"
                ),
                Placement::Auto | Placement::Workspace => format!(
                    "herdr worktree list --cwd {workdir}   (reuse the workspace holding it, as a new tab)"
                ),
            });
        }
        lines.push(format!(
            "herdr workspace create --label \"{label}\" --cwd {workdir} {env_args} --no-focus"
        ));
        if ctx.focus {
            lines.push("herdr workspace focus <workspace>".to_owned());
        } else {
            lines.push("(no focus change: the session opens in the background)".to_owned());
        }
        lines.push(format!(
            "herdr agent start task-{} --kind claude --pane <pane> --timeout {AGENT_TIMEOUT_MS}   \
             (retried every {} ms, up to {} s, while the pane's shell starts)",
            ctx.task.id,
            SHELL_WAIT_INTERVAL.as_millis(),
            (SHELL_WAIT_INTERVAL * SHELL_WAIT_RETRIES).as_secs()
        ));
        lines.push(format!(
            "herdr agent prompt task-{} \"<prompt below>\"",
            ctx.task.id
        ));
        lines.push(format!(
            "(if herdr cannot open a pane: {} launcher in the current pane)",
            self.fallback.name()
        ));
        lines.push(String::new());
        lines.push("--- prompt ---".to_owned());
        lines.extend((self.prompt)(ctx)?.lines().map(str::to_owned));
        Ok(lines)
    }

    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError> {
        self.check()?;
        let label = short_label(&ctx.task.title);
        let prompt = (self.prompt)(ctx)?;
        let Pane {
            workspace: ws_id,
            tab: tab_id,
            id: pane_id,
        } = match self.open_pane(ctx, &label) {
            Ok(pane) => pane,
            Err(reason) => {
                self.trace.warn(&format!(
                    "herdr could not open a pane ({reason}); opening in the current pane with {}",
                    self.fallback.name()
                ));
                return self.fallback.launch(ctx);
            }
        };
        // Switch the view as soon as the pane exists: `agent start` below
        // waits for Claude to be ready (seconds), and the user should watch
        // that happen rather than wait for it in the old workspace. Only
        // `workspace focus` switches the view, landing on the workspace's
        // active tab, hence the `tab focus` first.
        if ctx.focus {
            if let Some(tab) = &tab_id {
                self.focus(&["tab", "focus", tab]);
            }
            self.focus(&["workspace", "focus", &ws_id]);
        }
        // The pane is an interactive shell at the workdir, so its direnv
        // hook loads .envrc; warn when direnv would refuse.
        if let Some(warning) = envrc_warning(envrc_status(&ctx.workdir, &self.env), &ctx.workdir) {
            self.trace.warn(&warning);
        }
        let agent = self.start_agent(ctx, &pane_id)?;
        let pasted = self.herdr(&["agent", "prompt", &agent, &prompt]);
        if !pasted.success {
            return Err(LaunchError::Tool {
                tool: "herdr agent prompt".to_owned(),
                message: pasted.message(),
            });
        }
        if !ctx.focus {
            return Ok(LaunchOutcome::Opened(format!(
                "Opened herdr workspace {ws_id} (\"{label}\") with agent {agent} in the background"
            )));
        }
        // `agent focus` moves the server's focus to the agent that now exists.
        self.focus(&["agent", "focus", &agent]);
        Ok(LaunchOutcome::Opened(format!(
            "Opened herdr workspace {ws_id} (\"{label}\") with agent {agent}"
        )))
    }

    fn resume_hint(&self, session_id: &str) -> Option<String> {
        self.fallback.resume_hint(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_from_concatenated_json() {
        let text = "{\"ok\":true,\"workspace\":{\"workspace_id\":\"ws-1\",\"panes\":[{\"pane_id\":\"p-9\"}]}}\n{\"pane_id\":\"p-other\"}";
        assert_eq!(field(text, "workspace_id").as_deref(), Some("ws-1"));
        assert_eq!(field(text, "pane_id").as_deref(), Some("p-9"));
        assert_eq!(field(text, "tab_id"), None);
        assert_eq!(field("not json", "pane_id"), None);
        assert_eq!(field("{\"pane_id\": 3}", "pane_id"), None);
    }

    #[test]
    fn no_pane_gives_the_error_or_the_reply() {
        let failed = Finished {
            success: false,
            stdout: String::new(),
            stderr: "error: server not running\n".into(),
        };
        assert_eq!(
            no_pane("workspace create", &failed),
            "herdr workspace create failed: error: server not running"
        );
        let odd = Finished {
            success: true,
            stdout: "{\"ok\":true}\n".into(),
            stderr: String::new(),
        };
        assert_eq!(
            no_pane("tab create", &odd),
            "herdr tab create replied without the pane's ids: {\"ok\":true}"
        );
    }

    fn finished(success: bool, stdout: &str, stderr: &str) -> Finished {
        Finished {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    const BUSY: &str = "{\"error\":{\"code\":\"agent_pane_busy\",\"message\":\"agent target pane w14:p1 is not an available shell\"},\"id\":\"cli:agent:start\"}\n";

    #[test]
    fn pane_busy_is_the_error_code_on_either_stream_of_a_failure() {
        assert!(pane_busy(&finished(false, "", BUSY)));
        assert!(pane_busy(&finished(false, BUSY, "")));
        assert!(!pane_busy(&finished(true, "", BUSY)));
        assert!(!pane_busy(&finished(
            false,
            "",
            "{\"error\":{\"code\":\"agent_pane_not_found\"}}"
        )));
        assert!(!pane_busy(&finished(false, "", "agent_pane_busy")));
        assert!(!pane_busy(&finished(false, "", "")));
    }

    #[test]
    fn herdr_message_prefers_the_json_message() {
        assert_eq!(
            herdr_message(&finished(false, "", BUSY)),
            "agent target pane w14:p1 is not an available shell"
        );
        assert_eq!(
            herdr_message(&finished(
                false,
                "{\"message\":\"out\"}",
                "{\"message\":\"err\"}"
            )),
            "err"
        );
        assert_eq!(
            herdr_message(&finished(false, "{\"message\":\"out\"}", "plain\n")),
            "out"
        );
        assert_eq!(
            herdr_message(&finished(false, "{\"ok\":false}\n", " plain \n")),
            "plain"
        );
    }

    #[test]
    fn foreground_process_names_the_first_foreground_entry() {
        let info = |front: &str, shell: &str| {
            format!(
                "{{\"id\":\"cli:pane:process_info\",\"result\":{{\"process_info\":{{\"foreground_process_group_id\":42,\"foreground_processes\":[{front}],\"pane_id\":\"w14:p1\",\"shell_pid\":{shell}}},\"type\":\"pane_process_info\"}}}}\n"
            )
        };
        assert_eq!(
            foreground_process(&info(
                "{\"argv\":[\"gitstatusd\"],\"name\":\"gitstatusd\",\"pid\":42},{\"name\":\"zsh\",\"pid\":7}",
                "7"
            ))
            .as_deref(),
            Some("gitstatusd (pid 42)")
        );
        assert_eq!(
            foreground_process(&info("{\"name\":\"zsh\",\"pid\":7}", "7")).as_deref(),
            Some("zsh (pid 7, the pane's shell)")
        );
        assert_eq!(
            foreground_process(&info("{\"name\":\"zsh\",\"pid\":7}", "null")).as_deref(),
            Some("zsh (pid 7)")
        );
        assert_eq!(foreground_process(&info("", "7")), None);
        assert_eq!(foreground_process(&info("{\"name\":\"zsh\"}", "7")), None);
        assert_eq!(foreground_process(&info("{\"pid\":7}", "7")), None);
        assert_eq!(foreground_process("{\"process_info\":{}}"), None);
        assert_eq!(foreground_process("not json"), None);
    }

    #[test]
    fn holding_workspace_matches_the_path_exactly() {
        let text = "{\"worktrees\":[{\"path\":\"/a\",\"open_workspace_id\":\"ws-a\"},{\"path\":\"/a/b\",\"open_workspace_id\":\"ws-b\"},{\"path\":\"/c\",\"open_workspace_id\":\"\"}]}";
        assert_eq!(workspace_holding(text, "/a/b").as_deref(), Some("ws-b"));
        assert_eq!(workspace_holding(text, "/a").as_deref(), Some("ws-a"));
        assert_eq!(workspace_holding(text, "/c"), None);
        assert_eq!(workspace_holding(text, "/zzz"), None);
        assert_eq!(workspace_holding("", "/a"), None);
    }
}
