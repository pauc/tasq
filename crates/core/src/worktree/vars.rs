//! Template variables for worktree paths, branch names, rendered files and
//! hooks (`docs/config.md`, "Worktrees").
//!
//! What is known grows with the lifecycle: a branch name is rendered from
//! the task alone ([`Scope::Branch`]), a path also knows the branch
//! ([`Scope::Path`]), files and hooks also know the worktree
//! ([`Scope::Worktree`]). [`check`] tells the configuration which names a
//! template may use; [`Vars`] supplies the values.

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;

use crate::config::PortsConfig;
use crate::model::Task;
use crate::template::{self, TemplateError};

/// What a template is rendered for, which decides the variables it sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `worktree.branch`: the task and its project.
    Branch,
    /// `worktree.path`: also the branch and `port`.
    Path,
    /// Rendered files and hooks: also the worktree itself.
    Worktree,
}

/// Variables every scope has.
const TASK_VARS: &[&str] = &[
    "id",
    "title",
    "slug",
    "project",
    "project_name",
    "project_parent",
    "repo",
];
/// Added by [`Scope::Path`].
const BRANCH_VARS: &[&str] = &["branch", "branch_safe", "branch_db"];
/// Added by [`Scope::Worktree`].
const WORKTREE_VARS: &[&str] = &["worktree", "worktree_name"];

/// The built-in names `scope` provides; `port` only with `[worktree.ports]`.
pub fn builtins(scope: Scope, ports: bool) -> Vec<&'static str> {
    let mut names = TASK_VARS.to_vec();
    if scope != Scope::Branch {
        names.extend_from_slice(BRANCH_VARS);
        if ports {
            names.push("port");
        }
    }
    if scope == Scope::Worktree {
        names.extend_from_slice(WORKTREE_VARS);
    }
    names
}

/// Whether `name` is reserved for a built-in variable of any scope.
pub fn is_builtin(name: &str) -> bool {
    builtins(Scope::Worktree, true).contains(&name)
}

/// A template names a variable its scope does not have.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown variable {{{{{name}}}}}; known: {}", known.iter().map(|k| format!("{{{{{k}}}}}")).collect::<Vec<_>>().join(", "))]
pub struct UnknownVariable {
    /// The name used.
    pub name: String,
    /// What the scope offers, sorted.
    pub known: Vec<String>,
}

/// Checks that `template` only names variables of `scope`, the user's
/// `extra` variables, and `port` when `ports` is set.
///
/// # Errors
///
/// [`UnknownVariable`] for the first name (in sorted order) that is none
/// of those.
pub fn check(
    template: &str,
    scope: Scope,
    ports: bool,
    extra: &BTreeMap<String, String>,
) -> Result<(), UnknownVariable> {
    let builtins = builtins(scope, ports);
    let known = |name: &str| builtins.contains(&name) || extra.contains_key(name);
    match template::names(template).into_iter().find(|n| !known(n)) {
        None => Ok(()),
        Some(name) => {
            let mut all: Vec<String> = builtins.iter().map(|b| (*b).to_owned()).collect();
            all.extend(extra.keys().cloned());
            all.sort();
            Err(UnknownVariable {
                name: name.to_owned(),
                known: all,
            })
        }
    }
}

/// The values for one task, built up scope by scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vars(BTreeMap<String, String>);

impl Vars {
    /// [`Scope::Branch`]: the task, its `project` directory and the
    /// repository name (`repo`, from the remote; `project_name` without
    /// one), on top of the user's `extra` variables.
    pub fn for_task(
        task: &Task,
        project: &Path,
        repo: Option<&str>,
        extra: &BTreeMap<String, String>,
    ) -> Self {
        let name = project
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent = project
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut map = extra.clone();
        map.insert("id".into(), task.id.to_string());
        map.insert("title".into(), task.title.clone());
        map.insert("slug".into(), slug(&task.title));
        map.insert("project".into(), project.display().to_string());
        map.insert(
            "repo".into(),
            repo.map_or_else(|| name.clone(), str::to_owned),
        );
        map.insert("project_name".into(), name);
        map.insert("project_parent".into(), parent);
        Self(map)
    }

