//! The store's part of `tasq doctor` (plan T-205): a list of [`Check`]s
//! about the notebook, its index, nb and the bookkeeper. The CLI (Phase 3)
//! prints them as OK/WARN/FAIL lines or as JSON and adds its own checks for
//! optional tools through [`tool_check`].

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tasq_core::store::{IdScheme, Store};

use crate::git::Git;
use crate::nb::find_in_path;
use crate::store::NbStore;

/// How a check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Fine.
    Ok,
    /// Works, but the user should know.
    Warn,
    /// Broken; `tasq doctor` exits 1.
    Fail,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        })
    }
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// Short identifier (`notebook`, `index`, `nb`, ...).
    pub name: String,
    /// The verdict.
    pub status: CheckStatus,
    /// One line saying what was found.
    pub detail: String,
    /// What to do about it, for `Warn` and `Fail`.
    pub fix: Option<String>,
}

impl Check {
    /// An `Ok` check.
    pub fn ok(name: &str, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Ok, detail, None)
    }

    /// A `Warn` check with its fix.
    pub fn warn(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Warn, detail, Some(fix.into()))
    }

    /// A `Fail` check with its fix.
    pub fn fail(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Fail, detail, Some(fix.into()))
    }

    fn new(
        name: &str,
        status: CheckStatus,
        detail: impl Into<String>,
        fix: Option<String>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            status,
            detail: detail.into(),
            fix,
        }
    }
}

/// Whether any check failed (the CLI exits 1 then).
pub fn any_failed(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.status == CheckStatus::Fail)
}

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

/// The checks for an opened store, in display order:
///
/// 1. `notebook`: the resolved directory, and whether it is a git repository.
/// 2. `index`: `.index` present and consistent (through the bookkeeper's
///    `verify`), with `nb index reconcile` as the fix.
/// 3. `ids`: a warning for positional ids (T-204).
/// 4. `nb`: available, with its version, or not.
/// 5. `bookkeeper`: which strategy is in use.
/// 6. `git-identity`: present when commits will be made. nb silently does
///    nothing without one, so with the nb bookkeeper this is a failure; with
///    the native one on a repository it is a warning.
pub fn checks(store: &NbStore) -> Vec<Check> {
    let dir = store.dir();
    let git = Git::new(dir, store.env().to_vec());
    let mut out = vec![notebook_check(dir, git.is_repository())];
    out.push(index_check(store));
    let info = store.describe();
    if info.id_scheme == IdScheme::Positional {
        out.push(Check::warn(
            "ids",
            format!(
                "ids are positions in .index ({} tasks) and can change after 'nb index reconcile' or deletions",
                info.task_count
            ),
            "refer to tasks by title in notes you keep outside tasq",
        ));
    } else {
        out.push(Check::ok("ids", "ids are stable"));
    }
    out.push(nb_check(store));
    let bookkeeper = store.bookkeeper().name();
    out.push(Check::ok("bookkeeper", format!("bookkeeper: {bookkeeper}")));
    if let Some(check) = identity_check(&git, bookkeeper) {
        out.push(check);
    }
    out
}

fn notebook_check(dir: &Path, is_repository: bool) -> Check {
    let repo = if is_repository {
        "a git repository"
    } else {
        "not a git repository (no commits or sync)"
    };
    Check::ok("notebook", format!("notebook at {}, {repo}", dir.display()))
}

fn index_check(store: &NbStore) -> Check {
    let path = store.index_path();
    if !path.is_file() {
        return Check::fail(
            "index",
            format!("{} is missing", path.display()),
            "run 'nb index reconcile' in the notebook",
        );
    }
    match store.bookkeeper().verify() {
        Ok(v) if v.consistent => Check::ok("index", format!("{}: {}", path.display(), v.detail)),
        Ok(v) => Check::fail(
            "index",
            format!("{}: {}", path.display(), v.detail),
            v.fix().unwrap_or(crate::bookkeeper::RECONCILE_FIX),
        ),
        Err(e) => Check::fail(
            "index",
            format!("could not verify {}: {e}", path.display()),
            crate::bookkeeper::RECONCILE_FIX,
        ),
    }
}

fn nb_check(store: &NbStore) -> Check {
    let Some(nb) = store.nb() else {
        return Check::warn(
            "nb",
            "nb is not on PATH; the native bookkeeper keeps .index and commits",
            "install nb (https://github.com/xwmx/nb) to share the notebook with nb",
        );
    };
    match nb.version() {
        Ok(version) => Check::ok("nb", format!("nb {version} at {}", nb.program().display())),
        Err(e) => Check::fail(
            "nb",
            format!("nb at {} does not run: {e}", nb.program().display()),
            "reinstall nb or set store.bookkeeper = \"native\"",
        ),
    }
}

