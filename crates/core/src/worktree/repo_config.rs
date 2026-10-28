//! `tasq doctor`'s check of a repository config (ADR 0031): its `path`
//! applies only when it is exactly the main checkout git reports, so
//! anything else never matches and is worth a warning.

use std::path::{Path, PathBuf};

use crate::config::contract_tilde;
use crate::doctor::Check;

/// The name of the check, also for the doctor checks of repository
/// configs made elsewhere.
pub const NAME: &str = "repo-config";

/// What a repository config's `path` points at on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Nothing, or something that is not a directory.
    Missing,
    /// A directory outside any git repository.
    NotRepository,
    /// A directory in a git repository.
    Repository {
        /// The path with every symlink and `..` resolved.
        resolved: PathBuf,
        /// The work tree the path is in, as found from it: the main
        /// checkout or a linked worktree.
        root: PathBuf,
        /// The repository's main checkout, as the worktree commands see it.
        main: PathBuf,
    },
}

/// The check of the repository config `file`, whose `path` is `path`; a
/// fix shows `home` as `~`.
pub fn check(file: &Path, path: &Path, found: &Found, home: Option<&Path>) -> Check {
    let detail = |what: &str| format!("{}: path {} {what}", file.display(), path.display());
    match found {
        Found::Missing => Check::warn(
            NAME,
            detail("does not exist or is not a directory"),
            "set path to the main checkout of a repository",
        ),
        Found::NotRepository => Check::warn(
            NAME,
            detail("is not inside a git repository"),
            "set path to the main checkout of a repository",
        ),
        Found::Repository { main, .. } if main == path => {
            Check::ok(NAME, detail("is a main checkout"))
        }
        Found::Repository {
            resolved,
            root,
            main,
        } => {
            let what = if resolved == main {
                format!(
                    "resolves to the main checkout {} through a symlink or ..",
                    main.display()
                )
            } else if root == path {
                "is a linked worktree, not the main checkout".to_owned()
            } else {
                "is a subdirectory, not the main checkout".to_owned()
            };
            Check::warn(
                NAME,
                detail(&what),
                format!(
                    "set path = {}",
                    toml::Value::String(contract_tilde(main, home))
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(path: &str, found: &Found) -> (String, String, Option<String>) {
        let check = check(
            Path::new("/c/repos/r.toml"),
            Path::new(path),
            found,
            Some(Path::new("/home/u")),
        );
        assert_eq!(check.name, "repo-config");
        (check.status.to_string(), check.detail, check.fix)
    }

    fn repository(resolved: &str, root: &str, main: &str) -> Found {
        Found::Repository {
            resolved: PathBuf::from(resolved),
            root: PathBuf::from(root),
            main: PathBuf::from(main),
        }
    }

    fn warned(detail: &str, fix: &str) -> (String, String, Option<String>) {
        (
            "WARN".to_owned(),
            format!("/c/repos/r.toml: {detail}"),
            Some(fix.to_owned()),
        )
    }

    #[test]
    fn only_the_main_checkout_itself_is_ok() {
        assert_eq!(
            checked("/r", &repository("/r", "/r", "/r")),
            (
                "OK".to_owned(),
                "/c/repos/r.toml: path /r is a main checkout".to_owned(),
                None
            )
        );
    }

    #[test]
    fn missing_and_outside_git_warn() {
        let fix = "set path to the main checkout of a repository";
        assert_eq!(
            checked("/r", &Found::Missing),
            warned("path /r does not exist or is not a directory", fix)
        );
        assert_eq!(
            checked("/r", &Found::NotRepository),
            warned("path /r is not inside a git repository", fix)
        );
    }

    #[test]
    fn anything_else_names_the_main_checkout_to_write() {
        let fix = "set path = \"/r\"";
        assert_eq!(
            checked("/link", &repository("/r", "/link", "/r")),
            warned(
                "path /link resolves to the main checkout /r through a symlink or ..",
                fix
            )
        );
        // Found through a symlink to a linked worktree: still a worktree.
        assert_eq!(
            checked("/l", &repository("/r-l", "/l", "/r")),
            warned("path /l is a linked worktree, not the main checkout", fix)
        );
        assert_eq!(
            checked("/r-l", &repository("/r-l", "/r-l", "/r")),
            warned("path /r-l is a linked worktree, not the main checkout", fix)
        );
        assert_eq!(
            checked("/r/sub", &repository("/r/sub", "/r", "/r")),
            warned("path /r/sub is a subdirectory, not the main checkout", fix)
        );
        assert_eq!(
            checked("/r-l/sub", &repository("/r-l/sub", "/r-l", "/r")),
            warned(
                "path /r-l/sub is a subdirectory, not the main checkout",
                fix
            )
        );
    }

    /// The fix is a TOML line to paste: home as `~`, quotes escaped.
    #[test]
    fn the_fix_is_a_toml_line() {
        assert_eq!(
            checked(
                "/home/u/p/sub",
                &repository("/home/u/p/sub", "/home/u/p", "/home/u/p")
            )
            .2,
            Some("set path = \"~/p\"".to_owned())
        );
        assert_eq!(
            checked("/x/sub", &repository("/x/sub", "/x\"y", "/x\"y")).2,
            Some("set path = '/x\"y'".to_owned())
        );
    }
}
