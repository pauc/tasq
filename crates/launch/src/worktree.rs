//! Worktree managers: plain `git worktree add`, or a user-configured command
//! (`work.worktree_command`) for tools that also provision the new worktree,
//! such as gwm.
//!
//! Both decide whether the branch is new the way the script did: it exists
//! when `git show-ref` finds it locally or `git ls-remote` finds it on
//! `origin`; otherwise it is created (`-b` for git, `{new}` for the command).

use std::path::{Path, PathBuf};

use tasq_core::config::WorktreeManager as Kind;
use tasq_core::work::{CreatedWorktree, WorkError, WorktreeManager, sibling_worktree};

use crate::process::run;

/// Whether `branch` exists in the repository at `project`, locally or on
/// `origin`.
pub fn branch_exists(project: &Path, branch: &str, env: &[(String, String)]) -> bool {
    let local = run(
        "git",
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
        Some(project),
        env,
        &[],
    );
    if local.success {
        return true;
    }
    run(
        "git",
        &["ls-remote", "--exit-code", "--heads", "origin", branch],
        Some(project),
        env,
        &[],
    )
    .success
}

/// The branch checked out at `dir` (`git branch --show-current`), `None`
/// when it is not a repository, is detached or git is unavailable.
pub fn current_branch(dir: &Path, env: &[(String, String)]) -> Option<String> {
    let out = run("git", &["branch", "--show-current"], Some(dir), env, &[]);
    if !out.success {
        return None;
    }
    let branch = out.stdout.trim();
    (!branch.is_empty()).then(|| branch.to_owned())
}

/// The manager `work.worktree_manager` asks for; `command` needs
/// `work.worktree_command`.
pub fn manager_for(
    kind: Kind,
    command: Option<&str>,
    env: Vec<(String, String)>,
) -> Result<Box<dyn WorktreeManager>, WorkError> {
    match kind {
        Kind::Git => Ok(Box::new(GitManager { env })),
        Kind::Command => {
            let template = command
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .ok_or(WorkError::CommandMissing)?;
            Ok(Box::new(CommandManager {
                template: template.to_owned(),
                env,
            }))
        }
    }
}

/// Fills a `work.worktree_command` template and splits it into argv.
///
/// `{branch}` and `{project}` are substituted everywhere; `{new}` becomes
/// `-b` when `new_branch` is set and nothing otherwise, and `{new:<text>}`
/// substitutes `<text>` instead of `-b`. Splitting follows shell quoting
/// rules (`shell-words`) without running a shell.
pub fn expand_template(
    template: &str,
    branch: &str,
    project: &Path,
    new_branch: bool,
) -> Result<Vec<String>, WorkError> {
    let bad = |reason: String| WorkError::BadCommand {
        template: template.to_owned(),
        reason,
    };
    let mut text = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        text.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(bad("unclosed '{'".to_owned()));
        };
        let name = &after[..end];
        match name {
            "branch" => text.push_str(branch),
            "project" => text.push_str(&project.display().to_string()),
            "new" => {
                if new_branch {
                    text.push_str("-b");
                }
            }
            flag if flag.starts_with("new:") => {
                if new_branch {
                    text.push_str(&flag["new:".len()..]);
                }
            }
            other => return Err(bad(format!("unknown placeholder {{{other}}}"))),
        }
        rest = &after[end + 1..];
    }
    text.push_str(rest);
    let argv = shell_words::split(&text).map_err(|e| bad(e.to_string()))?;
    if argv.is_empty() {
        return Err(bad("expands to nothing".to_owned()));
    }
    Ok(argv)
}

/// Runs `work.worktree_command` (see [`expand_template`]) inside the
/// project; the last non-empty line of its output is the worktree path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandManager {
    /// The configured template.
    pub template: String,
    /// Environment the processes run with.
    pub env: Vec<(String, String)>,
}