    /// Adds [`Scope::Path`]: `branch`, its safe forms and, with `ports`,
    /// `port`.
    #[must_use]
    pub fn with_branch(mut self, branch: &str, ports: Option<PortsConfig>) -> Self {
        if let Some(ports) = ports {
            let project = self.0.get("project").cloned().unwrap_or_default();
            self.0
                .insert("port".into(), port(&project, branch, ports).to_string());
        }
        self.0.insert("branch".into(), branch.to_owned());
        self.0.insert("branch_safe".into(), branch_safe(branch));
        self.0.insert("branch_db".into(), branch_db(branch));
        self
    }

    /// Adds [`Scope::Worktree`]: the worktree directory and its name.
    #[must_use]
    pub fn with_worktree(mut self, worktree: &Path) -> Self {
        let name = worktree
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.0
            .insert("worktree".into(), worktree.display().to_string());
        self.0.insert("worktree_name".into(), name);
        self
    }

    /// The values, by name.
    pub fn map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    /// Renders `template` with these values.
    ///
    /// # Errors
    ///
    /// The [`TemplateError`] of [`template::render`].
    pub fn render(&self, template: &str) -> Result<String, TemplateError> {
        template::render(template, &self.0)
    }
}

/// A branch as a name for directories and windows: every character but
/// ASCII letters and digits becomes `_`, runs collapse, and leading and
/// trailing `_` go (`feature/login-form` is `feature_login_form`).
pub fn branch_safe(branch: &str) -> String {
    let mut safe = String::with_capacity(branch.len());
    for c in branch.chars() {
        if c.is_ascii_alphanumeric() {
            safe.push(c);
        } else if !safe.ends_with('_') {
            safe.push('_');
        }
    }
    safe.trim_matches('_').to_owned()
}

/// Longest identifier PostgreSQL keeps.
const DB_NAME_MAX: usize = 63;

/// A branch as a database name: [`branch_safe`] in lowercase, at most 63
/// bytes, never ending in `_`.
pub fn branch_db(branch: &str) -> String {
    let mut db = branch_safe(branch).to_ascii_lowercase();
    db.truncate(DB_NAME_MAX);
    db.trim_end_matches('_').to_owned()
}

/// Longest slug [`slug`] makes.
const SLUG_MAX: usize = 40;

/// A title as a branch-name fragment: lowercase ASCII letters and digits,
/// everything else a single `-`, trimmed, at most 40 bytes cut at a `-`
/// when there is one (`Fix the login form` is `fix-the-login-form`).
pub fn slug(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    if slug.len() > SLUG_MAX {
        let cut = slug[..=SLUG_MAX].rfind('-').unwrap_or(SLUG_MAX);
        slug.truncate(cut);
    }
    slug.trim_end_matches('-').to_owned()
}

/// The repository name in a remote URL: its last path component without
/// `.git` (`git@host:group/app.git` and `https://host/group/app` are both
/// `app`). `None` when there is no such component.
pub fn repo_name(remote_url: &str) -> Option<String> {
    let path = remote_url.trim().trim_end_matches('/');
    let last = path.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != path).then(|| name.to_owned())
}

/// The first port of the block `branch` gets in `project`: `base` plus a
/// stable offset (FNV-1a of project and branch) below `span`, a multiple of
/// `block`. Stable across runs and machines.
pub fn port(project: &str, branch: &str, ports: PortsConfig) -> u16 {
    let blocks = u64::from(ports.span / ports.block.max(1)).max(1);
    let offset = fnv1a(project, branch) % blocks * u64::from(ports.block);
    let port = u64::from(ports.base) + offset;
    u16::try_from(port).unwrap_or(u16::MAX)
}

