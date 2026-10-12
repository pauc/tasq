//! `tasq worktree <id> [BRANCH|PATH]`: declare a worktree or track an
//! existing one; `tasq worktree up <id>` creates the declared one now, as a
//! launch does; `--create <branch>` (hidden) is "declare, then up";
//! `tasq worktree release <id>` does what closing the task does to its
//! worktrees ([`release`], also run by `tasq done` and the TUI's `d`).

use std::path::{Path, PathBuf};

use crate::commands::worktree_hooks::{self, Approval, Hooks};
use crate::commands::worktree_nav;
use tasq_core::config::{OnDone, WorktreeConfig, project_file};
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::store::Store;
use tasq_core::work;
use tasq_core::worktree::hooks::{self, Kind};
use tasq_core::worktree::integration::Safety;
use tasq_core::worktree::lifecycle::{
    self, BranchFate, BranchSeen, BranchState, Declared, KeepBranch, KeepWorktree, LaunchAction,
    WorktreeFate,
};
use tasq_core::worktree::vars::Vars;
use tasq_core::worktree::{declaration, git, provision, status, vars};
use tasq_store_nb::NbStore;
use tasq_worktree::provision::{apply, prepare};
use tasq_worktree::{Git, describe as describe_start};

use crate::app::App;
use crate::cli::{Command, WorktreeAction};
use crate::commands::{existing_dir, finish};
use crate::error::{CliError, Result};
use crate::json;
use crate::plugins::HookEvent;

/// `tasq worktree`: a subcommand, else declare or track (`run`).
pub fn dispatch(app: &App, command: &Command) -> Result<()> {
    let Command::Worktree {
        action,
        id,
        target,
        create,
    } = command
    else {
        unreachable!("dispatch is only called for the worktree command");
    };
    match action {
        Some(WorktreeAction::Up {
            id,
            from,
            dry_run,
            yes,
        }) => up(app, id, from.as_deref(), *yes, *dry_run),
        Some(WorktreeAction::Trust { dir }) => trust(app, dir.as_deref()),
        Some(WorktreeAction::Ls { id, all }) => worktree_nav::ls(app, id.as_deref(), *all),
        Some(WorktreeAction::Switch { target, yes }) => {
            worktree_nav::switch(app, target.as_deref(), *yes)
        }
        Some(WorktreeAction::Path { target }) => worktree_nav::path(app, target),
        Some(WorktreeAction::ShellInit { shell }) => worktree_nav::shell_init(app, *shell),
        Some(WorktreeAction::Release {
            id,
            force,
            dry_run,
            yes,
        }) => release_command(app, id, *force, *yes, *dry_run),
        Some(WorktreeAction::Prune { all, dry_run, yes }) => {
            worktree_nav::prune(app, *all, *yes, *dry_run)
        }
        None => run(
            app,
            id.as_deref().unwrap_or_default(),
            target.as_deref(),
            create.as_deref(),
        ),
    }
}

/// Declares a worktree on `target` (a branch, or the default without one),
/// tracks `target` when it is a path, or creates a worktree for `create`.
/// Refused when worktrees are disabled (ADR 0022).
pub fn run(app: &App, id: &str, target: Option<&str>, create: Option<&str>) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let mut store = app.open_store()?;
    if let Some(branch) = create {
        return create_and_track(app, &mut store, &id, branch);
    }
    match target {
        Some(arg) if declaration::is_path(arg, Path::new(arg).is_dir()) => {
            track(app, &mut store, &id, Path::new(arg))
        }
        branch => {
            let mut task = store.get(&id)?;
            let project = project_of(app, &task)?;
            let (worktree, new) = declare(app, &task, branch, &project)?;
            if new {
                task.add_worktree(worktree.clone());
                store.update(&task)?;
            }
            let already = if new { "" } else { "already " };
            finish(
                app,
                &store,
                &id,
                &format!(
                    "[{id}] worktree {already}declared: {}\n",
                    describe(&worktree)
                ),
            )
        }
    }
}

/// The error for a worktree command or flag while the feature is off.
pub fn disabled() -> CliError {
    CliError::user(
        "worktrees are disabled; set worktree.enabled = true (docs/config.md, \"Worktrees\")",
    )
}

/// `path (branch)`, or the path alone when no branch is recorded.
pub fn describe(worktree: &Worktree) -> String {
    let path = worktree.path.display();
    worktree
        .branch
        .as_ref()
        .map_or_else(|| path.to_string(), |b| format!("{path} ({b})"))
}

