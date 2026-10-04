//! `~` expansion.

use std::path::{Path, PathBuf};

/// Expands a leading `~` or `~/` to `home`.
///
/// Only the current user's home is expanded: `~alice/x` and a `~` in the
/// middle of a path are returned unchanged, as is everything when `home` is
/// `None` (no `$HOME` in the environment).
pub fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_bare_and_prefixed_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(expand_tilde(Path::new("~"), Some(home)), home);
        assert_eq!(
            expand_tilde(Path::new("~/code/x"), Some(home)),
            PathBuf::from("/home/me/code/x")
        );
    }

    #[test]
    fn leaves_other_paths_alone() {
        let home = Path::new("/home/me");
        for p in ["/abs", "rel/~", "~alice/x", "~x", ""] {
            assert_eq!(
                expand_tilde(Path::new(p), Some(home)),
                PathBuf::from(p),
                "{p}"
            );
        }
        assert_eq!(expand_tilde(Path::new("~/x"), None), PathBuf::from("~/x"));
    }
}
