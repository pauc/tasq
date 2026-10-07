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
}