/// The task's project, else `work.default_project`.
pub fn project_of(app: &App, task: &Task) -> Result<PathBuf> {
    work::project_dir(task, app.config().work.default_project.as_deref()).ok_or_else(|| {
        let id = &task.id;
        CliError::user(format!(
            "task {id} tracks no project and work.default_project is unset; set one with tasq project {id} <path>"
        ))
    })
}

/// Declares a worktree for `task` from `project` on `branch` (the
/// configured default without one); also whether it is new.
pub fn declare(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    project: &Path,
) -> Result<(Worktree, bool)> {
    declare_in(app, task, branch, &RepoSettings::load(app, project)?)
}

/// [`declare`] with the repository's settings loaded already. Templates
/// see the repository's main checkout as `{{project}}`.
pub fn declare_in(
    app: &App,
    task: &Task,
    branch: Option<&str>,
    settings: &RepoSettings,
) -> Result<(Worktree, bool)> {
    let declared = declaration::declare(
        task,
        branch,
        &settings.config,
        &settings.main,
        settings.name(),
        app.opts.home.as_deref(),
    )
    .map_err(|e| CliError::user(e.to_string()))?;
    Ok(match declared {
        Declared::New(worktree) => (worktree, true),
        Declared::Already(worktree) => (worktree, false),
    })
}

/// The worktree settings of the repository a task works in: `[worktree]`
/// as loaded from its main checkout, so the `.tasq.toml` next to the
/// repository applies wherever tasq runs (plan 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSettings {
    /// The task's project directory, where `git` runs.
    pub project: PathBuf,
    /// The repository, when `project` is inside one.
    pub repo: Option<Repo>,
    /// The main checkout (`project` outside a repository).
    pub main: PathBuf,
    /// `[worktree]` as configured for the main checkout.
    pub config: WorktreeConfig,
    /// Where relative template sources start: the directory of the file
    /// that set `worktree.templates`, else the main checkout.
    pub templates_dir: PathBuf,
    /// The repository's own `.tasq.toml`, when there is one.
    pub repo_file: Option<PathBuf>,
    /// The hook kinds that file sets: they run only once approved.
    pub untrusted: Vec<Kind>,
}

impl RepoSettings {
    /// The settings for `project`.
    pub fn load(app: &App, project: &Path) -> Result<Self> {
        let repo = Repo::of(project);
        let main = repo.as_ref().map_or(project, |r| &r.main).to_path_buf();
        let loaded = app.config_at(&main)?;
        let templates_dir = loaded
            .file_for("worktree.templates")
            .and_then(Path::parent)
            .map_or_else(|| main.clone(), Path::to_path_buf);
        let repo_file = project_file(&main, app.opts.home.as_deref());
        let untrusted = Kind::ALL
            .into_iter()
            .filter(|kind| {
                repo_file.is_some()
                    && loaded.file_for(&format!("worktree.hooks.{}", kind.key()))
                        == repo_file.as_deref()
            })
            .collect();
        Ok(Self {
            project: project.to_path_buf(),
            repo,
            main,
            config: loaded.config.worktree,
            templates_dir,
            repo_file,
            untrusted,
        })
    }

    /// The repository name for `{{repo}}`, when known.
    pub fn name(&self) -> Option<&str> {
        self.repo.as_ref().and_then(|r| r.name.as_deref())
    }
}

/// A git repository as found on disk, without running `git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The main checkout: where the shared git directory lives, so a
    /// linked worktree names the repository it belongs to.
    pub main: PathBuf,
    /// The repository name from `remote.origin.url`, when there is one.
    pub name: Option<String>,
}

impl Repo {
    /// The repository `dir` is inside, if any.
    pub fn of(dir: &Path) -> Option<Self> {
        let root = git::work_tree_root(dir, Path::exists)?;
        let dot_git = root.join(".git");
        let git_dir = if dot_git.is_file() {
            let text = std::fs::read_to_string(&dot_git).unwrap_or_default();
            git::gitdir(&text).map_or_else(|| dot_git.clone(), |d| root.join(d))
        } else {
            dot_git
        };
        let common = std::fs::read_to_string(git_dir.join("commondir"))
            .map_or_else(|_| git_dir.clone(), |rel| git_dir.join(rel.trim()));
        let common = common.canonicalize().unwrap_or(common);
        let main = match common.parent() {
            Some(parent) if common.file_name().is_some_and(|n| n == ".git") => parent.to_path_buf(),
            _ => root.to_path_buf(),
        };
        let name = std::fs::read_to_string(common.join("config"))
            .ok()
            .and_then(|config| git::origin_url(&config))
            .and_then(|url| vars::repo_name(&url));
        Some(Self { main, name })
    }
}

