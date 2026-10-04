//! Launching a work session on a task: where it starts and the [`Launcher`]
//! extension point (plan Phase 4).
//!
//! The working directory follows FR-8: the first tracked worktree that
//! exists, else the task's `## Project`, else `work.default_project`.
//! [`resolve_workdir`] is pure (the filesystem is a closure) so the rules
//! are unit tested; the launchers that run `claude`, a shell, `tmux` or
//! `herdr` live in `tasq-launch`.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{Task, Worktree};

/// Environment variable carrying the task id into the session (the status
/// line and the wrap-up skill read it).
pub const ENV_TASK_ID: &str = "TASQ_TASK_ID";
/// Environment variable carrying the notebook into the session, so the
/// session's own `tasq` calls hit the same notebook even when the session
/// does not inherit the launching shell's environment (herdr panes).
pub const ENV_NOTEBOOK: &str = "TASQ_NOTEBOOK";

/// Everything a launcher needs to open a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchContext {
    /// The task, as stored when the session starts.
    pub task: Task,
    /// The task's file, named in the prompt.
    pub file: PathBuf,
    /// The task file's text, quoted in the prompt.
    pub markdown: String,
    /// Where the session starts.
    pub workdir: PathBuf,
    /// Whether `workdir` is a tracked worktree (as opposed to the project).
    pub in_worktree: bool,
    /// Variables to set in the session on top of its environment
    /// ([`ENV_TASK_ID`], [`ENV_NOTEBOOK`], the profile when selected).
    pub env: Vec<(String, String)>,
    /// Status names of the workflow, for the prompt.
    pub statuses: Vec<String>,
    /// Whether the user's attention should move to the new session. Only
    /// launchers that open another window (herdr, tmux) can leave it where
    /// it is (`false`); the ones that run in the current terminal ignore
    /// it.
    pub focus: bool,
}

/// Where a session should start, as decided by [`resolve_workdir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The directory.
    pub workdir: PathBuf,
    /// Whether it is a tracked worktree.
    pub in_worktree: bool,
    /// When every tracked worktree is gone: the newest one, which the CLI
    /// offers to recreate on its recorded branch (only interactively).
    pub missing_worktree: Option<Worktree>,
    /// Things the user should hear (a tracked project that no longer exists).
    pub warnings: Vec<String>,
}

/// Why a session cannot start.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LaunchError {
    /// Neither the task nor the config says where to work.
    #[error(
        "task {task} tracks no project and work.default_project is unset; set one with tasq project {task} <path>"
    )]
    NoWorkdir {
        /// The task id.
        task: String,
    },
    /// The chosen directory does not exist.
    #[error("workdir not found: {0}")]
    WorkdirMissing(PathBuf),
    /// The launcher cannot run here (tmux outside tmux, herdr without a server).
    #[error("{launcher}: {reason}")]
    Unavailable {
        /// Launcher name.
        launcher: String,
        /// Why.
        reason: String,
    },
    /// A program the launcher runs failed.
    #[error("{tool} failed: {message}")]
    Tool {
        /// The program.
        tool: String,
        /// Its message.
        message: String,
    },
    /// The prompt template could not be rendered.
    #[error("prompt template: {0}")]
    Template(String),
}

/// What a successful launch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// The process was replaced (`exec`); only seen when exec fails to happen.
    Replaced,
    /// The session opened elsewhere (a tmux window, a herdr workspace);
    /// `detail` says where.
    Opened(String),
}

/// Opens a work session.
pub trait Launcher {
    /// Launcher name (`shell`, `claude`, `tmux`, `herdr`).
    fn name(&self) -> &str;

    /// What [`launch`](Self::launch) would do, one line per step, for
    /// `--dry-run`.
    fn describe(&self, ctx: &LaunchContext) -> Result<Vec<String>, LaunchError>;

    /// Opens the session.
    fn launch(&self, ctx: &LaunchContext) -> Result<LaunchOutcome, LaunchError>;

    /// How to get back into a session this launcher started, when it can.
    fn resume_hint(&self, session_id: &str) -> Option<String>;
}

/// Decides where the session for `task` starts (FR-8). `is_dir` answers
/// for the filesystem so the rules can be tested without one.
pub fn resolve_workdir(
    task: &Task,
    default_project: Option<&Path>,
    is_dir: impl Fn(&Path) -> bool,
) -> Result<Resolution, LaunchError> {
    if let Some(worktree) = task.worktrees.iter().find(|w| is_dir(&w.path)) {
        return Ok(Resolution {
            workdir: worktree.path.clone(),
            in_worktree: true,
            missing_worktree: None,
            warnings: Vec::new(),
        });
    }
    let mut warnings = Vec::new();
    let project = match &task.project {
        Some(project) if is_dir(project) => Some(project.clone()),
        Some(project) => {
            warnings.push(format!(
                "tracked project not found: {}; using work.default_project",
                project.display()
            ));
            None
        }
        None => None,
    };
    let Some(workdir) = project.or_else(|| default_project.map(Path::to_path_buf)) else {
        return Err(LaunchError::NoWorkdir {
            task: task.id.to_string(),
        });
    };
    if !is_dir(&workdir) {
        return Err(LaunchError::WorkdirMissing(workdir));
    }
    Ok(Resolution {
        workdir,
        in_worktree: false,
        missing_worktree: task.worktrees.last().cloned(),
        warnings,
    })
}

