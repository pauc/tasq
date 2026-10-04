//! `tasq doctor`: configuration, store and optional-tool diagnostics.
//!
//! The store checks come from `tasq_store_nb::doctor::checks`; this module
//! adds the `config` check (the layers that loaded, or why loading failed)
//! and one check per optional tool. Any `FAIL` makes the exit code 1.

use std::fmt::Write as _;

use serde_json::Value;
use tasq_core::config::{Config, LoadOptions, UiConfig};
use tasq_store_nb::doctor::{self, Check, CheckStatus, any_failed, tool_check};

use crate::app::{App, env_vec};
use crate::cli::GlobalArgs;
use crate::error::{CliError, Result};
use crate::json;
use crate::output::{Color, Output, Style};

/// Optional tools and what each one is for.
pub const TOOLS: &[(&str, &str)] = &[
    (
        "git",
        "install git; the native bookkeeper needs it to commit the notebook",
    ),
    (
        "glow",
        "install glow (https://github.com/charmbracelet/glow) for rendered `tasq view`",
    ),
    (
        "claude",
        "install Claude Code for the claude launcher and the llm summarizer",
    ),
    (
        "direnv",
        "install direnv (https://direnv.net), or set launch.env = \"inherit\"",
    ),
    ("gwm", "install gwm, or set work.worktree_manager = \"git\""),
    (
        "herdr",
        "install herdr to open sessions in herdr workspaces",
    ),
    (
        "glab",
        "install glab (https://gitlab.com/gitlab-org/cli) to resolve GitLab merge request titles",
    ),
    (
        "gh",
        "install gh (https://cli.github.com) to resolve GitHub pull request titles",
    ),
];

/// Runs `doctor`. Takes the raw options because a broken config is one of
/// the things it diagnoses.
pub fn run(global: GlobalArgs, opts: LoadOptions) -> Result<()> {
    let mut checks = Vec::new();
    let env = env_vec(&opts.env);
    let loaded = match Config::load(&opts) {
        Ok(loaded) => {
            checks.push(Check::ok("config", describe_layers(&loaded)));
            Some(loaded)
        }
        Err(e) => {
            checks.push(Check::fail(
                "config",
                e.to_string(),
                "fix the file named in the message (tasq config show prints the layers)",
            ));
            None
        }
    };
    let ui = loaded
        .as_ref()
        .map_or_else(UiConfig::default, |l| l.config.ui.clone());
    let out = Output::from_process(&global, &ui, &opts.env);
    if let Some(loaded) = loaded {
        let app = App::new(global, opts, loaded, out.clone());
        match app.open_store() {
            Ok(store) => checks.extend(doctor::checks(&store)),
            Err(e) => checks.push(Check::fail(
                "store",
                e.to_string(),
                "point store.notebook at an existing notebook (tasq config show)",
            )),
        }
    }
    for (tool, fix) in TOOLS {
        checks.push(tool_check(tool, &env, fix));
    }
    let failed = any_failed(&checks);
    if out.json_mode() {
        out.json(&json::document([
            ("checks", json::to_value(&checks)),
            ("ok", Value::from(!failed)),
        ]))?;
    } else {
        out.page(&render(&checks, out.style()))?;
    }
    if failed {
        Err(CliError::Silent(1))
    } else {
        Ok(())
    }
}

fn describe_layers(loaded: &tasq_core::config::Loaded) -> String {
    let layers: Vec<String> = loaded.layers.iter().map(|l| l.origin.to_string()).collect();
    format!("loaded from {}", layers.join(", "))
}

/// The check list: `OK   name  detail`, with `fix:` lines under warnings
/// and failures.
pub fn render(checks: &[Check], style: Style) -> String {
    let mut out = String::new();
    for check in checks {
        let verdict = match check.status {
            CheckStatus::Ok => style.color(Color::Green, "OK  "),
            CheckStatus::Warn => style.color(Color::Yellow, "WARN"),
            CheckStatus::Fail => style.bold_color(Color::Red, "FAIL"),
        };
        let _ = writeln!(out, "{verdict}  {:<13}{}", check.name, check.detail);
        if let Some(fix) = &check.fix {
            let _ = writeln!(out, "{:<19}{}", "", style.dim(&format!("fix: {fix}")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lines() {
        let checks = vec![
            Check::ok("config", "loaded from defaults"),
            Check::warn("glow", "glow is not on PATH", "install glow"),
            Check::fail("index", "missing", "run reconcile"),
        ];
        assert_eq!(
            render(&checks, Style::OFF),
            "OK    config       loaded from defaults\n\
             WARN  glow         glow is not on PATH\n\
             \x20                  fix: install glow\n\
             FAIL  index        missing\n\
             \x20                  fix: run reconcile\n"
        );
        let colored = render(&checks[..1], Style::ON);
        assert!(colored.starts_with("\x1b[32mOK  \x1b[0m  config"));
    }
}