/// `tasq worktree <id> --create <branch>`: declares a worktree on
/// `branch` and creates it, unless its directory exists already.
pub fn create_and_track(app: &App, store: &mut NbStore, id: &TaskId, branch: &str) -> Result<()> {
    let mut task = store.get(id)?;
    let project = project_of(app, &task)?;
    let (worktree, new) = declare(app, &task, Some(branch), &project)?;
    if new {
        task.add_worktree(worktree.clone());
        store.update(&task)?;
    }
    let only = Task {
        worktrees: vec![worktree.clone()],
        ..task
    };
    let line = match materialize(app, &only, None, false, false)? {
        Some(created) => format!("[{id}] created worktree {}\n", created.line),
        None => format!("[{id}] worktree: {}\n", describe(&worktree)),
    };
    finish(app, store, id, &line)
}

/// `tasq worktree up <id> [--from REF] [--dry-run]`: creates the task's
/// declared worktree now, as launching it would.
pub fn up(app: &App, id: &str, from: Option<&str>, yes: bool, dry_run: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let store = app.open_store()?;
    let task = store.get(&id)?;
    let line = match materialize(app, &task, from, yes, dry_run)? {
        Some(created) if dry_run => {
            let mut lines = vec![format!("[{id}] would create worktree {}", created.line)];
            lines.extend(created.steps.iter().map(|step| format!("  {step}")));
            lines.join("\n") + "\n"
        }
        Some(created) => format!("[{id}] created worktree {}\n", created.line),
        None => match task.worktrees.iter().find(|w| w.path.is_dir()) {
            Some(existing) => format!("[{id}] worktree exists: {}\n", describe(existing)),
            None => {
                return Err(CliError::user(format!(
                    "task {id} declares no worktree with a branch; declare one with tasq worktree {id} [BRANCH]"
                )));
            }
        },
    };
    finish(app, &store, &id, &line)
}

/// `tasq worktree trust [DIR]`: approves the hooks of the repository's own
/// `.tasq.toml` ahead of time.
pub fn trust(app: &App, dir: Option<&Path>) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let dir = dir.unwrap_or(&app.opts.cwd);
    let dir = existing_dir(dir)
        .ok_or_else(|| CliError::user(format!("directory not found: {}", dir.display())))?;
    let settings = RepoSettings::load(app, &dir)?;
    let approved = worktree_hooks::trust(app, &settings)?;
    let file = settings.repo_file.as_deref().unwrap_or(&settings.main);
    if app.out.json_mode() {
        return app.out.json(&trusted_json(file, &approved));
    }
    let text = if approved.is_empty() {
        format!("no hooks to approve in {}\n", file.display())
    } else {
        let mut lines = vec![format!("approved the hooks of {}:", file.display())];
        lines.extend(
            approved
                .iter()
                .map(|(kind, line)| format!("  {}: {line}", kind.key())),
        );
        lines.join("\n") + "\n"
    };
    app.out.print(&text)
}

/// The JSON document of `trust`: the file and the lines it approved.
fn trusted_json(file: &Path, approved: &[(Kind, String)]) -> serde_json::Value {
    let approved = approved
        .iter()
        .map(|(kind, line)| serde_json::json!({"kind": kind.key(), "line": line}))
        .collect();
    json::document([
        ("file", serde_json::json!(file)),
        ("approved", serde_json::Value::Array(approved)),
    ])
}

/// A worktree [`materialize`] created, or would create on a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its directory.
    pub path: PathBuf,
    /// `<path> on <branch> (<where the branch comes from>)`.
    pub line: String,
    /// The provisioning steps, one line each (`link node_modules -> ...`).
    pub steps: Vec<String>,
}

