//! Worktree hooks (plan 4.5, WT-204): the command lines of
//! `[worktree.hooks]` made ready to run for one worktree, the environment
//! they run with, and the store of the ones the user approved.
//!
//! A line is split into words like a shell would (quotes group, no
//! expansion), then each word is rendered with the template variables, so
//! a value with spaces or quotes stays one argument. No shell runs it;
//! `sh -c '...'` is there for whoever needs one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::WorktreeHooks;

use super::vars::Vars;

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Before `git worktree add`, in the main checkout; a failure aborts.
    PreCreate,
    /// After the worktree is provisioned, inside it; a failure warns.
    PostCreate,
    /// Before `git worktree remove`, inside it; a failure keeps it.
    PreRemove,
    /// After the worktree is removed, in the main checkout; a failure warns.
    PostRemove,
}

impl Kind {
    /// Every kind, in lifecycle order.
    pub const ALL: [Kind; 4] = [
        Kind::PreCreate,
        Kind::PostCreate,
        Kind::PreRemove,
        Kind::PostRemove,
    ];

    /// The key under `[worktree.hooks]`.
    pub fn key(self) -> &'static str {
        match self {
            Self::PreCreate => "pre-create",
            Self::PostCreate => "post-create",
            Self::PreRemove => "pre-remove",
            Self::PostRemove => "post-remove",
        }
    }

    /// Whether a failure stops the step it belongs to (the `pre-*` hooks).
    pub fn blocking(self) -> bool {
        matches!(self, Self::PreCreate | Self::PreRemove)
    }

    /// The configured command lines of this kind.
    pub fn lines(self, hooks: &WorktreeHooks) -> &[String] {
        match self {
            Self::PreCreate => &hooks.pre_create,
            Self::PostCreate => &hooks.post_create,
            Self::PreRemove => &hooks.pre_remove,
            Self::PostRemove => &hooks.post_remove,
        }
    }
}

/// A hook line that cannot be run.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("worktree.hooks.{key} {line:?}: {message}")]
pub struct HookError {
    /// `pre-create`, ...
    pub key: &'static str,
    /// The line as configured.
    pub line: String,
    /// What is wrong with it.
    pub message: String,
}

/// The program and arguments of `line`, each word rendered with `vars`.
///
/// # Errors
///
/// [`HookError`] for an unbalanced quote, an empty line or an unknown
/// `{{variable}}`.
pub fn argv(kind: Kind, line: &str, vars: &Vars) -> Result<Vec<String>, HookError> {
    let failed = |message: String| HookError {
        key: kind.key(),
        line: line.to_owned(),
        message,
    };
    let words = shell_words::split(line).map_err(|e| failed(e.to_string()))?;
    if words.is_empty() {
        return Err(failed("empty command".to_owned()));
    }
    words
        .iter()
        .map(|word| vars.render(word).map_err(|e| failed(e.to_string())))
        .collect()
}

/// The environment a hook gets on top of the user's: `TASQ_WORKTREE`,
/// `TASQ_BRANCH`, `TASQ_TASK_ID`, and every variable as `TASQ_VAR_<NAME>`
/// (upper case; variable names are `[a-z0-9_]` already).
pub fn env(vars: &Vars) -> Vec<(String, String)> {
    let map = vars.map();
    let mut env: Vec<(String, String)> = [
        ("TASQ_WORKTREE", "worktree"),
        ("TASQ_BRANCH", "branch"),
        ("TASQ_TASK_ID", "id"),
    ]
    .into_iter()
    .filter_map(|(name, var)| map.get(var).map(|v| (name.to_owned(), v.clone())))
    .collect();
    env.extend(map.iter().map(|(name, value)| {
        (
            format!("TASQ_VAR_{}", name.to_ascii_uppercase()),
            value.clone(),
        )
    }));
    env
}

/// Where approvals are kept: `$XDG_STATE_HOME/tasq/trust.toml`, else
/// `~/.local/state/tasq/trust.toml`; `None` when neither is known.
pub fn trust_file(xdg_state_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    let state = match xdg_state_home.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home?.join(".local").join("state"),
    };
    Some(state.join("tasq").join("trust.toml"))
}

/// The hook command lines the user approved, per configuration file. A
/// line is approved as written, so editing it asks again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustStore {
    /// Approved lines, by the absolute path of the file that holds them.
    #[serde(default)]
    approved: BTreeMap<PathBuf, Vec<String>>,
}

