//! Worktree hooks (plan 4.5, WT-204): the command lines of
//! `[worktree.hooks]` made ready to run for one worktree, the environment
//! they run with.
//!
//! A line is split into words like a shell would (quotes group, no
//! expansion), then each word is rendered with the template variables, so
//! a value with spaces or quotes stays one argument. No shell runs it;
//! `sh -c '...'` is there for whoever needs one.

use std::path::{Path, PathBuf};

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

/// `name` in tasq's state directory: `$XDG_STATE_HOME/tasq`, else
/// `~/.local/state/tasq`; `None` when neither is known.
pub fn state_file(
    xdg_state_home: Option<&str>,
    home: Option<&Path>,
    name: &str,
) -> Option<PathBuf> {
    let state = match xdg_state_home.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home?.join(".local").join("state"),
    };
    Some(state.join("tasq").join(name))
}

/// What a hook command had to say while it ran: worktree hooks, and the
/// plugin hooks of the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    /// A command exited 0 with something on stdout (`-v` material).
    Output(String),
    /// A non-blocking command failed or could not start; the event it was
    /// attached to already happened, so this is a warning, not an error.
    Warning(String),
}

/// One hook line, rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The line as configured.
    pub line: String,
    /// The program and its arguments.
    pub argv: Vec<String>,
}

/// The hooks of one kind for one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hooks {
    /// When they run.
    pub kind: Kind,
    /// What runs.
    pub run: Vec<Command>,
}

impl Hooks {
    /// The lines of `kind` in `hooks`, rendered with `vars`.
    ///
    /// # Errors
    ///
    /// The [`HookError`] of the first line that cannot be rendered.
    pub fn render(kind: Kind, hooks: &WorktreeHooks, vars: &Vars) -> Result<Self, HookError> {
        let run = kind
            .lines(hooks)
            .iter()
            .map(|line| {
                Ok(Command {
                    line: line.clone(),
                    argv: argv(kind, line, vars)?,
                })
            })
            .collect::<Result<_, HookError>>()?;
        Ok(Self { kind, run })
    }

    /// `--dry-run` lines: what would run.
    pub fn describe(&self) -> Vec<String> {
        let key = self.kind.key();
        self.run
            .iter()
            .map(|c| format!("run {key} hook: {}", shell_words::join(&c.argv)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Task, TaskId};
    use std::collections::BTreeMap;

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
    fn the_state_directory() {
        let home = Path::new("/home/me");
        assert_eq!(
            state_file(Some("/state"), Some(home), "previous-worktree"),
            Some(PathBuf::from("/state/tasq/previous-worktree"))
        );
        assert_eq!(
            state_file(Some(""), Some(home), "previous-worktree"),
            Some(home.join(".local/state/tasq/previous-worktree"))
        );
        assert_eq!(
            state_file(None, Some(home), "previous-worktree"),
            Some(home.join(".local/state/tasq/previous-worktree"))
        );
        assert_eq!(state_file(None, None, "previous-worktree"), None);
    }

    #[test]
    fn hooks_render_their_lines() {
        let hooks = WorktreeHooks {
            post_create: vec!["echo '{{branch}}'".into(), "make setup".into()],
            ..WorktreeHooks::default()
        };
        let rendered = Hooks::render(Kind::PostCreate, &hooks, &vars()).unwrap();
        assert_eq!(
            rendered,
            Hooks {
                kind: Kind::PostCreate,
                run: vec![
                    Command {
                        line: "echo '{{branch}}'".into(),
                        argv: vec!["echo".into(), "feat/x".into()],
                    },
                    Command {
                        line: "make setup".into(),
                        argv: vec!["make".into(), "setup".into()],
                    }
                ],
            }
        );
        assert_eq!(
            rendered.describe(),
            [
                "run post-create hook: echo feat/x",
                "run post-create hook: make setup"
            ]
        );
        let other = Hooks::render(Kind::PreCreate, &hooks, &vars()).unwrap();
        assert_eq!(other.run, Vec::new());
        assert_eq!(other.describe(), Vec::<String>::new());
    }

    #[test]
    fn a_bad_line_fails_to_render() {
        let hooks = WorktreeHooks {
            pre_remove: vec!["ok".into(), "echo {{nope}}".into()],
            ..WorktreeHooks::default()
        };
        let failed = Hooks::render(Kind::PreRemove, &hooks, &vars()).unwrap_err();
        assert_eq!(failed.line, "echo {{nope}}");
    }
}