/// Step 2 of the lifecycle: when none of `task`'s declared worktrees
/// exists, creates the first one that records a branch, in the task's
/// project repository, and provisions it (copies, links, templates). A new
/// branch starts from `from`, else `worktree.base`. Templates are rendered
/// before git runs, so a broken one stops everything. A dry run decides
/// and renders without fetching or creating anything. `None` when a
/// declared worktree exists or none can be created. The `pre-create`
/// hooks run in the main checkout before git, the `post-create` ones in
/// the worktree after provisioning; `yes` approves the repository's own
/// hooks for this run. The caller checks that worktrees are enabled.
pub fn materialize(
    app: &App,
    task: &Task,
    from: Option<&str>,
    yes: bool,
    dry_run: bool,
) -> Result<Option<Created>> {
    let git = Git { env: app.env_vec() };
    let settings = match project_of(app, task) {
        Ok(project) => Some(RepoSettings::load(app, &project)?),
        Err(_) => None,
    };
    let configured = settings
        .as_ref()
        .map_or(&app.config().worktree, |s| &s.config);
    let base = from.unwrap_or(&configured.base);
    let mut failed = None;
    let action = lifecycle::on_launch(
        &task.worktrees,
        Path::is_dir,
        |branch| {
            let state = match &settings {
                Some(s) => git
                    .branch_state(&s.project, branch, !dry_run)
                    .map_err(|e| CliError::user(e.to_string())),
                None => project_of(app, task).map(|_| BranchState::Absent),
            };
            state.unwrap_or_else(|e| {
                failed = Some(e);
                BranchState::Absent
            })
        },
        base,
    );
    if let Some(e) = failed {
        return Err(e);
    }
    let (
        LaunchAction::Materialize {
            path,
            branch,
            start,
        },
        Some(s),
    ) = (action, settings)
    else {
        return Ok(None);
    };
    let (start, note) = git.resolve_start(&s.project, &branch, &start);
    if let Some(note) = note {
        app.out.warn(&note);
    }
    let user = |e: &dyn std::fmt::Display| CliError::user(e.to_string());
    let steps =
        provision::plan(&s.config, &s.main, &path, &s.templates_dir).map_err(|e| user(&e))?;
    let vars = Vars::for_task(task, &s.main, s.name(), &s.config.variables)
        .with_branch(&branch, s.config.ports)
        .with_worktree(&path);
    let ready = prepare(&steps, &vars).map_err(|e| user(&e))?;
    let kinds = [Kind::PreCreate, Kind::PostCreate];
    let mut report = worktree_hooks::to_terminal(app);
    let approval = Approval::from_yes(yes);
    let [pre, post] =
        worktree_hooks::prepare(app, &s, &kinds, &vars, approval, dry_run, &mut report)?
            .try_into()
            .unwrap_or_else(|_| unreachable!("one Hooks per kind"));
    if !dry_run {
        let env = hooks::env(&vars);
        pre.run(app, &s.main, &env, &mut report)?;
        git.add(&s.project, &path, &branch, &start)
            .map_err(|e| user(&e))?;
        let warnings = apply(&ready).map_err(|e| {
            CliError::user(format!(
                "worktree {} was created, but provisioning it failed: {e}",
                path.display()
            ))
        })?;
        for warning in warnings {
            app.out.warn(&warning);
        }
        post.run(app, &path, &env, &mut report)?;
    }
    let mut lines = pre.describe();
    lines.extend(steps.iter().map(|step| step.describe(&path)));
    lines.extend(post.describe());
    Ok(Some(Created {
        line: describe_start(&path, &branch, &start),
        steps: lines,
        path,
    }))
}

/// Records `path` (which must exist) with its current branch, unless it is
/// already tracked.
fn track(app: &App, store: &mut NbStore, id: &TaskId, path: &Path) -> Result<()> {
    let dir = existing_dir(path)
        .ok_or_else(|| CliError::user(format!("worktree path not found: {}", path.display())))?;
    let mut task = store.get(id)?;
    if task.worktrees.iter().any(|w| w.path == dir) {
        return finish(
            app,
            store,
            id,
            &format!("[{id}] worktree already tracked: {}\n", dir.display()),
        );
    }
    let branch = Git { env: app.env_vec() }.current_branch(&dir);
    task.add_worktree(Worktree {
        path: dir.clone(),
        branch: branch.clone(),
    });
    store.update(&task)?;
    let suffix = branch.map_or_else(String::new, |b| format!(" ({b})"));
    finish(
        app,
        store,
        id,
        &format!("[{id}] worktree: {}{suffix}\n", dir.display()),
    )
}

/// `tasq worktree release <id> [--force] [--dry-run] [--yes]`: what closing
/// the task does to its worktrees, whatever its status and `on_done`.
pub fn release_command(app: &App, id: &str, force: bool, yes: bool, dry_run: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = App::task_id(id)?;
    let store = app.open_store()?;
    let task = store.get(&id)?;
    let how = Releasing {
        force,
        approval: Approval::from_yes(yes),
        dry_run,
        closing: false,
    };
    let lines = release(app, &task, how, &mut worktree_hooks::to_terminal(app))?;
    let text = if lines.is_empty() {
        format!("[{id}] nothing released\n")
    } else {
        prefixed(&id, &lines)
    };
    finish(app, &store, &id, &text)
}

