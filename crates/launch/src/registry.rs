//! Launcher selection by name (`launch.default`, `--launcher`).

use std::path::PathBuf;

use tasq_core::config::{EnvStrategy, Placement};
use tasq_core::launch::{LaunchError, Launcher};

use crate::claude::ClaudeLauncher;
use crate::process::env_var;
use crate::shell::ShellLauncher;
use crate::tmux::TmuxLauncher;

/// Names [`launcher_for`] accepts. `auto` is `herdr` inside herdr
/// (`HERDR_ENV` set), else `claude`.
pub const LAUNCHER_NAMES: &[&str] = &["auto", "claude", "shell", "tmux", "herdr"];

/// Names `launch.detached` accepts: the launchers that open another window.
/// `auto` is `herdr` inside herdr, `tmux` inside tmux, and an error elsewhere.
pub const DETACHED_LAUNCHER_NAMES: &[&str] = &["auto", "tmux", "herdr"];

/// What every launcher is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSettings {
    /// The process environment the session inherits.
    pub env: Vec<(String, String)>,
    /// `launch.env`.
    pub strategy: EnvStrategy,
    /// The Claude prompt template text.
    pub template: String,
    /// `work.default_project`.
    pub default_project: Option<PathBuf>,
    /// `launch.herdr.placement`.
    pub placement: Placement,
}

/// Resolves `auto` against the environment; other names pass through.
pub fn resolve_name<'a>(name: &'a str, env: &[(String, String)]) -> &'a str {
    if name == "auto" {
        if env_var(env, "HERDR_ENV").is_some() {
            "herdr"
        } else {
            "claude"
        }
    } else {
        name
    }
}

/// Resolves a `launch.detached` name: `auto` becomes the window manager
/// the current terminal runs in, other names pass through when they open
/// a window. A launcher that takes over the current terminal (`claude`,
/// `shell`) is refused, since a detached launch exists to not do that.
pub fn resolve_detached<'a>(
    name: &'a str,
    env: &[(String, String)],
) -> Result<&'a str, LaunchError> {
    match name {
        "auto" => {
            if env_var(env, "HERDR_ENV").is_some() {
                Ok("herdr")
            } else if env_var(env, "TMUX").is_some() {
                Ok("tmux")
            } else {
                Err(LaunchError::Unavailable {
                    launcher: "auto".to_owned(),
                    reason: "no window to open a session in: not inside herdr or tmux \
                             (set launch.detached)"
                        .to_owned(),
                })
            }
        }
        "tmux" | "herdr" => Ok(name),
        other => Err(LaunchError::Unavailable {
            launcher: other.to_owned(),
            reason: format!(
                "not a detached launcher (available: {})",
                DETACHED_LAUNCHER_NAMES.join(", ")
            ),
        }),
    }
}

/// The launcher called `name`.
pub fn launcher_for(
    name: &str,
    settings: &LaunchSettings,
) -> Result<Box<dyn Launcher>, LaunchError> {
    let claude = || ClaudeLauncher {
        env: settings.env.clone(),
        strategy: settings.strategy,
        template: settings.template.clone(),
        in_herdr: env_var(&settings.env, "HERDR_ENV").is_some(),
    };
    match resolve_name(name, &settings.env) {
        "claude" => Ok(Box::new(claude())),
        "shell" => Ok(Box::new(ShellLauncher {
            env: settings.env.clone(),
        })),
        "tmux" => Ok(Box::new(TmuxLauncher {
            env: settings.env.clone(),
        })),
        "herdr" => herdr(settings, Box::new(claude())),
        other => Err(LaunchError::Unavailable {
            launcher: other.to_owned(),
            reason: format!(
                "unknown launcher (available: {})",
                LAUNCHER_NAMES.join(", ")
            ),
        }),
    }
}

