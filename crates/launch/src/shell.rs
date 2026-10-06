//! The shell launcher: `exec $SHELL` in the working directory.

use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher};

use crate::process::{env_var, exec};

/// Replaces `tasq` with the user's shell in the task's directory, with the
/// context variables exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLauncher {
    /// Environment of the session (and where `SHELL` is read from).
    pub env: Vec<(String, String)>,
}

impl ShellLauncher {
    /// `$SHELL`, else `sh`.
    pub fn shell(&self) -> String {
        env_var(&self.env, "SHELL").unwrap_or("sh").to_owned()
    }
}

/// `K=V K=V` for a dry-run line.
pub fn assignments(env: &[(String, String)]) -> String {
    env.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Launcher for ShellLauncher {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn describe(&self, ctx: &LaunchContext) -> Result<Vec<String>, LaunchError> {
        Ok(vec![
            format!("cd {}", ctx.workdir.display()),
            format!("{} exec {}", assignments(&ctx.env), self.shell()),
        ])
    }

    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError> {
        let shell = self.shell();
        let err = exec(
            std::slice::from_ref(&shell),
            &ctx.workdir,
            &self.env,
            &ctx.env,
        );
        Err(LaunchError::Tool {
            tool: shell,
            message: err.to_string(),
        })
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
            task: Task::new(TaskId::from(3), "T"),
            file: "/nb/3.todo.md".into(),
            markdown: "# [ ] T\n".into(),
            workdir: "/work".into(),
            in_worktree: false,
            env: vec![("TASQ_TASK_ID".into(), "3".into())],
            statuses: vec!["ready".into()],
            focus: true,
        }
    }

    #[test]
    fn describe_names_shell_and_directory() {
        let launcher = ShellLauncher {
            env: vec![("SHELL".into(), "/bin/zsh".into())],
        };
        assert_eq!(launcher.name(), "shell");
        assert_eq!(
            launcher.describe(&ctx()).unwrap(),
            vec!["cd /work", "TASQ_TASK_ID=3 exec /bin/zsh"]
        );
        assert_eq!(launcher.resume_hint("x"), None);
        let plain = ShellLauncher { env: Vec::new() };
        assert_eq!(plain.shell(), "sh");
        assert_eq!(assignments(&[]), "");
    }
}