impl WorktreeManager for CommandManager {
    fn name(&self) -> &'static str {
        "command"
    }

    fn create(&self, project: &Path, branch: &str) -> Result<CreatedWorktree, WorkError> {
        if !project.is_dir() {
            return Err(WorkError::ProjectMissing(project.to_path_buf()));
        }
        let new_branch = !branch_exists(project, branch, &self.env);
        let argv = expand_template(&self.template, branch, project, new_branch)?;
        let tail: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        let out = run(&argv[0], &tail, Some(project), &self.env, &[]);
        if !out.success {
            return Err(WorkError::Tool {
                tool: argv[0].clone(),
                branch: branch.to_owned(),
                message: out.message(),
            });
        }
        let mut lines: Vec<&str> = out
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        let path = lines.pop().map(|l| PathBuf::from(l.trim()));
        match path {
            Some(path) if path.is_dir() => Ok(CreatedWorktree {
                path,
                branch: branch.to_owned(),
                messages: lines.iter().map(|l| (*l).to_owned()).collect(),
            }),
            _ => Err(WorkError::NoPath {
                tool: argv[0].clone(),
                output: out.stdout.trim().to_owned(),
            }),
        }
    }
}

/// `git worktree add` into `<project>-<branch>` next to the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitManager {
    /// Environment the processes run with.
    pub env: Vec<(String, String)>,
}

impl WorktreeManager for GitManager {
    fn name(&self) -> &'static str {
        "git"
    }

    fn create(&self, project: &Path, branch: &str) -> Result<CreatedWorktree, WorkError> {
        if !project.is_dir() {
            return Err(WorkError::ProjectMissing(project.to_path_buf()));
        }
        let path = sibling_worktree(project, branch);
        if path.is_dir() {
            return Ok(CreatedWorktree {
                path: path.clone(),
                branch: branch.to_owned(),
                messages: vec![format!("reusing existing worktree {}", path.display())],
            });
        }
        let path_text = path.display().to_string();
        let args: Vec<&str> = if branch_exists(project, branch, &self.env) {
            vec!["worktree", "add", &path_text, branch]
        } else {
            vec!["worktree", "add", "-b", branch, &path_text]
        };
        let out = run("git", &args, Some(project), &self.env, &[]);
        if !out.success {
            return Err(WorkError::Tool {
                tool: "git".to_owned(),
                branch: branch.to_owned(),
                message: out.message(),
            });
        }
        Ok(CreatedWorktree {
            path,
            branch: branch.to_owned(),
            messages: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "/code/app";

    #[test]
    fn template_substitution() {
        let p = Path::new(P);
        assert_eq!(
            expand_template("gwm create {new} {branch} --no-tmux -s", "f/x", p, true).unwrap(),
            vec!["gwm", "create", "-b", "f/x", "--no-tmux", "-s"]
        );
        assert_eq!(
            expand_template("gwm create {new} {branch} --no-tmux -s", "f/x", p, false).unwrap(),
            vec!["gwm", "create", "f/x", "--no-tmux", "-s"]
        );
        assert_eq!(
            expand_template("mk {new:--fresh} '{project}' \"{branch} \"", "b", p, true).unwrap(),
            vec!["mk", "--fresh", P, "b "]
        );
        assert_eq!(
            expand_template("mk {new:--fresh} {project}", "b", p, false).unwrap(),
            vec!["mk", P]
        );
        assert_eq!(
            expand_template("plain command", "b", p, true).unwrap(),
            vec!["plain", "command"]
        );
    }

    #[test]
    fn template_errors() {
        let p = Path::new(P);
        let reason = |t: &str| match expand_template(t, "b", p, true).unwrap_err() {
            WorkError::BadCommand { template, reason } => {
                assert_eq!(template, t);
                reason
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(reason("mk {branch"), "unclosed '{'");
        assert_eq!(reason("mk {nope}"), "unknown placeholder {nope}");
        assert_eq!(reason("{new:}"), "expands to nothing");
        assert_eq!(reason("   "), "expands to nothing");
        assert!(reason("mk 'unbalanced").contains("quote"));
    }

    #[test]
    fn manager_selection() {
        assert_eq!(
            manager_for(Kind::Git, None, Vec::new()).unwrap().name(),
            "git"
        );
        assert_eq!(
            manager_for(Kind::Command, Some(" mk {branch} "), Vec::new())
                .unwrap()
                .name(),
            "command"
        );
        assert!(matches!(
            manager_for(Kind::Command, None, Vec::new())
                .map(|m| m.name().to_owned())
                .unwrap_err(),
            WorkError::CommandMissing
        ));
        assert!(matches!(
            manager_for(Kind::Command, Some("  "), Vec::new())
                .map(|m| m.name().to_owned())
                .unwrap_err(),
            WorkError::CommandMissing
        ));
    }
}