/// `lines`, one per line, each prefixed with `[id] ` unless indented (the
/// steps under a dry run's line).
pub fn prefixed(id: &TaskId, lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| {
            if l.starts_with(' ') {
                format!("{l}\n")
            } else {
                format!("[{id}] {l}\n")
            }
        })
        .collect::<Vec<_>>()
        .concat()
}

/// How [`release`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Releasing {
    /// Remove dirty worktrees and delete unmerged, unpushed branches too.
    pub force: bool,
    /// For the repository's own `pre-remove`/`post-remove` hooks.
    pub approval: Approval,
    /// Say what would happen; change nothing.
    pub dry_run: bool,
    /// Called for a close: `worktree.on_done = "keep"` skips it all.
    pub closing: bool,
}

/// Step 3 of the lifecycle for every declared worktree of `task`: a clean
/// one is removed (with the `pre-remove` hook in it before, the
/// `post-remove` one in the main checkout after), and its branch deleted
/// when that loses nothing ([`Git::safety`]). What is kept is a warning
/// through `report`, with the command that finishes the job when there is
/// one. The entries stay. The lines say what was done (or would be, on a
/// dry run). The caller checks that worktrees are enabled.
///
/// # Errors
///
/// When the task's repository cannot be read; a removal or a deletion
/// that fails is a warning.
pub fn release(
    app: &App,
    task: &Task,
    how: Releasing,
    report: &mut dyn FnMut(HookEvent),
) -> Result<Vec<String>> {
    if task.worktrees.is_empty() {
        return Ok(Vec::new());
    }
    let s = RepoSettings::load(app, &project_of(app, task)?)?;
    if how.closing && s.config.on_done == OnDone::Keep {
        return Ok(Vec::new());
    }
    if s.repo.is_none() {
        return Err(CliError::user(format!(
            "{} is not in a git repository",
            s.project.display()
        )));
    }
    let git = Git { env: app.env_vec() };
    let user = |e: &dyn std::fmt::Display| CliError::user(e.to_string());
    let listed = git.worktrees(&s.main).map_err(|e| user(&e))?;
    let main = canonical(&s.main);
    let cwd = canonical(&app.opts.cwd);
    let mut lines = Vec::new();
    for worktree in &task.worktrees {
        let path = canonical(&worktree.path);
        let provisioned = provisioned(&s, &path);
        let dir = status::dir_seen(&path, path.is_dir(), &main, &cwd, &listed, || {
            // Doubt keeps it: an unreadable status counts as a change.
            git.changes(&path, &provisioned)
                .unwrap_or_else(|e| vec![e.to_string()])
        });
        let safety = match &worktree.branch {
            Some(branch) => git.safety(&s.main, branch).map_err(|e| user(&e))?,
            None => None,
        };
        let seen = safety.as_ref().map_or(BranchSeen::Absent, Safety::seen);
        let decided = lifecycle::on_close(dir, seen, s.config.delete_branch, how.force);
        let one = One {
            app,
            git: &git,
            settings: &s,
            task,
            worktree,
            path: &path,
            how,
        };
        let gone = one.directory(decided.worktree, &mut lines, report)?;
        one.branch(decided.branch, gone, safety.as_ref(), &mut lines, report);
    }
    Ok(lines)
}

/// One declared worktree being released.
struct One<'a> {
    app: &'a App,
    git: &'a Git,
    settings: &'a RepoSettings,
    task: &'a Task,
    worktree: &'a Worktree,
    path: &'a Path,
    how: Releasing,
}

