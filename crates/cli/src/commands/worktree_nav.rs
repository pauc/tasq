//! Finding and moving between worktrees (plan 4.6, WT-207):
//! `tasq worktree ls`, `switch`, `path` and `shell-init`.
//!
//! A process cannot change its shell's directory, so `switch` writes the
//! destination to the file `$TASQ_CD_FILE` names, and the shell function
//! `shell-init` prints sets that variable and changes directory after
//! tasq exits. Without the function, `switch` prints the path.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::ValueEnum;
use serde_json::{Value, json};
use tasq_core::model::{Task, TaskId, Worktree};
use tasq_core::query::Filter;
use tasq_core::store::Store;
use tasq_core::worktree::target::{self, Found, Target};
use tasq_core::worktree::{git, hooks};
use tasq_store_nb::NbStore;
use tasq_worktree::Git;

use crate::app::App;
use crate::commands::existing_dir;
use crate::commands::worktree::{
    Repo, RepoSettings, disabled, materialize, project_of, provisioned,
};
use crate::error::{CliError, Result};
use crate::json;

/// The variable naming the file `switch` writes its destination to.
pub const CD_FILE: &str = "TASQ_CD_FILE";

/// The state file holding the worktree the last `switch` left (`-`).
const PREVIOUS: &str = "previous-worktree";

/// One declared worktree, as `ls` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The task declaring it.
    pub id: TaskId,
    /// The task's title.
    pub title: String,
    /// The task's status, `done` for a closed task.
    pub status: Option<String>,
    /// Whether the task is closed.
    pub done: bool,
    /// The declaration.
    pub worktree: Worktree,
    /// Whether its directory exists.
    pub exists: bool,
    /// Its changed paths (provisioned files left out); `None` when it does
    /// not exist or git cannot say.
    pub changes: Option<Vec<String>>,
}

impl Row {
    /// `clean`, `<n> changed`, `not created` or `unknown`.
    pub fn state(&self) -> String {
        match (&self.changes, self.exists) {
            (_, false) => "not created".to_owned(),
            (None, true) => "unknown".to_owned(),
            (Some(c), true) if c.is_empty() => "clean".to_owned(),
            (Some(c), true) => format!("{} changed", c.len()),
        }
    }

    fn json(&self) -> Value {
        json!({
            "task": self.id.as_str(),
            "title": self.title,
            "status": self.status,
            "done": self.done,
            "path": self.worktree.path,
            "branch": self.worktree.branch,
            "exists": self.exists,
            "changes": self.changes,
        })
    }
}

/// `tasq worktree ls [ID] [--all]`.
pub fn ls(app: &App, id: Option<&str>, all: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let id = id.map(App::task_id).transpose()?;
    let store = app.open_store()?;
    let rows = rows(app, &store, id.as_ref(), all)?;
    if app.out.json_mode() {
        let list = rows.iter().map(Row::json).collect();
        return app
            .out
            .json(&json::document([("worktrees", Value::Array(list))]));
    }
    if rows.is_empty() {
        return app.out.print("no declared worktrees\n");
    }
    app.out.print(&render(&rows))
}

/// The rows as aligned text: id, status, branch, state, path.
pub fn render(rows: &[Row]) -> String {
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            [
                format!("[{}]", r.id),
                r.status.clone().unwrap_or_else(|| "-".to_owned()),
                r.worktree.branch.clone().unwrap_or_else(|| "-".to_owned()),
                r.state(),
                r.worktree.path.display().to_string(),
            ]
        })
        .collect();
    let mut widths = [0; 4];
    for row in &cells {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let [w0, w1, w2, w3] = widths;
    cells
        .iter()
        .map(|[id, status, branch, state, path]| {
            format!("{id:<w0$}  {status:<w1$}  {branch:<w2$}  {state:<w3$}  {path}\n")
        })
        .collect::<Vec<_>>()
        .concat()
}

