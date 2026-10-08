//! What a repository says about itself on disk, read without running `git`
//! (`on_create = "auto"` and the `{{repo}}` variable at declaration time).
//!
//! The caller reads the files; these functions only interpret them.

use std::path::Path;

/// The work tree `dir` belongs to: the nearest of `dir` and its ancestors
/// with a `.git` entry (a directory in a checkout, a file in a linked
/// worktree), as told by `has_git`.
pub fn work_tree_root(dir: &Path, has_git: impl Fn(&Path) -> bool) -> Option<&Path> {
    dir.ancestors().find(|d| has_git(&d.join(".git")))
}

/// The git directory a linked worktree's `.git` file points to
/// (`gitdir: /repo/.git/worktrees/x`), as written; relative paths are the
/// caller's to resolve against the work tree.
pub fn gitdir(dot_git: &str) -> Option<&str> {
    dot_git
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

/// `remote.origin.url` in the text of a git `config` file: the first `url`
/// of a `[remote "origin"]` section. Quotes are removed and an unquoted
/// value ends at `#` or `;`, as git reads it. Includes are not followed.
pub fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            in_origin = is_origin_section(header);
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("url") {
            let url = config_value(value);
            return (!url.is_empty()).then_some(url);
        }
    }
    None
}

/// Whether a section header (after its `[`) is `remote "origin"`; the
/// section name ignores case, the subsection does not.
fn is_origin_section(header: &str) -> bool {
    let header = header.split(']').next().unwrap_or_default();
    header
        .split_once(char::is_whitespace)
        .is_some_and(|(name, sub)| {
            name.eq_ignore_ascii_case("remote") && sub.trim() == "\"origin\""
        })
}

/// A config value: quoted text without its quotes, or the text up to a
/// comment, trimmed.
fn config_value(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(quoted) = raw.strip_prefix('"') {
        return quoted.split('"').next().unwrap_or_default().to_owned();
    }
    raw.split(['#', ';'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_ancestor_with_git_is_the_root() {
        let has = |roots: &'static [&'static str]| {
            move |p: &Path| roots.iter().any(|r| p == Path::new(r))
        };
        let dir = Path::new("/code/app/src/bin");
        assert_eq!(
            work_tree_root(dir, has(&["/code/app/.git"])),
            Some(Path::new("/code/app"))
        );
        assert_eq!(
            work_tree_root(dir, has(&["/code/app/.git", "/code/app/src/bin/.git"])),
            Some(dir),
            "the directory itself first"
        );
        assert_eq!(work_tree_root(dir, has(&["/.git"])), Some(Path::new("/")));
        assert_eq!(work_tree_root(dir, has(&["/other/.git"])), None);
    }

    #[test]
    fn gitdir_lines() {
        assert_eq!(
            gitdir("gitdir: /code/app/.git/worktrees/x\n"),
            Some("/code/app/.git/worktrees/x")
        );
        assert_eq!(
            gitdir("\ngitdir:../.git/worktrees/y"),
            Some("../.git/worktrees/y")
        );
        assert_eq!(gitdir("gitdir:   \n"), None);
        assert_eq!(gitdir("something else"), None);
        assert_eq!(gitdir(""), None);
        assert_eq!(gitdir("gitdir: a b "), Some("a b"));
    }

    #[test]
    fn origin_url_from_a_config() {
        let config = "\
[core]
\turl = not-a-remote
[remote \"upstream\"]
\turl = git@host:up/app.git
[REMOTE \"origin\"]
\tfetch = +refs/heads/*:refs/remotes/origin/*
\tURL = git@gitlab.com:group/app.git
\turl = second
[branch \"main\"]
\turl = nope
";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("git@gitlab.com:group/app.git")
        );
    }

    #[test]
    fn origin_url_values() {
        let url = |value: &str| origin_url(&format!("[remote \"origin\"]\n url ={value}\n"));
        assert_eq!(
            url(" https://h/a ; comment").as_deref(),
            Some("https://h/a")
        );
        assert_eq!(url(" https://h/a # c").as_deref(), Some("https://h/a"));
        assert_eq!(
            url(" \"/srv/my repo;x\" ").as_deref(),
            Some("/srv/my repo;x")
        );
        assert_eq!(url(" "), None);
        assert_eq!(url(" \"\""), None);
    }

    #[test]
    fn origin_needs_its_own_section() {
        assert_eq!(origin_url("[remote \"Origin\"]\nurl = x\n"), None);
        assert_eq!(origin_url("[remote]\nurl = x\n"), None);
        assert_eq!(origin_url("[remoteorigin]\nurl = x\n"), None);
        assert_eq!(origin_url("url = x\n"), None);
        assert_eq!(origin_url("[remote \"origin\"]\nurlx = x\nurl\n"), None);
        assert_eq!(
            origin_url("[remote \"origin\"] # c\n  url = x").as_deref(),
            Some("x")
        );
        assert_eq!(
            origin_url("[remote \"origin\"]\n[core]\nurl = x\n"),
            None,
            "a later section ends it"
        );
    }
}
