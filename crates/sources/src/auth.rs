//! Tokens for forge APIs: `token_cmd` (`glab auth token`, `gh auth token`)
//! or the `GITLAB_TOKEN` / `GITHUB_TOKEN` environment variables. Tokens
//! never appear in messages.

use std::process::Command;

use tasq_core::config::{ForgeConfig, ForgeKind};
use tasq_core::source::SourceError;

/// The environment variable a forge kind reads when `token_cmd` is unset.
pub fn token_env_var(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Gitlab => "GITLAB_TOKEN",
        ForgeKind::Github => "GITHUB_TOKEN",
    }
}

/// The token for `[forge.<name>]`: the trimmed output of `token_cmd`, else
/// the kind's environment variable.
pub fn token(
    name: &str,
    forge: &ForgeConfig,
    env: &[(String, String)],
) -> Result<String, SourceError> {
    let kind = forge.kind.unwrap_or(ForgeKind::Gitlab);
    if let Some(cmd) = forge
        .token_cmd
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let argv = shell_words::split(cmd)
            .map_err(|e| SourceError::Auth(format!("forge.{name}.token_cmd {cmd:?}: {e}")))?;
        let output = run(&argv, env).map_err(|e| {
            SourceError::Auth(format!("forge.{name}.token_cmd {cmd:?} failed: {e}"))
        })?;
        return non_empty(&output).ok_or_else(|| {
            SourceError::Auth(format!("forge.{name}.token_cmd {cmd:?} printed nothing"))
        });
    }
    let var = token_env_var(kind);
    env.iter()
        .find(|(k, _)| k == var)
        .and_then(|(_, v)| non_empty(v))
        .ok_or_else(|| {
            SourceError::Auth(format!(
                "no token for forge.{name}: set forge.{name}.token_cmd or the {var} environment variable"
            ))
        })
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Runs `argv` with `env` and returns its stdout; the error text carries
/// stderr but never stdout (which may hold the token).
///
/// Reason: process I/O; the surrounding logic is tested with a fake command.
#[mutants::skip]
fn run(argv: &[String], env: &[(String, String)]) -> Result<String, String> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| "empty command".to_owned())?;
    let mut command = Command::new(program);
    command.args(rest).env_clear();
    for (k, v) in env {
        command.env(k, v);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forge(kind: ForgeKind, cmd: Option<&str>) -> ForgeConfig {
        ForgeConfig {
            kind: Some(kind),
            host: None,
            token_cmd: cmd.map(str::to_owned),
            url: None,
        }
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn env_variables_by_kind() {
        assert_eq!(token_env_var(ForgeKind::Gitlab), "GITLAB_TOKEN");
        assert_eq!(token_env_var(ForgeKind::Github), "GITHUB_TOKEN");
        assert_eq!(
            token(
                "gl",
                &forge(ForgeKind::Gitlab, None),
                &env(&[("GITLAB_TOKEN", " t1 ")])
            )
            .unwrap(),
            "t1"
        );
        assert_eq!(
            token(
                "gh",
                &forge(ForgeKind::Github, None),
                &env(&[("GITHUB_TOKEN", "t2")])
            )
            .unwrap(),
            "t2"
        );
        assert_eq!(
            token("gh", &forge(ForgeKind::Github, None), &env(&[("GITHUB_TOKEN", "  ")])).unwrap_err(),
            SourceError::Auth(
                "no token for forge.gh: set forge.gh.token_cmd or the GITHUB_TOKEN environment variable".into()
            )
        );
        let unknown_kind = ForgeConfig {
            kind: None,
            ..forge(ForgeKind::Gitlab, None)
        };
        assert!(token("x", &unknown_kind, &env(&[("GITLAB_TOKEN", "t")])).is_ok());
    }

    #[test]
    fn token_cmd_output_is_trimmed_and_failures_hide_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = env(&[("PATH", dir.path().to_str().unwrap())]);
        let script = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            // Probe until the fresh script is runnable (ETXTBSY from other threads).
            for _ in 0..200 {
                if Command::new(&p).env("TOKEN_PROBE", "1").output().is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        };
        script("tok", "echo '  s3cret  '");
        assert_eq!(
            token("gl", &forge(ForgeKind::Gitlab, Some(" tok  ")), &path).unwrap(),
            "s3cret"
        );
        script("tok", "echo 'not logged in' >&2; exit 1");
        assert_eq!(
            token("gl", &forge(ForgeKind::Gitlab, Some("tok")), &path).unwrap_err(),
            SourceError::Auth("forge.gl.token_cmd \"tok\" failed: not logged in".into())
        );
        script("tok", "echo");
        assert_eq!(
            token("gl", &forge(ForgeKind::Gitlab, Some("tok")), &path).unwrap_err(),
            SourceError::Auth("forge.gl.token_cmd \"tok\" printed nothing".into())
        );
        assert!(matches!(
            token("gl", &forge(ForgeKind::Gitlab, Some("tok 'unbalanced")), &path).unwrap_err(),
            SourceError::Auth(m) if m.starts_with("forge.gl.token_cmd")
        ));
        assert!(matches!(
            token("gl", &forge(ForgeKind::Gitlab, Some("no-such-tool")), &path).unwrap_err(),
            SourceError::Auth(m) if m.contains("failed")
        ));
        // An empty command falls back to the environment variable.
        assert_eq!(
            token(
                "gl",
                &forge(ForgeKind::Gitlab, Some("  ")),
                &env(&[("GITLAB_TOKEN", "e")])
            )
            .unwrap(),
            "e"
        );
    }
}
