//! `tasq completions <shell>`.

use std::io;

use clap_complete::Shell;

use crate::app::worktrees_enabled;
use crate::cli::{self, GlobalArgs};
use crate::error::Result;
use crate::output::write_all;

/// Prints the completion script for `shell`, leaving out the `worktree`
/// command unless the configuration `global` selects enables worktrees.
pub fn run(shell: Shell, global: &GlobalArgs) -> Result<()> {
    let mut command = cli::completion_command(worktrees_enabled(global));
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut command, "tasq", &mut script);
    write_all(&mut io::stdout().lock(), &script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shell_generates_a_script_naming_the_binary() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let mut command = cli::completion_command(true);
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut command, "tasq", &mut script);
            let text = String::from_utf8(script).unwrap();
            assert!(text.contains("tasq"), "{shell}: {text}");
            assert!(text.contains("doctor"), "{shell} lacks subcommands");
        }
    }

    #[test]
    fn worktree_subcommands_complete_only_when_enabled() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let script = |enabled: bool| {
                let mut command = cli::completion_command(enabled);
                let mut script = Vec::new();
                clap_complete::generate(shell, &mut command, "tasq", &mut script);
                String::from_utf8(script).unwrap()
            };
            let on = script(true);
            for word in ["shell-init", "switch", "release", "ls"] {
                assert!(on.contains(word), "{shell} lacks {word}");
            }
            assert!(on.contains("fish"), "{shell} lacks the shell-init values");
            assert!(!script(false).contains("shell-init"), "{shell}");
        }
    }

    #[test]
    fn subcommands_complete_as_the_first_word() {
        for enabled in [true, false] {
            let mut command = cli::completion_command(enabled);
            let mut script = Vec::new();
            clap_complete::generate(Shell::Zsh, &mut command, "tasq", &mut script);
            let text = String::from_utf8(script).unwrap();
            // Every `_arguments` call that offers subcommands, with the
            // positional specs ahead of that slot: any of them would take
            // the first word, and the subcommands would never complete.
            let mut slots = Vec::new();
            let mut positionals = Vec::new();
            for call in text.split("_arguments \"${_arguments_options[@]}\" :") {
                let Some(slot) = call.find("\":: :_tasq") else {
                    continue;
                };
                slots.extend(call[slot..].split('"').nth(1));
                positionals.extend(
                    call[..slot]
                        .lines()
                        .filter(|l| l.starts_with("':") || l.starts_with("'::")),
                );
            }
            assert_eq!(positionals, Vec::<&str>::new(), "worktrees {enabled}");
            assert!(slots.contains(&":: :_tasq_commands"), "{slots:?}");
            assert_eq!(
                slots.contains(&":: :_tasq__subcmd__worktree_commands"),
                enabled,
                "{slots:?}"
            );
        }
    }
}
