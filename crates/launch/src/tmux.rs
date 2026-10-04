//! The tmux launcher: a new window in the task's directory.

use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher, short_label};

use crate::process::{env_var, run};

/// Opens `tmux new-window` at the working directory with the context
/// variables set (`-e`, tmux 3.0 or later). Only works inside tmux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxLauncher {
    /// Environment `tmux` runs with and `TMUX` is read from.
    pub env: Vec<(String, String)>,
}

impl TmuxLauncher {
    fn check(&self) -> Result<(), LaunchError> {
        if env_var(&self.env, "TMUX").is_some() {
            Ok(())
        } else {
            Err(LaunchError::Unavailable {
                launcher: "tmux".to_owned(),
                reason: "not inside a tmux session ($TMUX is unset)".to_owned(),
            })
        }
    }

    /// The `tmux` argv for `ctx`.
    pub fn argv(ctx: &LaunchContext) -> Vec<String> {
        let mut argv = vec![
            "tmux".to_owned(),
            "new-window".to_owned(),
            "-c".to_owned(),
            ctx.workdir.display().to_string(),
            "-n".to_owned(),
            window_name(ctx),
        ];
        for (k, v) in &ctx.env {
            argv.push("-e".to_owned());
            argv.push(format!("{k}={v}"));
        }
        argv
    }
}

/// `<id> <short label>`, or just the id for an untitled task.
pub fn window_name(ctx: &LaunchContext) -> String {
    let label = short_label(&ctx.task.title);
    if label.is_empty() {
        ctx.task.id.to_string()
    } else {
        format!("{} {label}", ctx.task.id)
    }
}

impl Launcher for TmuxLauncher {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn describe(&self, ctx: &LaunchContext) -> Result<Vec<String>, LaunchError> {
        self.check()?;
        Ok(vec![Self::argv(ctx).join(" ")])
    }

    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError> {
        self.check()?;
        let argv = Self::argv(ctx);
        let rest: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        let out = run("tmux", &rest, None, &self.env, &[]);
        if out.success {
            Ok(LaunchOutcome::Opened(format!(
                "opened tmux window \"{}\" in {}",
                window_name(ctx),
                ctx.workdir.display()
            )))
        } else {
            Err(LaunchError::Tool {
                tool: "tmux".to_owned(),
                message: out.message(),
            })
        }
    }

    fn resume_hint(&self, _session_id: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::model::{Task, TaskId};

    fn ctx() -> LaunchContext {
        LaunchContext {
            task: Task::new(TaskId::from(3), "Fix the login form"),
            file: "/nb/3.todo.md".into(),
            markdown: String::new(),
            workdir: "/work".into(),
            in_worktree: false,
            env: vec![("TASQ_TASK_ID".into(), "3".into())],
            statuses: Vec::new(),
        }
    }

    #[test]
    fn needs_tmux() {
        let outside = TmuxLauncher { env: Vec::new() };
        assert_eq!(
            outside.describe(&ctx()).unwrap_err(),
            LaunchError::Unavailable {
                launcher: "tmux".into(),
                reason: "not inside a tmux session ($TMUX is unset)".into()
            }
        );
        assert!(matches!(
            outside.launch(&ctx()).unwrap_err(),
            LaunchError::Unavailable { .. }
        ));
        assert_eq!(outside.name(), "tmux");
        assert_eq!(outside.resume_hint("x"), None);
    }

    #[test]
    fn argv_and_window_name() {
        let inside = TmuxLauncher {
            env: vec![("TMUX".into(), "/tmp/tmux-1/default,1,0".into())],
        };
        assert_eq!(
            inside.describe(&ctx()).unwrap(),
            vec!["tmux new-window -c /work -n 3 Fix the login -e TASQ_TASK_ID=3"]
        );
        let mut untitled = ctx();
        untitled.task.title = "[x]".into();
        assert_eq!(window_name(&untitled), "3");
    }
}