impl One<'_> {
    /// Carries out `fate` for the directory; whether it is gone after.
    fn directory(
        &self,
        fate: WorktreeFate,
        lines: &mut Vec<String>,
        report: &mut dyn FnMut(HookEvent),
    ) -> Result<bool> {
        let path = self.path.display();
        match fate {
            WorktreeFate::Absent => Ok(true),
            WorktreeFate::Keep(KeepWorktree::Main) => Ok(false),
            WorktreeFate::Keep(why) => {
                let id = &self.task.id;
                let finish = match why {
                    KeepWorktree::Dirty => {
                        format!("; tasq worktree release {id} --force removes it")
                    }
                    KeepWorktree::InUse => {
                        format!("; tasq worktree release {id} from another directory removes it")
                    }
                    _ => String::new(),
                };
                report(HookEvent::Warning(format!(
                    "kept worktree {path}: {}{finish}",
                    why.reason()
                )));
                Ok(false)
            }
            WorktreeFate::Remove if self.how.dry_run => {
                let s = self.settings;
                let vars = self.vars();
                let kinds = [Kind::PreRemove, Kind::PostRemove];
                let hooks = worktree_hooks::prepare(
                    self.app,
                    s,
                    &kinds,
                    &vars,
                    self.how.approval,
                    true,
                    report,
                )?;
                lines.push(format!("would remove worktree {path}"));
                lines.extend(
                    hooks
                        .iter()
                        .flat_map(Hooks::describe)
                        .map(|l| format!("  {l}")),
                );
                Ok(true)
            }
            WorktreeFate::Remove => self.remove(lines, report),
        }
    }

    /// The remove hooks around `git worktree remove`; whether it is gone.
    fn remove(&self, lines: &mut Vec<String>, report: &mut dyn FnMut(HookEvent)) -> Result<bool> {
        let s = self.settings;
        let path = self.path.display();
        let vars = self.vars();
        let kinds = [Kind::PreRemove, Kind::PostRemove];
        let [pre, post] =
            worktree_hooks::prepare(self.app, s, &kinds, &vars, self.how.approval, false, report)?
                .try_into()
                .unwrap_or_else(|_| unreachable!("one Hooks per kind"));
        let env = hooks::env(&vars);
        if let Err(e) = pre.run(self.app, self.path, &env, report) {
            report(HookEvent::Warning(format!("kept worktree {path}: {e}")));
            return Ok(false);
        }
        if let Err(e) = self.git.remove(&s.main, self.path) {
            report(HookEvent::Warning(format!("kept worktree {path}: {e}")));
            return Ok(false);
        }
        lines.push(format!("removed worktree {path}"));
        post.run(self.app, &s.main, &env, report)?;
        Ok(true)
    }

    /// Carries out `fate` for the branch, once its directory is `gone`.
    fn branch(
        &self,
        fate: BranchFate,
        gone: bool,
        safety: Option<&Safety>,
        lines: &mut Vec<String>,
        report: &mut dyn FnMut(HookEvent),
    ) {
        let Some(branch) = &self.worktree.branch else {
            return;
        };
        let why = safety
            .and_then(Safety::reason)
            .unwrap_or_else(|| "forced".to_owned());
        match fate {
            BranchFate::Delete if !gone => {}
            BranchFate::Delete if self.how.dry_run => {
                lines.push(format!("would delete branch {branch} ({why})"));
            }
            BranchFate::Delete => {
                let main = &self.settings.main;
                // A directory removed by hand leaves its registration, and
                // git refuses to delete a branch checked out there.
                let _ = self.git.prune(main, false);
                match self.git.delete_branch(main, branch) {
                    Ok(()) => lines.push(format!("deleted branch {branch} ({why})")),
                    Err(e) => report(HookEvent::Warning(format!("kept branch {branch}: {e}"))),
                }
            }
            BranchFate::Keep(KeepBranch::Unsafe) => report(HookEvent::Warning(format!(
                "kept branch {branch}: {}; tasq worktree release {} --force deletes it",
                KeepBranch::Unsafe.reason(),
                self.task.id
            ))),
            BranchFate::Keep(_) | BranchFate::Absent => {}
        }
    }

    /// The template variables of this worktree, for its hooks.
    fn vars(&self) -> Vars {
        let s = self.settings;
        Vars::for_task(self.task, &s.main, s.name(), &s.config.variables)
            .with_branch(
                self.worktree.branch.as_deref().unwrap_or_default(),
                s.config.ports,
            )
            .with_worktree(self.path)
    }
}

/// The paths provisioning writes into the worktree at `path`, relative to
/// it: they can be made again, so they do not make it dirty.
pub fn provisioned(settings: &RepoSettings, path: &Path) -> Vec<PathBuf> {
    provision::plan(
        &settings.config,
        &settings.main,
        path,
        &settings.templates_dir,
    )
    .unwrap_or_default()
    .iter()
    .filter_map(|step| step.target().strip_prefix(path).ok().map(Path::to_path_buf))
    .collect()
}

/// `path` with symlinks resolved when it exists, as git records paths.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