/// The declared worktrees to list: of task `id` when given; else of every
/// task working in the current directory's repository (every task with
/// `all`, or outside a repository), leaving out those of closed tasks that
/// do not exist.
pub fn rows(app: &App, store: &NbStore, id: Option<&TaskId>, all: bool) -> Result<Vec<Row>> {
    let tasks = match id {
        Some(id) => vec![store.get(id)?],
        None => store.list(&Filter::new().any_done())?,
    };
    let here = if id.is_none() && !all {
        Repo::of(&app.opts.cwd).map(|r| r.main)
    } else {
        None
    };
    let git = Git { env: app.env_vec() };
    let mut settings: BTreeMap<PathBuf, Option<RepoSettings>> = BTreeMap::new();
    let mut rows = Vec::new();
    for task in &tasks {
        if task.worktrees.is_empty() {
            continue;
        }
        let project = project_of(app, task).ok();
        let main = project.as_deref().and_then(Repo::of).map(|r| r.main);
        if here.is_some() && main != here {
            continue;
        }
        for worktree in &task.worktrees {
            let exists = worktree.path.is_dir();
            if id.is_none() && task.done && !exists {
                continue;
            }
            let changes = match (&project, exists) {
                (Some(project), true) => {
                    let s = settings
                        .entry(project.clone())
                        .or_insert_with(|| RepoSettings::load(app, project).ok());
                    let path = worktree
                        .path
                        .canonicalize()
                        .unwrap_or_else(|_| worktree.path.clone());
                    let skip = s
                        .as_ref()
                        .map(|s| provisioned(s, &path))
                        .unwrap_or_default();
                    git.changes(&path, &skip).ok()
                }
                _ => None,
            };
            rows.push(Row {
                id: task.id.clone(),
                title: task.title.clone(),
                status: status_of(task),
                done: task.done,
                worktree: worktree.clone(),
                exists,
                changes,
            });
        }
    }
    Ok(rows)
}

fn status_of(task: &Task) -> Option<String> {
    if task.done {
        Some("done".to_owned())
    } else {
        task.status.as_ref().map(|s| s.as_str().to_owned())
    }
}

/// `tasq worktree path <TARGET>`: prints the directory, creating nothing.
pub fn path(app: &App, target: &str) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let store = app.open_store()?;
    let dir = resolve(app, &store, target, None)?;
    if app.out.json_mode() {
        return app.out.json(&json::document([("path", json!(dir))]));
    }
    app.out.print(&format!("{}\n", dir.display()))
}

/// `tasq worktree switch [TARGET] [--yes]`: resolves the target (a task's
/// worktree is created first), or lets the user choose one, remembers the
/// worktree being left for `-`, and hands the directory to the shell
/// function.
pub fn switch(app: &App, target: Option<&str>, yes: bool) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let store = app.open_store()?;
    let dir = match target {
        Some(target) => resolve(app, &store, target, Some(yes))?,
        None => choose(app, &store, yes)?,
    };
    let here = git::work_tree_root(&app.opts.cwd, Path::exists).unwrap_or(&app.opts.cwd);
    let here = here.canonicalize().unwrap_or_else(|_| here.to_path_buf());
    if here != dir {
        write_state(app, PREVIOUS, &format!("{}\n", here.display()))?;
    }
    if app.out.json_mode() {
        return app.out.json(&json::document([("path", json!(dir))]));
    }
    if let Some(file) = app.opts.env.get(CD_FILE).filter(|f| !f.is_empty()) {
        return std::fs::write(file, dir.display().to_string())
            .map_err(|e| CliError::user(format!("{file}: {e}")));
    }
    app.out.warn(
        "the shell function changes directory: add eval \"$(tasq worktree shell-init zsh)\" \
         (or bash, fish) to the shell's rc file",
    );
    app.out.print(&format!("{}\n", dir.display()))
}

