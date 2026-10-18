//! The store's part of `tasq doctor` (plan T-205): a list of [`Check`]s
//! about the notebook, its index, the bookkeeper and the git identity,
//! reached through [`Store::doctor`]. The CLI prints them as OK/WARN/FAIL
//! lines or as JSON.

use std::path::Path;

use tasq_core::store::{IdScheme, Store};

use crate::git::Git;
use crate::store::NbStore;

pub use tasq_core::doctor::{Check, CheckStatus, any_failed};

/// The checks for an opened store, in display order:
///
/// 1. `notebook`: the resolved directory, and whether it is a git repository.
/// 2. `index`: `.index` present and consistent (through the bookkeeper's
///    `verify`), with [`crate::bookkeeper::RECONCILE_FIX`] as the fix.
/// 3. `ids`: a warning for positional ids (T-204).
/// 4. `bookkeeper`: which strategy is in use and how often it syncs.
/// 5. `git-identity`: present when commits will be made (a repository); a
///    warning otherwise, since every background commit would fail.
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
                "ids are positions in .index ({} tasks) and can change when another tool rewrites it (nb index reconcile)",
                info.task_count
            ),
            "refer to tasks by title in notes you keep outside tasq",
        ));
    } else {
        out.push(Check::ok("ids", "ids are stable"));
    }
    out.push(Check::ok(
        "bookkeeper",
        bookkeeper_detail(store.bookkeeper().name(), store.sync_interval()),
    ));
    if let Some(check) = identity_check(&git, store.bookkeeper().name()) {
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
            crate::bookkeeper::RECONCILE_FIX,
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

/// `bookkeeper: native, syncing at most every 3600 s after a write`.
fn bookkeeper_detail(name: &str, sync_interval: u64) -> String {
    if sync_interval == 0 {
        format!("bookkeeper: {name}, no automatic sync")
    } else {
        format!("bookkeeper: {name}, syncing at most every {sync_interval} s after a write")
    }
}

/// `None` when no commits will ever be made (no repository, or no
/// bookkeeper that commits).
fn identity_check(git: &Git, bookkeeper: &str) -> Option<Check> {
    if bookkeeper != "native" || !git.is_repository() {
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
            Check::warn(
                "git-identity",
                format!("git {missing} not set; commits will fail"),
                fix,
            )
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
    fn bookkeeper_detail_names_the_interval() {
        assert_eq!(
            bookkeeper_detail("native", 3600),
            "bookkeeper: native, syncing at most every 3600 s after a write"
        );
        assert_eq!(
            bookkeeper_detail("native", 0),
            "bookkeeper: native, no automatic sync"
        );
    }

    #[test]
    fn identity_check_only_where_commits_are_made() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let env = vec![
            ("HOME".to_owned(), home.display().to_string()),
            ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ];
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = Git::new(&repo, env.clone());
        assert_eq!(identity_check(&git, "none"), None);
        assert_eq!(identity_check(&git, "native"), None, "not a repository");

        git.run(&["init", "-q"]).unwrap();
        assert_eq!(identity_check(&git, "none"), None, "no bookkeeper commits");
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.status, CheckStatus::Warn);
        assert_eq!(
            c.detail,
            "git user.name and user.email not set; commits will fail"
        );
        assert!(c.fix.as_deref().unwrap().starts_with("set a git identity"));

        git.run(&["config", "user.email", "t@example.invalid"])
            .unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.detail, "git user.name not set; commits will fail");
        git.run(&["config", "--unset", "user.email"]).unwrap();
        git.run(&["config", "user.name", "T"]).unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.detail, "git user.email not set; commits will fail");
        git.run(&["config", "user.email", "t@example.invalid"])
            .unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.status, CheckStatus::Ok);
        assert_eq!(c.detail, "git identity: T <t@example.invalid>");
        assert_eq!(c.fix, None);

        // A repository git cannot read: a warning, not a verdict.
        std::fs::write(repo.join(".git/config"), "[broken").unwrap();
        let c = identity_check(&git, "native").unwrap();
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(
            c.detail.starts_with("could not read the git identity: "),
            "{c:?}"
        );
    }
}
