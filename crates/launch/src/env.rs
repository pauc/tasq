//! Environment strategy (plan T-402): give the session the working
//! directory's own environment through `direnv exec`, or inherit ours.
//!
//! The script's reasoning: Claude inherits the environment of the shell
//! that ran `tasks`, so a session opened in another project keeps the
//! launching repo's `./bin` on `PATH` and its `BUNDLE_GEMFILE`. Running the
//! session through `direnv exec` reverts the loaded `.envrc` and loads the
//! target directory's.

use std::path::Path;

use tasq_core::config::EnvStrategy;

use crate::process::{run, which};

/// What direnv says about a directory's `.envrc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvrcStatus {
    /// `direnv` is not on `PATH`.
    NoDirenv,
    /// The directory has no `.envrc`.
    NoEnvrc,
    /// `.envrc` exists and direnv allows it.
    Allowed,
    /// `.envrc` exists but was not allowed (or direnv could not say).
    NotAllowed,
}

/// Reads `Found RC allowed <value>` from `direnv status` output (the first
/// such line); `true` and `0` both mean allowed, as the script accepted.
pub fn parse_direnv_status(text: &str) -> Option<bool> {
    text.lines()
        .find_map(|line| line.strip_prefix("Found RC allowed"))
        .map(|rest| matches!(rest.trim(), "true" | "0"))
}

/// Asks direnv about `dir` (running `direnv status` inside it).
pub fn envrc_status(dir: &Path, env: &[(String, String)]) -> EnvrcStatus {
    if which(env, "direnv").is_none() {
        return EnvrcStatus::NoDirenv;
    }
    if !dir.join(".envrc").is_file() {
        return EnvrcStatus::NoEnvrc;
    }
    let out = run("direnv", &["status"], Some(dir), env, &[]);
    if out.success && parse_direnv_status(&out.stdout) == Some(true) {
        EnvrcStatus::Allowed
    } else {
        EnvrcStatus::NotAllowed
    }
}

/// The warning for a `.envrc` direnv refuses to load, which would otherwise
/// leave the session silently without the project environment.
pub fn envrc_warning(status: EnvrcStatus, dir: &Path) -> Option<String> {
    (status == EnvrcStatus::NotAllowed).then(|| {
        format!(
            "{}/.envrc is not allowed by direnv; run: direnv allow {}",
            dir.display(),
            dir.display()
        )
    })
}

/// `argv` wrapped as `direnv exec <dir> argv...` when the strategy is
/// `direnv` and the `.envrc` is allowed; unchanged otherwise.
pub fn wrap_command(
    strategy: EnvStrategy,
    status: EnvrcStatus,
    dir: &Path,
    argv: Vec<String>,
) -> Vec<String> {
    if strategy == EnvStrategy::Direnv && status == EnvrcStatus::Allowed {
        let mut wrapped = vec![
            "direnv".to_owned(),
            "exec".to_owned(),
            dir.display().to_string(),
        ];
        wrapped.extend(argv);
        wrapped
    } else {
        argv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "direnv exec path /usr/bin/direnv\nLoaded RC path /other/.envrc\nLoaded RC allowed false\nFound RC path /proj/.envrc\nFound watch: \".envrc\"\nFound RC allowed true\nFound RC allowPath /x\n";

    #[test]
    fn parses_the_found_rc_line_only() {
        assert_eq!(parse_direnv_status(STATUS), Some(true));
        assert_eq!(
            parse_direnv_status(&STATUS.replace("Found RC allowed true", "Found RC allowed false")),
            Some(false)
        );
        assert_eq!(
            parse_direnv_status(&STATUS.replace("Found RC allowed true", "Found RC allowed 0")),
            Some(true)
        );
        assert_eq!(parse_direnv_status("Loaded RC allowed true\n"), None);
        assert_eq!(parse_direnv_status(""), None);
    }

    #[test]
    fn warning_only_when_not_allowed() {
        let dir = Path::new("/proj");
        assert_eq!(
            envrc_warning(EnvrcStatus::NotAllowed, dir).as_deref(),
            Some("/proj/.envrc is not allowed by direnv; run: direnv allow /proj")
        );
        for status in [
            EnvrcStatus::Allowed,
            EnvrcStatus::NoEnvrc,
            EnvrcStatus::NoDirenv,
        ] {
            assert_eq!(envrc_warning(status, dir), None);
        }
    }

    #[test]
    fn wrapping() {
        let argv = vec!["claude".to_owned(), "hi".to_owned()];
        assert_eq!(
            wrap_command(
                EnvStrategy::Direnv,
                EnvrcStatus::Allowed,
                Path::new("/p"),
                argv.clone()
            ),
            vec!["direnv", "exec", "/p", "claude", "hi"]
        );
        for status in [
            EnvrcStatus::NotAllowed,
            EnvrcStatus::NoEnvrc,
            EnvrcStatus::NoDirenv,
        ] {
            assert_eq!(
                wrap_command(EnvStrategy::Direnv, status, Path::new("/p"), argv.clone()),
                argv
            );
        }
        assert_eq!(
            wrap_command(
                EnvStrategy::Inherit,
                EnvrcStatus::Allowed,
                Path::new("/p"),
                argv.clone()
            ),
            argv
        );
    }
}