/// The directory `arg` names. A task's worktree that does not exist is
/// created when `create` is `Some(yes)` (`switch`), an error otherwise.
fn resolve(app: &App, store: &NbStore, arg: &str, create: Option<bool>) -> Result<PathBuf> {
    let parsed =
        target::parse(arg, Path::new(arg).is_dir()).map_err(|e| CliError::user(e.to_string()))?;
    match parsed {
        Target::Task(id) => task_dir(app, &store.get(&id)?, create),
        Target::Main => Repo::of(&app.opts.cwd).map(|r| r.main).ok_or_else(|| {
            CliError::user(format!(
                "not in a git repository: {}",
                app.opts.cwd.display()
            ))
        }),
        Target::Previous => {
            let text = read_state(app, PREVIOUS)?.ok_or_else(|| {
                CliError::user("no previous worktree: nothing was switched from yet")
            })?;
            let previous = PathBuf::from(text.trim());
            existing_dir(&previous).ok_or_else(|| {
                CliError::user(format!(
                    "the previous worktree is gone: {}",
                    previous.display()
                ))
            })
        }
        Target::Path(path) => existing_dir(&path)
            .ok_or_else(|| CliError::user(format!("directory not found: {}", path.display()))),
        Target::Branch(branch) => {
            let git = Git { env: app.env_vec() };
            let listed = match Repo::of(&app.opts.cwd) {
                Some(repo) => git.worktrees(&repo.main).unwrap_or_default(),
                None => Vec::new(),
            };
            let tasks = store.list(&Filter::new().any_done())?;
            let declared: Vec<(TaskId, Worktree)> = tasks
                .iter()
                .flat_map(|t| t.worktrees.iter().map(|w| (t.id.clone(), w.clone())))
                .collect();
            match target::by_branch(&branch, &listed, &declared, Path::is_dir) {
                Found::Dir(dir) => Ok(dir),
                Found::Create(id) => task_dir(app, &store.get(&id)?, create),
                Found::Nothing => Err(CliError::user(format!("no worktree on branch {branch}"))),
            }
        }
    }
}

/// The task's first existing worktree; else, with `create`, the one
/// [`materialize`] creates.
fn task_dir(app: &App, task: &Task, create: Option<bool>) -> Result<PathBuf> {
    if let Some(found) = task.worktrees.iter().find(|w| w.path.is_dir()) {
        return Ok(found.path.clone());
    }
    let id = &task.id;
    let Some(yes) = create else {
        return Err(CliError::user(format!(
            "task {id} has no worktree yet; tasq worktree switch task:{id} creates it"
        )));
    };
    match materialize(app, task, None, yes, false)? {
        Some(created) => {
            app.out
                .print(&format!("[{id}] created worktree {}\n", created.line))?;
            Ok(created.path)
        }
        None => Err(CliError::user(format!(
            "task {id} declares no worktree with a branch; declare one with tasq worktree {id} [BRANCH]"
        ))),
    }
}

/// The user's pick among the main checkout and the listed worktrees: with
/// fzf when it is on `PATH`, else from a numbered list on stderr.
fn choose(app: &App, store: &NbStore, yes: bool) -> Result<PathBuf> {
    enum Choice {
        Dir(PathBuf),
        Task(TaskId),
    }
    let mut choices: Vec<(String, Choice)> = Vec::new();
    if let Some(repo) = Repo::of(&app.opts.cwd) {
        choices.push((
            format!("^  main checkout  {}", repo.main.display()),
            Choice::Dir(repo.main),
        ));
    }
    let rows = rows(app, store, None, false)?;
    // A task with a worktree that exists creates no other: its declared
    // but missing ones are not destinations.
    let has_one: Vec<TaskId> = rows
        .iter()
        .filter(|r| r.exists)
        .map(|r| r.id.clone())
        .collect();
    for row in rows {
        if !row.exists && has_one.contains(&row.id) {
            continue;
        }
        let label = render(std::slice::from_ref(&row)).trim_end().to_owned();
        let choice = if row.exists {
            Choice::Dir(row.worktree.path)
        } else {
            Choice::Task(row.id)
        };
        choices.push((label, choice));
    }
    if choices.is_empty() {
        return Err(CliError::user("no worktrees to switch to"));
    }
    let lines: Vec<String> = choices
        .iter()
        .enumerate()
        .map(|(i, (label, _))| format!("{:>2}  {label}", i + 1))
        .collect();
    let env = app.env_vec();
    let reply = match tasq_launch::process::which(&env, "fzf") {
        Some(fzf) => fzf_pick(&fzf, &lines, &env)?,
        None => numbered_pick(&lines)?,
    };
    let index = chosen(&reply, choices.len())
        .ok_or_else(|| CliError::user(format!("not a choice: {:?}", reply.trim())))?;
    match choices.swap_remove(index).1 {
        Choice::Dir(dir) => Ok(dir),
        Choice::Task(id) => task_dir(app, &store.get(&id)?, Some(yes)),
    }
}

