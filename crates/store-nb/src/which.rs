//! Finding an executable on a `PATH` value, for `tasq doctor`'s tool checks
//! and the CLI's optional tools.

use std::path::{Path, PathBuf};

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