// The `Result` is shared with the feature-off variant below, which fails.
#[allow(clippy::unnecessary_wraps)]
#[cfg(feature = "herdr")]
fn herdr(
    settings: &LaunchSettings,
    fallback: Box<ClaudeLauncher>,
) -> Result<Box<dyn Launcher>, LaunchError> {
    // Inside herdr (the only place this launcher runs) the Claude launcher
    // already renders the prompt with the workspace-renaming instruction.
    let prompt_source = (*fallback).clone();
    Ok(Box::new(crate::herdr::HerdrLauncher {
        env: settings.env.clone(),
        default_project: settings.default_project.clone(),
        placement: settings.placement,
        fallback,
        prompt: Box::new(move |ctx| prompt_source.prompt(ctx)),
    }))
}

/// Reason: only compiled without the `herdr` feature, which the test
/// suite (default features) never builds; nothing can observe a mutation.
#[mutants::skip]
#[cfg(not(feature = "herdr"))]
fn herdr(
    _settings: &LaunchSettings,
    _fallback: Box<ClaudeLauncher>,
) -> Result<Box<dyn Launcher>, LaunchError> {
    Err(LaunchError::Unavailable {
        launcher: "herdr".to_owned(),
        reason: "this build of tasq has no herdr support (feature `herdr`)".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(env: &[(&str, &str)]) -> LaunchSettings {
        LaunchSettings {
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            strategy: EnvStrategy::Inherit,
            template: "{{id}}".to_owned(),
            default_project: None,
            placement: Placement::Auto,
        }
    }

    #[test]
    fn detached_follows_the_window_manager_and_refuses_in_pane_launchers() {
        let herdr = settings(&[("HERDR_ENV", "1"), ("TMUX", "/tmp/tmux-1/default,1,0")]);
        let tmux = settings(&[("TMUX", "/tmp/tmux-1/default,1,0")]);
        let plain = settings(&[]);
        assert_eq!(resolve_detached("auto", &herdr.env), Ok("herdr"));
        assert_eq!(resolve_detached("auto", &tmux.env), Ok("tmux"));
        assert_eq!(
            resolve_detached("auto", &plain.env)
                .unwrap_err()
                .to_string(),
            "auto: no window to open a session in: not inside herdr or tmux (set launch.detached)"
        );
        assert_eq!(resolve_detached("tmux", &plain.env), Ok("tmux"));
        assert_eq!(resolve_detached("herdr", &plain.env), Ok("herdr"));
        for name in DETACHED_LAUNCHER_NAMES {
            assert!(LAUNCHER_NAMES.contains(name), "{name}");
        }
        assert_eq!(
            resolve_detached("claude", &herdr.env)
                .unwrap_err()
                .to_string(),
            "claude: not a detached launcher (available: auto, tmux, herdr)"
        );
        assert_eq!(
            resolve_detached("shell", &herdr.env)
                .unwrap_err()
                .to_string(),
            "shell: not a detached launcher (available: auto, tmux, herdr)"
        );
    }

    #[test]
    fn auto_follows_herdr_env() {
        let inside = settings(&[("HERDR_ENV", "1")]);
        let outside = settings(&[]);
        assert_eq!(resolve_name("auto", &inside.env), "herdr");
        assert_eq!(resolve_name("auto", &outside.env), "claude");
        assert_eq!(resolve_name("shell", &inside.env), "shell");
        assert_eq!(launcher_for("auto", &outside).unwrap().name(), "claude");
        assert_eq!(launcher_for("auto", &inside).unwrap().name(), "herdr");
    }

    #[test]
    fn every_name_resolves_and_unknown_names_do_not() {
        let s = settings(&[]);
        for name in LAUNCHER_NAMES {
            assert!(launcher_for(name, &s).is_ok(), "{name}");
        }
        assert_eq!(launcher_for("shell", &s).unwrap().name(), "shell");
        assert_eq!(launcher_for("tmux", &s).unwrap().name(), "tmux");
        let err = launcher_for("nope", &s)
            .map(|l| l.name().to_owned())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "nope: unknown launcher (available: auto, claude, shell, tmux, herdr)"
        );
    }
}
