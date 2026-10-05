//! The Claude Code launcher: `claude "<prompt>"` in the working directory,
//! through `direnv exec` when the strategy and the `.envrc` allow it.

use std::path::Path;

use tasq_core::config::EnvStrategy;
use tasq_core::launch::{LaunchContext, LaunchError, LaunchOutcome, Launcher};

use crate::env::{envrc_status, envrc_warning, wrap_command};
use crate::process::exec;
use crate::prompt::{render, variables};
use crate::shell::assignments;

/// Builds the task prompt from the template and replaces `tasq` with
/// `claude` (plan T-404). The task is marked in-progress by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeLauncher {
    /// Environment the session inherits (and `PATH` for `direnv`).
    pub env: Vec<(String, String)>,
    /// `launch.env`.
    pub strategy: EnvStrategy,
    /// The prompt template (built-in or `launch.claude.prompt_file`).
    pub template: String,
    /// Whether to add the herdr workspace-renaming instruction.
    pub in_herdr: bool,
}

impl ClaudeLauncher {
    /// The rendered prompt.
    pub fn prompt(&self, ctx: &LaunchContext) -> Result<String, LaunchError> {
        render(&self.template, &variables(ctx, self.in_herdr))
    }

    /// The command to run and, when the directory's `.envrc` is not
    /// allowed, the warning to show first.
    pub fn command(
        &self,
        ctx: &LaunchContext,
    ) -> Result<(Vec<String>, Option<String>), LaunchError> {
        let prompt = self.prompt(ctx)?;
        Ok(command_in(&ctx.workdir, prompt, self.strategy, &self.env))
    }
}

/// `claude "<prompt>"` in `workdir`, through `direnv exec` when the
/// strategy and the directory's `.envrc` allow it, plus the warning to show
/// first when the `.envrc` is not allowed. This is the whole of a Claude
/// session that is not about one task (`tasq sync --interactive`); the
/// launcher adds the task prompt on top.
pub fn command_in(
    workdir: &Path,
    prompt: String,
    strategy: EnvStrategy,
    env: &[(String, String)],
) -> (Vec<String>, Option<String>) {
    let status = envrc_status(workdir, env);
    let argv = wrap_command(strategy, status, workdir, vec!["claude".to_owned(), prompt]);
    (argv, envrc_warning(status, workdir))
}

impl Launcher for ClaudeLauncher {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn describe(&self, ctx: &LaunchContext) -> Result<Vec<String>, LaunchError> {
        let (argv, warning) = self.command(ctx)?;
        let prompt = argv.last().cloned().unwrap_or_default();
        let shown: Vec<String> = argv[..argv.len() - 1].to_vec();
        let mut lines = Vec::new();
        if let Some(warning) = warning {
            lines.push(format!("warning: {warning}"));
        }
        lines.push(format!("cd {}", ctx.workdir.display()));
        lines.push(format!(
            "{} exec {} \"<prompt below>\"",
            assignments(&ctx.env),
            shown.join(" ")
        ));
        lines.push(String::new());
        lines.push("--- prompt ---".to_owned());
        lines.extend(prompt.lines().map(str::to_owned));
        Ok(lines)
    }

    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError> {
        let (argv, warning) = self.command(ctx)?;
        if let Some(warning) = warning {
            eprintln!("tasq: warning: {warning}");
        }
        let err = exec(&argv, &ctx.workdir, &self.env, &ctx.env);
        Err(LaunchError::Tool {
            tool: argv[0].clone(),
            message: err.to_string(),
        })
    }

    fn resume_hint(&self, session_id: &str) -> Option<String> {
        Some(format!("claude --resume {session_id}"))
    }
}