/// 64-bit FNV-1a over `project`, a NUL byte, then `branch`.
fn fnv1a(project: &str, branch: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    project
        .bytes()
        .chain(std::iter::once(0))
        .chain(branch.bytes())
        .fold(OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(PRIME)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskId;

    #[test]
    fn branch_safe_matches_the_old_worktree_names() {
        for (branch, safe) in [
            ("feature/x", "feature_x"),
            ("fix--y", "fix_y"),
            ("JIRA-12_z", "JIRA_12_z"),
            ("/lead/and/trail/", "lead_and_trail"),
            ("__a__b__", "a_b"),
            ("café", "caf"),
            ("plain", "plain"),
            ("---", ""),
        ] {
            assert_eq!(branch_safe(branch), safe, "{branch}");
        }
    }

    #[test]
    fn branch_db_is_lowercase_and_short() {
        assert_eq!(branch_db("Feature/X-1"), "feature_x_1");
        let long = format!("{}/{}", "a".repeat(62), "b");
        assert_eq!(branch_db(&long), "a".repeat(62));
        assert_eq!(branch_db(&"c".repeat(70)), "c".repeat(63));
        assert_eq!(branch_db(&"d".repeat(63)).len(), 63);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Fix the login form"), "fix-the-login-form");
        assert_eq!(slug("  Review MR !88: paginate  "), "review-mr-88-paginate");
        assert_eq!(slug("Café au lait"), "caf-au-lait");
        assert_eq!(slug("!!!"), "");
        assert_eq!(
            slug("Create a plugin to manage worktrees associated to tasks"),
            "create-a-plugin-to-manage-worktrees"
        );
        // Exactly 40 bytes stays; a 41st byte cuts at the last dash.
        let forty = format!("{} {}", "a".repeat(19), "b".repeat(20));
        assert_eq!(
            slug(&forty),
            format!("{}-{}", "a".repeat(19), "b".repeat(20))
        );
        let cut = format!("{} {}x", "a".repeat(19), "b".repeat(20));
        assert_eq!(slug(&cut), "a".repeat(19));
        assert_eq!(slug(&format!("{} z", "a".repeat(40))), "a".repeat(40));
        assert_eq!(slug(&"q".repeat(50)), "q".repeat(40));
    }

    #[test]
    fn repo_names_from_remotes() {
        for (url, name) in [
            ("git@gitlab.com:group/app.git", Some("app")),
            ("https://github.com/owner/tool", Some("tool")),
            ("https://github.com/owner/tool.git/", Some("tool")),
            ("ssh://git@host:22/a/b/c.git", Some("c")),
            ("/srv/git/local.git", Some("local")),
            ("host:repo", Some("repo")),
            ("plain", None),
            ("https://host/.git", None),
            ("", None),
        ] {
            assert_eq!(repo_name(url).as_deref(), name, "{url}");
        }
    }

    #[test]
    fn ports_are_stable_blocks_in_range() {
        let ports = PortsConfig {
            base: 4000,
            span: 1000,
            block: 10,
        };
        // Pinned: the same project and branch always get the same block.
        assert_eq!(fnv1a("", ""), 0xaf63_bd4c_8601_b7df);
        assert_eq!(fnv1a("a", "b"), 0xe5d2_9919_0426_66b2);
        let p = port("/code/app", "feature-x", ports);
        assert_eq!(p, 4640, "reference FNV-1a computed outside Rust");
        assert_eq!(p, port("/code/app", "feature-x", ports));
        assert_eq!(
            u64::from(p),
            4000 + fnv1a("/code/app", "feature-x") % 100 * 10
        );
        for branch in ["a", "b", "c", "d", "e", "main", "x/y"] {
            let p = port("/code/app", branch, ports);
            assert!((4000..5000).contains(&p), "{branch}: {p}");
            assert_eq!((p - 4000) % 10, 0, "{branch}: {p}");
        }
        let one = PortsConfig {
            base: 7000,
            span: 5,
            block: 5,
        };
        assert_eq!(port("/p", "anything", one), 7000);
        let top = PortsConfig {
            base: 65530,
            span: 6,
            block: 1,
        };
        assert!(port("/p", "b", top) >= 65530);
    }

    fn task() -> Task {
        Task::new(TaskId::from(44), "Fix the login form")
    }

    #[test]
    fn vars_grow_with_the_scope() {
        let extra = BTreeMap::from([("db_prefix".to_owned(), "app".to_owned())]);
        let vars = Vars::for_task(&task(), Path::new("/code/ws/app"), None, &extra);
        let expected_task = BTreeMap::from(
            [
                ("db_prefix", "app"),
                ("id", "44"),
                ("title", "Fix the login form"),
                ("slug", "fix-the-login-form"),
                ("project", "/code/ws/app"),
                ("project_name", "app"),
                ("project_parent", "/code/ws"),
                ("repo", "app"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned())),
        );
        assert_eq!(vars.map(), &expected_task);
        assert_eq!(
            vars.render("{{id}}-{{slug}}").as_deref(),
            Ok("44-fix-the-login-form")
        );
        let ports = PortsConfig {
            base: 4000,
            span: 1000,
            block: 10,
        };
        let vars = vars.with_branch("Feature/X", Some(ports));
        assert_eq!(
            vars.render(
                "{{project_parent}}/{{project_name}}-{{branch_safe}} {{branch}} {{branch_db}}"
            )
            .as_deref(),
            Ok("/code/ws/app-Feature_X Feature/X feature_x")
        );
        assert_eq!(
            vars.map().get("port"),
            Some(&port("/code/ws/app", "Feature/X", ports).to_string())
        );
        let vars = vars.with_worktree(Path::new("/code/ws/app-Feature_X"));
        assert_eq!(
            vars.render("{{worktree}} {{worktree_name}}").as_deref(),
            Ok("/code/ws/app-Feature_X app-Feature_X")
        );
        assert_eq!(vars.map().len(), expected_task.len() + 6);
    }

    #[test]
    fn repo_and_missing_parts() {
        let none = BTreeMap::new();
        let vars = Vars::for_task(&task(), Path::new("/"), Some("tool"), &none)
            .with_branch("b", None)
            .with_worktree(Path::new("/"));
        assert_eq!(vars.map().get("repo").map(String::as_str), Some("tool"));
        assert_eq!(vars.map().get("project_name").map(String::as_str), Some(""));
        assert_eq!(
            vars.map().get("project_parent").map(String::as_str),
            Some("")
        );
        assert_eq!(
            vars.map().get("worktree_name").map(String::as_str),
            Some("")
        );
        assert_eq!(vars.map().get("port"), None);
        assert_eq!(
            vars.render("{{port}}"),
            Err(TemplateError::UnknownPlaceholder("port".into()))
        );
    }

    #[test]
    fn scopes_and_checks() {
        assert_eq!(builtins(Scope::Branch, true), TASK_VARS.to_vec());
        assert_eq!(
            builtins(Scope::Path, false),
            [TASK_VARS, BRANCH_VARS].concat()
        );
        assert_eq!(
            builtins(Scope::Path, true),
            [TASK_VARS, BRANCH_VARS, &["port"]].concat()
        );
        assert_eq!(
            builtins(Scope::Worktree, false),
            [TASK_VARS, BRANCH_VARS, WORKTREE_VARS].concat()
        );
        assert!(is_builtin("port") && is_builtin("worktree") && is_builtin("id"));
        assert!(!is_builtin("db_prefix"));

        let extra = BTreeMap::from([("db".to_owned(), String::new())]);
        assert_eq!(
            check("{{id}}-{{slug}}", Scope::Branch, false, &extra),
            Ok(())
        );
        assert_eq!(
            check("{{db}}_{{branch_db}}", Scope::Path, false, &extra),
            Ok(())
        );
        assert_eq!(check("{{port}}", Scope::Path, true, &extra), Ok(()));
        let err = check("{{branch}}", Scope::Branch, false, &extra).unwrap_err();
        assert_eq!(err.name, "branch");
        assert_eq!(
            err.to_string(),
            "unknown variable {{branch}}; known: {{db}}, {{id}}, {{project}}, {{project_name}}, {{project_parent}}, {{repo}}, {{slug}}, {{title}}"
        );
        assert_eq!(
            check("{{port}}", Scope::Path, false, &extra)
                .unwrap_err()
                .name,
            "port"
        );
        assert_eq!(
            check("{{#worktree}}x{{/worktree}}", Scope::Path, true, &extra)
                .unwrap_err()
                .name,
            "worktree"
        );
        assert_eq!(
            check("{{worktree}} {{zzz}}", Scope::Worktree, true, &extra)
                .unwrap_err()
                .name,
            "zzz"
        );
    }
}