/// `None` when no commits will ever be made (no repository, or no
/// bookkeeper that commits).
fn identity_check(git: &Git, bookkeeper: &str) -> Option<Check> {
    let commits = match bookkeeper {
        "nb" => true,
        "native" => git.is_repository(),
        _ => false,
    };
    if !commits {
        return None;
    }
    let fix = "set a git identity: git config --global user.name <name>; git config --global user.email <email>";
    Some(match git.identity() {
        Ok((Some(name), Some(email))) => {
            Check::ok("git-identity", format!("git identity: {name} <{email}>"))
        }
        Ok((name, email)) => {
            let missing = match (name, email) {
                (None, None) => "user.name and user.email",
                (None, Some(_)) => "user.name",
                (Some(_), _) => "user.email",
            };
            let detail = format!("git {missing} not set");
            if bookkeeper == "nb" {
                Check::fail(
                    "git-identity",
                    format!("{detail}; nb silently skips commits without an identity"),
                    fix,
                )
            } else {
                Check::warn("git-identity", format!("{detail}; commits will fail"), fix)
            }
        }
        Err(e) => Check::warn(
            "git-identity",
            format!("could not read the git identity: {e}"),
            fix,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_display_and_serde() {
        assert_eq!(CheckStatus::Ok.to_string(), "OK");
        assert_eq!(CheckStatus::Warn.to_string(), "WARN");
        assert_eq!(CheckStatus::Fail.to_string(), "FAIL");
        assert_eq!(
            serde_json::to_string(&CheckStatus::Warn).unwrap(),
            "\"warn\""
        );
        let c = Check::fail("index", "missing", "reconcile");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            "{\"name\":\"index\",\"status\":\"fail\",\"detail\":\"missing\",\"fix\":\"reconcile\"}"
        );
        assert_eq!(serde_json::from_str::<Check>(&json).unwrap(), c);
        assert_eq!(
            serde_json::to_string(&Check::ok("nb", "fine")).unwrap(),
            "{\"name\":\"nb\",\"status\":\"ok\",\"detail\":\"fine\",\"fix\":null}"
        );
    }

    #[test]
    fn constructors() {
        let ok = Check::ok("a", "fine");
        assert_eq!(
            (
                ok.name.as_str(),
                ok.status,
                ok.detail.as_str(),
                ok.fix.as_deref()
            ),
            ("a", CheckStatus::Ok, "fine", None)
        );
        let warn = Check::warn("b", "meh", "do x");
        assert_eq!(
            (warn.status, warn.fix.as_deref()),
            (CheckStatus::Warn, Some("do x"))
        );
        let fail = Check::fail("c", "bad", "do y");
        assert_eq!(
            (fail.status, fail.fix.as_deref()),
            (CheckStatus::Fail, Some("do y"))
        );
        assert!(!any_failed(&[ok.clone(), warn.clone()]));
        assert!(any_failed(&[ok, warn, fail]));
        assert!(!any_failed(&[]));
    }

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
    fn notebook_check_mentions_git() {
        let c = notebook_check(Path::new("/nb/home"), true);
        assert_eq!(c.status, CheckStatus::Ok);
        assert_eq!(c.detail, "notebook at /nb/home, a git repository");
        let c = notebook_check(Path::new("/nb/home"), false);
        assert_eq!(
            c.detail,
            "notebook at /nb/home, not a git repository (no commits or sync)"
        );
    }

    #[test]
    fn identity_check_depends_on_who_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let env = vec![
            ("HOME".to_owned(), home.display().to_string()),
            ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ];
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git_env = env;
        let git = Git::new(&repo, git_env.clone());
        assert_eq!(identity_check(&git, "none"), None);
        assert_eq!(identity_check(&git, "native"), None, "not a repository");
        // nb commits whether or not we can tell it is a repository; `git
        // config` reads the global identity outside a repository too.
        let c = identity_check(&git, "nb").unwrap();
        assert_eq!(c.status, CheckStatus::Fail);
        // A directory git cannot enter is a warning, not a verdict.
        let gone = Git::new(tmp.path().join("gone"), git_env.clone());
        let c = identity_check(&gone, "nb").unwrap();
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(
            c.detail.starts_with("could not read the git identity: "),
            "{c:?}"
        );

        git.run(&["init", "-q"]).unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.status, CheckStatus::Warn);
        assert_eq!(
            c.detail,
            "git user.name and user.email not set; commits will fail"
        );
        let c = identity_check(&git, "nb").unwrap();
        assert_eq!(c.status, CheckStatus::Fail);
        assert_eq!(
            c.detail,
            "git user.name and user.email not set; nb silently skips commits without an identity"
        );
        assert!(c.fix.as_deref().unwrap().starts_with("set a git identity"));

        git.run(&["config", "user.email", "t@example.invalid"])
            .unwrap();
        let c = identity_check(&git, "nb").unwrap();
        assert_eq!(
            c.detail,
            "git user.name not set; nb silently skips commits without an identity"
        );
        git.run(&["config", "--unset", "user.email"]).unwrap();
        git.run(&["config", "user.name", "T"]).unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.detail, "git user.email not set; commits will fail");
        git.run(&["config", "user.email", "t@example.invalid"])
            .unwrap();
        let c = identity_check(&git, "nb").unwrap();
        assert_eq!(c.status, CheckStatus::Ok);
        assert_eq!(c.detail, "git identity: T <t@example.invalid>");
        assert_eq!(c.fix, None);
    }
}