/// The index a reply picks among `count` numbered lines: its first word,
/// a number from 1 to `count`.
pub fn chosen(reply: &str, count: usize) -> Option<usize> {
    let n: usize = reply.split_whitespace().next()?.parse().ok()?;
    (1..=count).contains(&n).then(|| n - 1)
}

/// Runs fzf over `lines` and returns the line picked.
///
/// Reason: runs fzf on the terminal; `chosen` reads what it returns.
#[mutants::skip]
fn fzf_pick(fzf: &Path, lines: &[String], env: &[(String, String)]) -> Result<String> {
    let mut child = Command::new(fzf)
        .args(["--reverse", "--height=40%", "--prompt=switch> "])
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| CliError::user(format!("could not run fzf: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(lines.join("\n").as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(CliError::user("nothing chosen"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Prints `lines` on stderr and reads the number typed on stdin.
///
/// Reason: reads stdin; `chosen` reads the reply.
#[mutants::skip]
fn numbered_pick(lines: &[String]) -> Result<String> {
    let mut stderr = std::io::stderr().lock();
    for line in lines {
        writeln!(stderr, "{line}")?;
    }
    write!(stderr, "switch to [1-{}]: ", lines.len())?;
    stderr.flush()?;
    let mut reply = String::new();
    if std::io::stdin().lock().read_line(&mut reply)? == 0 {
        return Err(CliError::user("nothing chosen"));
    }
    Ok(reply)
}

fn state_path(app: &App, name: &str) -> Result<PathBuf> {
    hooks::state_file(
        app.opts.env.get("XDG_STATE_HOME").map(String::as_str),
        app.opts.home.as_deref(),
        name,
    )
    .ok_or_else(|| CliError::user("cannot tell where to keep tasq's state: HOME is not set"))
}

fn read_state(app: &App, name: &str) -> Result<Option<String>> {
    let file = state_path(app, name)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CliError::user(format!("{}: {e}", file.display()))),
    }
}

fn write_state(app: &App, name: &str, text: &str) -> Result<()> {
    let file = state_path(app, name)?;
    let failed = |e: std::io::Error| CliError::user(format!("{}: {e}", file.display()));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    std::fs::write(&file, text).map_err(failed)
}

/// The shells `shell-init` writes a function for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InitShell {
    /// bash.
    Bash,
    /// zsh.
    Zsh,
    /// fish.
    Fish,
}

/// `tasq worktree shell-init <SHELL>`.
pub fn shell_init(app: &App, shell: InitShell) -> Result<()> {
    if !app.worktrees() {
        return Err(disabled());
    }
    let script = init_script(shell);
    if app.out.json_mode() {
        return app.out.json(&json::document([("script", json!(script))]));
    }
    app.out.print(script)
}

