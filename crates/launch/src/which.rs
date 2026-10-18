//! Finding an executable on a `PATH` value, for `tasq doctor`'s tool checks
//! ([`tool_check`]) and the CLI's optional tools (`glow` for `tasq view`).

use std::path::{Path, PathBuf};

use tasq_core::doctor::Check;

/// Whether an executable called `name` is on the `PATH` entry of `env`.
/// `Ok` with its path, otherwise `Warn` with `fix` (how to install it).
pub fn tool_check(name: &str, env: &[(String, String)], fix: &str) -> Check {
    let path_var = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map_or("", |(_, v)| v.as_str());
    match find_in_path(path_var, name) {
        Some(found) => Check::ok(name, format!("{name} found at {}", found.display())),
        None => Check::warn(name, format!("{name} is not on PATH"), fix),
    }
}

/// The first executable file called `name` in the `:`-separated `path_var`.
/// Empty entries are skipped (the shell would treat them as the current
/// directory, which we do not want for a tool we then run).
pub fn find_in_path(path_var: &str, name: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| is_executable(candidate))
}

/// A regular file with (on Unix) at least one execute bit set.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::doctor::CheckStatus;

    #[test]
    #[cfg(unix)]
    fn tool_check_looks_in_the_given_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("glow");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = vec![("PATH".to_owned(), dir.path().display().to_string())];
        let c = tool_check("glow", &env, "install glow");
        assert_eq!(c.status, CheckStatus::Ok);
        assert_eq!(c.name, "glow");
        assert_eq!(c.detail, format!("glow found at {}", tool.display()));
        assert_eq!(c.fix, None);
        let c = tool_check("gh", &env, "install gh");
        assert_eq!(c.status, CheckStatus::Warn);
        assert_eq!(c.detail, "gh is not on PATH");
        assert_eq!(c.fix.as_deref(), Some("install gh"));
        let c = tool_check("glow", &[], "install glow");
        assert_eq!(c.status, CheckStatus::Warn, "no PATH at all");
    }

    #[test]
    #[cfg(unix)]
    fn finds_the_first_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("tool"), "not executable").unwrap();
        std::fs::write(second.join("tool"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(second.join("tool"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let path_var = format!(
            ":{}:{}:{}",
            first.display(),
            second.display(),
            dir.path().display()
        );
        assert_eq!(find_in_path(&path_var, "tool"), Some(second.join("tool")));
        assert_eq!(find_in_path(&path_var, "missing"), None);
        assert_eq!(find_in_path("", "tool"), None);
        // A directory with the name is not an executable.
        std::fs::create_dir_all(dir.path().join("dir")).unwrap();
        assert_eq!(find_in_path(&dir.path().display().to_string(), "dir"), None);
    }
}