/// A short workspace label from a task title, as the script's
/// `short_label`: leading `[tag]` groups and a `Review MR !n:` prefix are
/// dropped, the first three words kept, cut at 24 characters.
pub fn short_label(title: &str) -> String {
    let mut rest = strip_bracket_groups(title);
    if let Some(after) = rest.strip_prefix("Review MR")
        && let Some((_, tail)) = after.split_once(':')
    {
        rest = tail.trim_start();
    }
    let rest = strip_bracket_groups(rest);
    let words: Vec<&str> = rest.split_whitespace().take(3).collect();
    let joined = words.join(" ");
    let cut: String = joined.chars().take(24).collect();
    cut.trim_end().to_owned()
}

/// Drops every leading `[...]` group and the spaces after it.
fn strip_bracket_groups(text: &str) -> &str {
    let mut rest = text;
    while let Some(after) = rest.strip_prefix('[')
        && let Some(end) = after.find(']')
    {
        rest = after[end + 1..].trim_start_matches(' ');
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskId;

    fn task() -> Task {
        Task::new(TaskId::from(7), "T")
    }

    fn fs(existing: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| existing.iter().any(|e| Path::new(e) == p)
    }

    #[test]
    fn first_existing_worktree_wins() {
        let mut t = task();
        t.add_worktree(Worktree::on_branch("/wt/gone", "a"));
        t.add_worktree(Worktree::new("/wt/here"));
        t.project = Some("/proj".into());
        let r =
            resolve_workdir(&t, Some(Path::new("/default")), fs(&["/wt/here", "/proj"])).unwrap();
        assert_eq!(
            r,
            Resolution {
                workdir: "/wt/here".into(),
                in_worktree: true,
                missing_worktree: None,
                warnings: Vec::new(),
            }
        );
    }

    #[test]
    fn project_then_default_with_the_newest_missing_worktree_named() {
        let mut t = task();
        t.add_worktree(Worktree::on_branch("/wt/old", "old"));
        t.add_worktree(Worktree::on_branch("/wt/new", "new"));
        t.project = Some("/proj".into());
        let r =
            resolve_workdir(&t, Some(Path::new("/default")), fs(&["/proj", "/default"])).unwrap();
        assert_eq!(r.workdir, PathBuf::from("/proj"));
        assert!(!r.in_worktree);
        assert_eq!(
            r.missing_worktree,
            Some(Worktree::on_branch("/wt/new", "new"))
        );
        assert_eq!(r.warnings, Vec::<String>::new());

        // Project gone: warn and use the default.
        let r = resolve_workdir(&t, Some(Path::new("/default")), fs(&["/default"])).unwrap();
        assert_eq!(r.workdir, PathBuf::from("/default"));
        assert_eq!(
            r.warnings,
            vec!["tracked project not found: /proj; using work.default_project".to_owned()]
        );

        // No worktrees tracked at all: nothing to recreate.
        let plain = task();
        let r = resolve_workdir(&plain, Some(Path::new("/default")), fs(&["/default"])).unwrap();
        assert_eq!(r.missing_worktree, None);
        assert_eq!(r.workdir, PathBuf::from("/default"));
    }

    #[test]
    fn errors() {
        let t = task();
        assert_eq!(
            resolve_workdir(&t, None, fs(&[])),
            Err(LaunchError::NoWorkdir { task: "7".into() })
        );
        assert_eq!(
            resolve_workdir(&t, Some(Path::new("/default")), fs(&[])),
            Err(LaunchError::WorkdirMissing("/default".into()))
        );
        let mut with_project = task();
        with_project.project = Some("/proj".into());
        assert_eq!(
            resolve_workdir(&with_project, None, fs(&[])),
            Err(LaunchError::NoWorkdir { task: "7".into() })
        );
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            LaunchError::NoWorkdir { task: "7".into() }.to_string(),
            "task 7 tracks no project and work.default_project is unset; set one with tasq project 7 <path>"
        );
        assert_eq!(
            LaunchError::WorkdirMissing("/x".into()).to_string(),
            "workdir not found: /x"
        );
        assert_eq!(
            LaunchError::Unavailable {
                launcher: "tmux".into(),
                reason: "not inside tmux".into()
            }
            .to_string(),
            "tmux: not inside tmux"
        );
        assert_eq!(
            LaunchError::Tool {
                tool: "herdr".into(),
                message: "boom".into()
            }
            .to_string(),
            "herdr failed: boom"
        );
        assert_eq!(
            LaunchError::Template("unknown placeholder".into()).to_string(),
            "prompt template: unknown placeholder"
        );
    }

    #[test]
    fn short_labels_match_the_script() {
        assert_eq!(
            short_label("Fix the login form validation"),
            "Fix the login"
        );
        assert_eq!(
            short_label("[gitlab] [A] Fix the login form"),
            "Fix the login"
        );
        assert_eq!(
            short_label("Review MR !123: Add the parser module"),
            "Add the parser"
        );
        assert_eq!(
            short_label("[gitlab] Review MR !123: [WIP] Add parser"),
            "Add parser"
        );
        assert_eq!(
            short_label("Supercalifragilisticexpialidocious words here"),
            "Supercalifragilisticexpi"
        );
        assert_eq!(short_label("One"), "One");
        assert_eq!(
            short_label("  spaced   out   title   words"),
            "spaced out title"
        );
        assert_eq!(short_label(""), "");
        assert_eq!(short_label("[unclosed bracket"), "[unclosed bracket");
    }
}
