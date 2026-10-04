//! The herdr launcher (plan T-405): a workspace (or a tab in the workspace
//! already holding the directory, as `launch.herdr.placement` says), a
//! Claude agent started in its pane, the prompt pasted in, focus moved
//! there unless the context asks otherwise. Falls back to another launcher
//! in the current pane when herdr cannot create the pane.

use std::fmt::{self, Debug};
use std::path::PathBuf;

use serde_json::Value;
use tasq_core::config::Placement;
use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher, short_label};

use crate::env::{envrc_status, envrc_warning};
use crate::process::{env_var, run};

/// Environment variable herdr sets inside its panes.
pub const ENV_HERDR: &str = "HERDR_ENV";
/// Environment variable naming the workspace of the current pane; where a
/// `tab` placement lands when no workspace holds the task's directory.
pub const ENV_HERDR_WORKSPACE: &str = "HERDR_WORKSPACE_ID";
/// Readiness timeout passed to `herdr agent start`, in milliseconds.
pub const AGENT_TIMEOUT_MS: &str = "90000";

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
}

impl Debug for HerdrLauncher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HerdrLauncher")
            .field("default_project", &self.default_project)
            .field("placement", &self.placement)
            .field("fallback", &self.fallback.name())
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

    fn herdr(&self, args: &[&str]) -> crate::process::Finished {
        run("herdr", args, None, &self.env, &[])
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

    /// Finds or creates the pane: `(workspace_id, tab_id, pane_id)`.
    fn open_pane(
        &self,
        ctx: &LaunchContext,
        label: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
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
                (
                    field(&out.stdout, "workspace_id"),
                    None,
                    field(&out.stdout, "pane_id"),
                )
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
                (
                    Some(ws.clone()),
                    field(&out.stdout, "tab_id"),
                    field(&out.stdout, "pane_id"),
                )
            }
        }
    }

    fn start_agent(&self, ctx: &LaunchContext, pane: &str) -> Result<String, LaunchError> {
        let first = format!("task-{}", ctx.task.id);
        let second = format!("task-{}-{}", ctx.task.id, std::process::id());
        let mut last = None;
        for name in [&first, &second] {
            let out = self.herdr(&[
                "agent",
                "start",
                name,
                "--kind",
                "claude",
                "--pane",
                pane,
                "--timeout",
                AGENT_TIMEOUT_MS,
            ]);
            if out.success {
                return Ok(name.clone());
            }
            last = Some(out.message());
        }
        Err(LaunchError::Tool {
            tool: "herdr agent start".to_owned(),
            message: last.unwrap_or_default(),
        })
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
            "herdr agent start task-{} --kind claude --pane <pane> --timeout {AGENT_TIMEOUT_MS}",
            ctx.task.id
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
        let (ws_id, tab_id, pane_id) = self.open_pane(ctx, &label);
        let (Some(ws_id), Some(pane_id)) = (ws_id, pane_id) else {
            eprintln!(
                "tasq: warning: herdr workspace creation failed; opening in the current pane"
            );
            return self.fallback.launch(ctx);
        };
        // Switch the view as soon as the pane exists: `agent start` below
        // waits for Claude to be ready (seconds), and the user should watch
        // that happen rather than wait for it in the old workspace. Only
        // `workspace focus` switches the view, landing on the workspace's
        // active tab, hence the `tab focus` first.
        if ctx.focus {
            if let Some(tab) = &tab_id {
                self.herdr(&["tab", "focus", tab]);
            }
            self.herdr(&["workspace", "focus", &ws_id]);
        }
        // The pane is an interactive shell at the workdir, so its direnv
        // hook loads .envrc; warn when direnv would refuse.
        if let Some(warning) = envrc_warning(envrc_status(&ctx.workdir, &self.env), &ctx.workdir) {
            eprintln!("tasq: warning: {warning}");
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
        self.herdr(&["agent", "focus", &agent]);
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
    fn holding_workspace_matches_the_path_exactly() {
        let text = "{\"worktrees\":[{\"path\":\"/a\",\"open_workspace_id\":\"ws-a\"},{\"path\":\"/a/b\",\"open_workspace_id\":\"ws-b\"},{\"path\":\"/c\",\"open_workspace_id\":\"\"}]}";
        assert_eq!(workspace_holding(text, "/a/b").as_deref(), Some("ws-b"));
        assert_eq!(workspace_holding(text, "/a").as_deref(), Some("ws-a"));
        assert_eq!(workspace_holding(text, "/c"), None);
        assert_eq!(workspace_holding(text, "/zzz"), None);
        assert_eq!(workspace_holding("", "/a"), None);
    }
}
