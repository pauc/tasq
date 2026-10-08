//! `tasq create`: a [`TaskDraft`] from the flags, written by the store.

use std::path::Path;
use tasq_core::dates::parse_day;

use tasq_core::model::{Link, Priority, Status, Tag, Task, TaskDraft, Workflow};
use tasq_core::store::Store;
use tasq_core::worktree::{declaration, lifecycle};
use tasq_store_nb::NbStore;

use crate::app::App;
use crate::cli::CreateArgs;
use crate::commands::{mr, worktree};
use crate::error::{CliError, Result};
use crate::json;
use crate::plugins::{self, Hook};

/// The initial status: a workflow status, or `done` for a closed task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialStatus {
    /// Open with this status.
    Open(Status),
    /// Created already closed (`# [x]`, no status tag).
    Done,
}

impl InitialStatus {
    /// Interprets `--status`; `None` means the workflow default.
    pub fn parse(value: Option<&str>, workflow: &Workflow, default: &Status) -> Result<Self> {
        match value {
            None => Ok(Self::Open(default.clone())),
            Some("done") => Ok(Self::Done),
            Some(value) => workflow.parse_status(value).map(Self::Open).ok_or_else(|| {
                CliError::user(format!(
                    "unknown status '{value}' (statuses: {}, done)",
                    workflow
                        .statuses
                        .iter()
                        .map(Status::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
            }),
        }
    }
}

/// Runs `create`.
pub fn run(app: &App, args: &CreateArgs) -> Result<()> {
    let workflow = app.workflow();
    let clock = app.clock()?;
    let cwd = crate::commands::existing_dir(&app.opts.cwd).unwrap_or_else(|| app.opts.cwd.clone());
    let mut draft = draft_from(
        args,
        &workflow,
        &app.config().workflow.default_status,
        clock.today(),
        &cwd,
    )?;
    for url in &args.mr {
        draft = draft.with_merge_request(mr::resolve_link(app, url, None)?);
    }
    let asked = worktree_flag(app, args)?;
    let closed = draft.done;
    let mut store = app.open_store()?;
    let task = store.create(draft)?;
    let (task, declared) = declare_worktree(app, &mut store, task, asked)?;
    plugins::run_hooks(app, Hook::PostCreate, &task, &[])?;
    if app.out.json_mode() {
        return app
            .out
            .json(&json::document([("task", app.task_json(&task))]));
    }
    let tail = if closed {
        format!("done, {}", task.priority.to_hash())
    } else {
        format!(
            "{} {}",
            task.status
                .as_ref()
                .map(Status::to_hash)
                .unwrap_or_default(),
            task.priority.to_hash()
        )
    };
    app.out
        .print(&format!("[{}] created: {} ({tail})\n", task.id, task.title))?;
    match declared {
        Some(line) => app
            .out
            .print(&format!("[{}] worktree declared: {line}\n", task.id)),
        None => Ok(()),
    }
}

/// What `--worktree [BRANCH]` asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked<'a> {
    /// No flag: `worktree.on_create` decides.
    No,
    /// `--worktree`: on the default branch.
    Default,
    /// `--worktree BRANCH`.
    Branch(&'a str),
}

/// Reads `--worktree`, checked before anything is written.
fn worktree_flag<'a>(app: &App, args: &'a CreateArgs) -> Result<Asked<'a>> {
    let Some(flag) = &args.worktree else {
        return Ok(Asked::No);
    };
    if !app.worktrees() {
        return Err(worktree::disabled());
    }
    match flag.as_deref() {
        None => Ok(Asked::Default),
        Some(branch) => lifecycle::branch_name(branch)
            .map(Asked::Branch)
            .map_err(|e| CliError::user(e.to_string())),
    }
}

/// Declares the new task's worktree when `--worktree` was given (`asked`)
/// or `worktree.on_create = "auto"` finds its project in a git work tree;
/// the task as stored, and the declared worktree for the message. Reads
/// the filesystem only, runs no process.
fn declare_worktree(
    app: &App,
    store: &mut NbStore,
    task: Task,
    asked: Asked,
) -> Result<(Task, Option<String>)> {
    let Some(project) = task.project.clone().filter(|_| app.worktrees()) else {
        return Ok((task, None));
    };
    let repo = worktree::Repo::of(&project);
    let on_create = app.config().worktree.on_create;
    if !declaration::wanted(on_create, asked != Asked::No, repo.is_some()) {
        return Ok((task, None));
    }
    let branch = match asked {
        Asked::Branch(branch) => Some(branch),
        Asked::No | Asked::Default => None,
    };
    let id = task.id.clone();
    let failed =
        |e: CliError| CliError::user(format!("[{id}] created, but no worktree was declared: {e}"));
    let (declared, _) =
        worktree::declare_in(app, &task, branch, &project, repo.as_ref()).map_err(failed)?;
    let mut task = task;
    task.add_worktree(declared.clone());
    store.update(&task)?;
    Ok((store.get(&id)?, Some(worktree::describe(&declared))))
}

/// The draft for `args`, everything but merge requests (they need the
/// title lookup, see [`run`]). The project is `--project`, else `cwd`:
/// every task gets one, so `pick` always has somewhere to start.
pub fn draft_from(
    args: &CreateArgs,
    workflow: &Workflow,
    default_status: &Status,
    today: chrono::NaiveDate,
    cwd: &Path,
) -> Result<TaskDraft> {
    let title = args.title.trim();
    if title.is_empty() {
        return Err(CliError::user("the title must not be empty"));
    }
    let mut draft = TaskDraft::new(title);
    match InitialStatus::parse(args.status.as_deref(), workflow, default_status)? {
        InitialStatus::Open(status) => draft = draft.with_status(Some(status)),
        InitialStatus::Done => draft = draft.with_status(None).with_done(true),
    }
    if let Some(prio) = &args.prio {
        let priority = prio
            .parse::<Priority>()
            .map_err(|_| CliError::user("priority must be A, B or C"))?;
        draft = draft.with_priority(priority);
    }
    if let Some(due) = &args.due {
        draft = draft.with_due(parse_day(due, today)?);
    }
    if let Some(desc) = &args.desc {
        draft = draft.with_description(desc.as_str());
    }
    let project = match &args.project {
        Some(project) => crate::commands::existing_dir(project).ok_or_else(|| {
            CliError::user(format!("project path not found: {}", project.display()))
        })?,
        None => cwd.to_path_buf(),
    };
    draft = draft.with_project(project);
    for tag in &args.tag {
        draft = draft.with_tag(tag.parse::<Tag>()?);
    }
    for url in &args.related {
        draft = draft.with_related(Link::new(url.as_str()));
    }
    if let Some(note) = &args.note {
        draft = draft.with_note(note.as_str());
    }
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 4).unwrap()
    }

    fn args(title: &str) -> CreateArgs {
        CreateArgs {
            title: title.to_owned(),
            ..CreateArgs::default()
        }
    }

    #[test]
    fn defaults_follow_the_workflow() {
        let wf = Workflow::default();
        let d = draft_from(
            &args("  T  "),
            &wf,
            &Status::LATER,
            today(),
            Path::new("/cwd"),
        )
        .unwrap();
        assert_eq!(d.title, "T");
        assert_eq!(d.status, Some(Status::LATER));
        assert_eq!(
            d.project.as_deref(),
            Some(Path::new("/cwd")),
            "the current directory"
        );
        assert_eq!(d.priority, Priority::B);
        assert!(!d.done);
        assert_eq!(d.note, None);
    }

    #[test]
    fn every_flag() {
        let wf = Workflow::default();
        let dir = tempfile::tempdir().unwrap();
        let a = CreateArgs {
            title: "T".into(),
            desc: Some("D".into()),
            status: Some("blocked".into()),
            prio: Some("#A".into()),
            due: Some("tomorrow".into()),
            project: Some(dir.path().to_path_buf()),
            tag: vec!["#x".into(), "y".into()],
            related: vec!["https://r".into()],
            mr: vec!["https://g/p/-/merge_requests/1".into()],
            note: Some("hello".into()),
            worktree: None,
        };
        let d = draft_from(&a, &wf, &Status::READY, today(), Path::new("/cwd")).unwrap();
        assert_eq!(d.description.as_deref(), Some("D"));
        assert_eq!(d.status, Some(Status::BLOCKED));
        assert_eq!(d.priority, Priority::A);
        assert_eq!(d.due, NaiveDate::from_ymd_opt(2026, 10, 5));
        assert_eq!(d.project, Some(dir.path().canonicalize().unwrap()));
        assert_eq!(d.tags, vec![Tag::new("x").unwrap(), Tag::new("y").unwrap()]);
        assert_eq!(d.related, vec![Link::new("https://r")]);
        assert_eq!(
            d.merge_requests,
            Vec::new(),
            "merge requests are added by run"
        );
        assert_eq!(d.note.as_deref(), Some("hello"));
    }

    #[test]
    fn done_status() {
        let wf = Workflow::default();
        let a = CreateArgs {
            status: Some("done".into()),
            ..args("T")
        };
        let d = draft_from(&a, &wf, &Status::READY, today(), Path::new("/cwd")).unwrap();
        assert!(d.done);
        assert_eq!(d.status, None);
    }

    #[test]
    fn errors() {
        let wf = Workflow::default();
        let err = |a: CreateArgs| {
            draft_from(&a, &wf, &Status::READY, today(), Path::new("/cwd"))
                .unwrap_err()
                .to_string()
        };
        assert_eq!(err(args("  ")), "the title must not be empty");
        assert_eq!(
            err(CreateArgs {
                status: Some("nope".into()),
                ..args("T")
            }),
            "unknown status 'nope' (statuses: in-progress ready waiting blocked later, done)"
        );
        assert_eq!(
            err(CreateArgs {
                prio: Some("D".into()),
                ..args("T")
            }),
            "priority must be A, B or C"
        );
        assert_eq!(
            err(CreateArgs {
                due: Some("someday".into()),
                ..args("T")
            }),
            "invalid date or timestamp \"someday\" (expected YYYY-MM-DD or YYYY-MM-DD HH:MM)"
        );
        assert_eq!(
            err(CreateArgs {
                project: Some("/definitely/not/here".into()),
                ..args("T")
            }),
            "project path not found: /definitely/not/here"
        );
        assert!(
            err(CreateArgs {
                tag: vec!["##x".into()],
                ..args("T")
            })
            .contains("tag")
        );
    }
}
