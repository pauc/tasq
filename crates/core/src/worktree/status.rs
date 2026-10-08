//! What git says about a repository's worktrees, interpreted for a release
//! (plan 4.2 step 3, WT-206): `git worktree list --porcelain -z`,
//! `git status --porcelain -z`, and from them the [`DirSeen`] of a declared
//! worktree. The caller runs git; these functions only read its output.

use std::path::{Path, PathBuf};

use super::lifecycle::DirSeen;

/// One entry of `git worktree list --porcelain -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its directory, as git records it.
    pub path: PathBuf,
    /// The branch checked out (`refs/heads/` removed), `None` when detached
    /// or bare.
    pub branch: Option<String>,
    /// Whether `git worktree lock` holds it.
    pub locked: bool,
    /// Whether it is the bare repository itself.
    pub bare: bool,
}

/// The entries of `git worktree list --porcelain -z`: attributes end with a
/// NUL, entries with an empty attribute. The first is the main worktree.
pub fn listed(output: &str) -> Vec<Listed> {
    let mut all = Vec::new();
    let mut current: Option<Listed> = None;
    for attribute in output.split('\0') {
        if attribute.is_empty() {
            all.extend(current.take());
            continue;
        }
        let (key, value) = attribute.split_once(' ').unwrap_or((attribute, ""));
        if key == "worktree" {
            all.extend(current.take());
            current = Some(Listed {
                path: PathBuf::from(value),
                branch: None,
                locked: false,
                bare: false,
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        match key {
            "branch" => {
                entry.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                );
            }
            "locked" => entry.locked = true,
            "bare" => entry.bare = true,
            _ => {}
        }
    }
    all.extend(current);
    all
}

/// The changed paths in `git status --porcelain -z` output that are not
/// among `provisioned` (paths relative to the worktree that provisioning
/// wrote: copies, links, rendered templates, which can be made again), nor
/// inside one of them. A rename's or copy's second path (its origin, in
/// either status column) is skipped.
pub fn changes(output: &str, provisioned: &[PathBuf]) -> Vec<String> {
    let mut found = Vec::new();
    let mut fields = output.split('\0').filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        let (code, path) = entry.split_at_checked(3).unwrap_or((entry, ""));
        if code.contains(['R', 'C']) {
            fields.next();
        }
        let trimmed = Path::new(path.trim_end_matches('/'));
        if !provisioned.iter().any(|p| trimmed.starts_with(p)) {
            found.push(path.to_owned());
        }
    }
    found
}

/// What a release sees at the declared worktree `path`, first match wins:
/// absent, the main checkout (`main`, or the first of `listed`), not among
/// `listed`, containing `cwd`, locked, with `changes()` (only asked when
/// all else passed), else clean. Paths are compared as given, so the
/// caller passes canonical ones.
pub fn dir_seen(
    path: &Path,
    exists: bool,
    main: &Path,
    cwd: &Path,
    listed: &[Listed],
    changes: impl FnOnce() -> Vec<String>,
) -> DirSeen {
    if !exists {
        return DirSeen::Absent;
    }
    if path == main || listed.first().is_some_and(|l| l.path == path) {
        return DirSeen::Main;
    }
    let Some(entry) = listed.iter().find(|l| l.path == path) else {
        return DirSeen::Foreign;
    };
    if cwd.starts_with(path) {
        DirSeen::InUse
    } else if entry.locked {
        DirSeen::Locked
    } else if changes().is_empty() {
        DirSeen::Clean
    } else {
        DirSeen::Dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, branch: Option<&str>, locked: bool, bare: bool) -> Listed {
        Listed {
            path: PathBuf::from(path),
            branch: branch.map(str::to_owned),
            locked,
            bare,
        }
    }

    #[test]
    fn worktree_list_entries() {
        let output = "worktree /code/app\0HEAD 1111\0branch refs/heads/main\0\0\
            worktree /code/app-b\0HEAD 2222\0branch refs/heads/feat/b\0locked\0\0\
            worktree /code/app d\0HEAD 3333\0detached\0locked because\0prunable gone\0\0";
        assert_eq!(
            listed(output),
            [
                entry("/code/app", Some("main"), false, false),
                entry("/code/app-b", Some("feat/b"), true, false),
                entry("/code/app d", None, true, false),
            ]
        );
        assert_eq!(
            listed("worktree /srv/app.git\0bare\0\0"),
            [entry("/srv/app.git", None, false, true)]
        );
        assert_eq!(listed(""), Vec::new());
        assert_eq!(listed("HEAD 1\0locked\0\0"), Vec::new(), "no worktree line");
        assert_eq!(
            listed("worktree /a\0branch other\0worktree /b"),
            [
                entry("/a", Some("other"), false, false),
                entry("/b", None, false, false)
            ],
            "no trailing NULs"
        );
    }

    #[test]
    fn changes_leave_out_what_provisioning_wrote() {
        let provisioned = [PathBuf::from(".envrc"), PathBuf::from("config/local")];
        let output = " M src/main.rs\0?? .envrc\0?? config/local/\0?? config/local/x\0\
            R  new.rs\0old.rs\0?? config/localx\0A  .envrc.d/a\0";
        assert_eq!(
            changes(output, &provisioned),
            ["src/main.rs", "new.rs", "config/localx", ".envrc.d/a"]
        );
        assert_eq!(changes("", &provisioned), Vec::<String>::new());
        assert_eq!(
            changes("C  copy\0orig\0?? .envrc\0 R moved\0from\0", &provisioned),
            ["copy", "moved"]
        );
        assert_eq!(changes("?? x\0", &[]), ["x"]);
        assert_eq!(changes("??", &[]), [""], "a cut entry still counts");
    }

    #[test]
    fn what_a_release_sees_first_match_wins() {
        let listed = [
            entry("/code/app", Some("main"), false, false),
            entry("/code/app-a", Some("a"), false, false),
            entry("/code/app-l", Some("l"), true, false),
        ];
        let main = Path::new("/code/app");
        let away = Path::new("/home");
        let clean = Vec::new;
        let dirty = || vec!["x".to_owned()];
        let never = || -> Vec<String> { unreachable!("not asked") };
        let seen = |path: &str, exists, cwd: &Path, changes: &dyn Fn() -> Vec<String>| {
            dir_seen(Path::new(path), exists, main, cwd, &listed, changes)
        };
        assert_eq!(seen("/code/app-a", false, away, &never), DirSeen::Absent);
        assert_eq!(seen("/code/app", true, away, &never), DirSeen::Main);
        assert_eq!(seen("/code/other", true, away, &never), DirSeen::Foreign);
        assert_eq!(
            seen("/code/app-l", true, Path::new("/code/app-l/src"), &never),
            DirSeen::InUse
        );
        assert_eq!(
            seen("/code/app-a", true, Path::new("/code/app-a"), &never),
            DirSeen::InUse
        );
        assert_eq!(seen("/code/app-l", true, away, &never), DirSeen::Locked);
        assert_eq!(seen("/code/app-a", true, away, &dirty), DirSeen::Dirty);
        assert_eq!(seen("/code/app-a", true, away, &clean), DirSeen::Clean);
        assert_eq!(
            seen("/code/app-a", true, Path::new("/code/app-ab"), &clean),
            DirSeen::Clean,
            "a sibling with a longer name is not inside"
        );
        // The main checkout as git lists it, under another spelling.
        assert_eq!(
            dir_seen(
                Path::new("/code/app"),
                true,
                Path::new("/elsewhere"),
                away,
                &listed,
                never
            ),
            DirSeen::Main
        );
    }
}
