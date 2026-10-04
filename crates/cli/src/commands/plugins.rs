//! `tasq plugins list`: the `tasq-<name>` executables on `PATH` and the
//! hooks configured under `[hooks]`.

use std::fmt::Write;

use tasq_core::config::HooksConfig;

use crate::app::App;
use crate::cli::PluginsCommand;
use crate::error::Result;
use crate::json;
use crate::plugins::{Plugin, discover};

/// Runs a `plugins` subcommand.
pub fn run(app: &App, command: PluginsCommand) -> Result<()> {
    match command {
        PluginsCommand::List => {
            let path = app.opts.env.get("PATH").map(std::ffi::OsString::from);
            let plugins = discover(path.as_deref());
            let hooks = &app.config().hooks;
            if app.out.json_mode() {
                return app.out.json(&json::document([
                    ("plugins", json::to_value(&plugins)),
                    ("hooks", json::to_value(hooks)),
                ]));
            }
            app.out.print(&render(&plugins, hooks))
        }
    }
}

/// The `plugins list` text.
pub fn render(plugins: &[Plugin], hooks: &HooksConfig) -> String {
    let mut text = String::from("Plugins on PATH (tasq-<name>):\n");
    if plugins.is_empty() {
        text.push_str("  (none)\n");
    } else {
        let width = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0);
        for plugin in plugins {
            let _ = writeln!(text, "  {:width$}  {}", plugin.name, plugin.path.display());
        }
    }
    text.push_str("\nHooks ([hooks] in the config):\n");
    if hooks.is_empty() {
        text.push_str("  (none)\n");
    } else {
        for (name, commands) in hooks.entries() {
            for command in commands {
                let _ = writeln!(text, "  {name:11}  {command}");
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn render_lists_plugins_and_hooks() {
        let plugins = vec![
            Plugin {
                name: "tlogs".to_owned(),
                path: PathBuf::from("/home/me/bin/tasq-tlogs"),
            },
            Plugin {
                name: "x".to_owned(),
                path: PathBuf::from("/usr/local/bin/tasq-x"),
            },
        ];
        let hooks = HooksConfig {
            post_create: vec!["tasq-notify".to_owned()],
            post_done: Vec::new(),
            pre_launch: vec!["check-vpn --quiet".to_owned()],
        };
        assert_eq!(
            render(&plugins, &hooks),
            "Plugins on PATH (tasq-<name>):\n  \
               tlogs  /home/me/bin/tasq-tlogs\n  \
               x      /usr/local/bin/tasq-x\n\
             \nHooks ([hooks] in the config):\n  \
               post-create  tasq-notify\n  \
               pre-launch   check-vpn --quiet\n"
        );
        assert_eq!(
            render(&[], &HooksConfig::default()),
            "Plugins on PATH (tasq-<name>):\n  (none)\n\nHooks ([hooks] in the config):\n  (none)\n"
        );
    }
}