impl TrustStore {
    /// Reads a store written by [`TrustStore::to_toml`].
    ///
    /// # Errors
    ///
    /// The TOML error for a malformed file.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// The store as TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// The lines of `lines` not approved for `file`, in order.
    pub fn pending<'a>(&self, file: &Path, lines: &'a [String]) -> Vec<&'a str> {
        let approved = self.approved.get(file);
        lines
            .iter()
            .filter(|line| !approved.is_some_and(|a| a.contains(line)))
            .map(String::as_str)
            .collect()
    }

    /// Approves `lines` for `file`, keeping earlier approvals.
    pub fn approve(&mut self, file: &Path, lines: &[String]) {
        let approved = self.approved.entry(file.to_path_buf()).or_default();
        for line in lines {
            if !approved.contains(line) {
                approved.push(line.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Task, TaskId};

    fn vars() -> Vars {
        let extra = [("db_prefix".to_owned(), "shop".to_owned())].into();
        Vars::for_task(
            &Task::new(TaskId::from(7), "Fix 'it' now"),
            Path::new("/ws/app"),
            None,
            &extra,
        )
        .with_branch("feat/x", None)
        .with_worktree(Path::new("/ws/app-feat_x"))
    }

    #[test]
    fn kinds() {
        assert_eq!(
            Kind::ALL.map(Kind::key),
            ["pre-create", "post-create", "pre-remove", "post-remove"]
        );
        assert_eq!(Kind::ALL.map(Kind::blocking), [true, false, true, false]);
        let hooks = WorktreeHooks {
            pre_create: vec!["a".into()],
            post_create: vec!["b".into()],
            pre_remove: vec!["c".into()],
            post_remove: vec!["d".into()],
        };
        assert_eq!(
            Kind::ALL.map(|k| k.lines(&hooks)[0].clone()),
            ["a", "b", "c", "d"]
        );
    }

    #[test]
    fn words_are_split_then_rendered() {
        assert_eq!(
            argv(
                Kind::PostRemove,
                "dropdb '{{db_prefix}}_{{branch_db}}' --if-exists",
                &vars()
            ),
            Ok(vec![
                "dropdb".to_owned(),
                "shop_feat_x".to_owned(),
                "--if-exists".to_owned()
            ])
        );
        assert_eq!(
            argv(Kind::PostCreate, "notify {{title}}", &vars()),
            Ok(vec!["notify".to_owned(), "Fix 'it' now".to_owned()]),
            "a value stays one word"
        );
    }

    #[test]
    fn bad_lines() {
        let err = |line: &str| argv(Kind::PreCreate, line, &vars()).unwrap_err();
        assert_eq!(
            err("echo {{nope}}"),
            HookError {
                key: "pre-create",
                line: "echo {{nope}}".into(),
                message: "unknown placeholder {{nope}}".into()
            }
        );
        assert_eq!(err("  ").message, "empty command");
        assert_eq!(err("echo 'open").key, "pre-create");
        assert_eq!(
            err("  ").to_string(),
            "worktree.hooks.pre-create \"  \": empty command"
        );
    }

    #[test]
    fn the_environment() {
        let env = env(&vars());
        let get = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        assert_eq!(get("TASQ_WORKTREE"), Some("/ws/app-feat_x"));
        assert_eq!(get("TASQ_BRANCH"), Some("feat/x"));
        assert_eq!(get("TASQ_TASK_ID"), Some("7"));
        assert_eq!(get("TASQ_VAR_DB_PREFIX"), Some("shop"));
        assert_eq!(get("TASQ_VAR_BRANCH_SAFE"), Some("feat_x"));
        assert_eq!(get("TASQ_VAR_PROJECT_NAME"), Some("app"));
        assert_eq!(
            &env[..3].iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            &["TASQ_WORKTREE", "TASQ_BRANCH", "TASQ_TASK_ID"]
        );
        // Before the worktree and branch are known, only what is.
        let early = super::env(&Vars::for_task(
            &Task::new(TaskId::from(1), "T"),
            Path::new("/p"),
            None,
            &BTreeMap::new(),
        ));
        assert_eq!(early[0], ("TASQ_TASK_ID".to_owned(), "1".to_owned()));
    }

    #[test]
    fn where_approvals_live() {
        let home = Path::new("/home/me");
        assert_eq!(
            trust_file(Some("/state"), Some(home)),
            Some(PathBuf::from("/state/tasq/trust.toml"))
        );
        assert_eq!(
            trust_file(Some(""), Some(home)),
            Some(PathBuf::from("/home/me/.local/state/tasq/trust.toml"))
        );
        assert_eq!(
            trust_file(None, Some(home)),
            Some(PathBuf::from("/home/me/.local/state/tasq/trust.toml"))
        );
        assert_eq!(trust_file(None, None), None);
    }

    #[test]
    fn approvals_are_per_file_and_per_line() {
        let file = Path::new("/ws/.tasq.toml");
        let lines = vec!["direnv allow .".to_owned(), "make setup".to_owned()];
        let mut store = TrustStore::default();
        assert_eq!(
            store.pending(file, &lines),
            ["direnv allow .", "make setup"]
        );
        store.approve(file, &lines[..1]);
        store.approve(file, &lines[..1]);
        assert_eq!(store.pending(file, &lines), ["make setup"]);
        assert_eq!(
            store.pending(Path::new("/other/.tasq.toml"), &lines).len(),
            2,
            "another file asks again"
        );
        let edited = vec!["direnv allow . && rm -rf ~".to_owned()];
        assert_eq!(
            store.pending(file, &edited).len(),
            1,
            "an edited line asks again"
        );
        store.approve(file, &lines);
        assert_eq!(store.pending(file, &lines), Vec::<&str>::new());
        let text = store.to_toml();
        assert_eq!(
            text,
            "[approved]\n\"/ws/.tasq.toml\" = [\"direnv allow .\", \"make setup\"]\n"
        );
        assert_eq!(TrustStore::parse(&text), Ok(store));
        assert_eq!(TrustStore::parse(""), Ok(TrustStore::default()));
        assert!(TrustStore::parse("other = 1").is_err());
    }
}