const POSIX_INIT: &str = r#"# tasq: `tasq worktree switch` changes this shell's directory.
# Add to ~/.bashrc or ~/.zshrc: eval "$(tasq worktree shell-init bash)" (or zsh)
tasq() {
    case " $* " in
        *" worktree switch "*) ;;
        *) command tasq "$@"; return ;;
    esac
    local tasq_cd_file tasq_status
    tasq_cd_file="$(mktemp "${TMPDIR:-/tmp}/tasq-cd.XXXXXX")" || return 1
    TASQ_CD_FILE="$tasq_cd_file" command tasq "$@"
    tasq_status=$?
    if [ -s "$tasq_cd_file" ]; then
        cd -- "$(cat "$tasq_cd_file")" || tasq_status=$?
    fi
    rm -f -- "$tasq_cd_file"
    return "$tasq_status"
}
"#;

const FISH_INIT: &str = r#"# tasq: `tasq worktree switch` changes this shell's directory.
# Add to ~/.config/fish/config.fish: tasq worktree shell-init fish | source
function tasq --wraps tasq --description 'tasq; worktree switch changes directory'
    if not string match -q -- '* worktree switch *' " $argv "
        command tasq $argv
        return
    end
    set -l tasq_cd_file (mktemp)
    or return 1
    env TASQ_CD_FILE=$tasq_cd_file tasq $argv
    set -l tasq_status $status
    if test -s $tasq_cd_file
        cd (cat $tasq_cd_file)
        or set tasq_status $status
    end
    rm -f $tasq_cd_file
    return $tasq_status
end
"#;

/// The function for `shell`.
pub fn init_script(shell: InitShell) -> &'static str {
    match shell {
        InitShell::Bash | InitShell::Zsh => POSIX_INIT,
        InitShell::Fish => FISH_INIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_are_numbers_in_range() {
        assert_eq!(chosen("2\n", 3), Some(1));
        assert_eq!(chosen(" 1  ^  main checkout  /app\n", 3), Some(0));
        assert_eq!(chosen("3", 3), Some(2));
        assert_eq!(chosen("0", 3), None);
        assert_eq!(chosen("4", 3), None);
        assert_eq!(chosen("x", 3), None);
        assert_eq!(chosen("", 3), None);
    }

    fn row(
        id: u64,
        status: Option<&str>,
        branch: Option<&str>,
        exists: bool,
        changes: Option<usize>,
    ) -> Row {
        Row {
            id: TaskId::from(id),
            title: "T".into(),
            status: status.map(str::to_owned),
            done: false,
            worktree: Worktree {
                path: format!("/w/{id}").into(),
                branch: branch.map(str::to_owned),
            },
            exists,
            changes: changes.map(|n| (0..n).map(|i| i.to_string()).collect()),
        }
    }

    #[test]
    fn rows_render_aligned() {
        let rows = [
            row(3, Some("in-progress"), Some("b"), true, Some(0)),
            row(12, None, Some("feat/long"), true, Some(2)),
            row(4, Some("done"), None, false, None),
            row(5, Some("ready"), Some("c"), true, None),
        ];
        assert_eq!(
            render(&rows),
            "[3]   in-progress  b          clean        /w/3\n\
             [12]  -            feat/long  2 changed    /w/12\n\
             [4]   done         -          not created  /w/4\n\
             [5]   ready        c          unknown      /w/5\n"
        );
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn row_json() {
        let r = row(3, Some("ready"), Some("b"), true, Some(1));
        assert_eq!(
            r.json(),
            json!({
                "task": "3", "title": "T", "status": "ready", "done": false,
                "path": "/w/3", "branch": "b", "exists": true, "changes": ["0"],
            })
        );
    }

    #[test]
    fn scripts_per_shell() {
        assert_eq!(init_script(InitShell::Bash), POSIX_INIT);
        assert_eq!(init_script(InitShell::Zsh), POSIX_INIT);
        assert_eq!(init_script(InitShell::Fish), FISH_INIT);
        assert!(FISH_INIT.contains("TASQ_CD_FILE=$tasq_cd_file"));
    }
}
