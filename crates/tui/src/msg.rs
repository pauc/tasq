//! Messages into [`crate::update()`] and the commands it asks the runtime to
//! run (the Elm shape: `update(model, msg) -> cmds`).

use std::path::PathBuf;

use tasq_core::edit::{Fields, Target};
use tasq_core::model::{Priority, Status, Task, TaskDraft, TaskId, Worktree};

/// Something that happened: a key, translated by [`crate::keys`] for the
/// current mode, a terminal resize, or the result of a [`Cmd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// Move the selection or the picker cursor up one.
    Up,
    /// Move the selection or the picker cursor down one.
    Down,
    /// In the edit view: the cursor back, or the previous choice.
    Left,
    /// In the edit view: the cursor forward, or the next choice.
    Right,
    /// In the edit view: the cursor to the start of the line.
    Home,
    /// In the edit view: the cursor to the end of the line.
    End,
    /// In the edit view: delete the character under the cursor.
    Delete,
    /// In a text input: `Ctrl+W`, delete the word before the cursor.
    DeleteWord,
    /// In a text input: `Ctrl+U`, delete from the start of the line to the
    /// cursor.
    KillToStart,
    /// In a text input: `Ctrl+K`, delete from the cursor to the end of the
    /// line.
    KillToEnd,
    /// In the edit view: `Tab`, the next row.
    NextField,
    /// In the edit view: `Shift+Tab`, the previous row.
    PrevField,
    /// In the edit view: `Ctrl+S`, validate and write.
    Save,
    /// In the calendar picker: `t`, the cursor to today.
    Today,
    /// Move the selection [`crate::model::PAGE`] rows up.
    PageUp,
    /// Move the selection [`crate::model::PAGE`] rows down.
    PageDown,
    /// Select the first task (`g`).
    Top,
    /// Select the last task (`G`).
    Bottom,
    /// `Ctrl+K`: move the selected task before the task above it in its
    /// status group.
    MoveUp,
    /// `Ctrl+J`: move the selected task after the task below it in its
    /// status group.
    MoveDown,
    /// Confirm: launch (normal mode), apply the filter, pick, send the note.
    Enter,
    /// Cancel: leave the mode, or clear the filter / hide the detail.
    Escape,
    /// Delete the last typed character.
    Backspace,
    /// A typed character, in a text or picker mode.
    Char(char),
    /// Text pasted into an input (bracketed paste).
    Paste(String),
    /// `/`: start typing a filter.
    BeginFilter,
    /// `t`: open the status picker.
    BeginStatus,
    /// `p`: open the priority picker.
    BeginPriority,
    /// `l`: start typing a progress note.
    BeginNote,
    /// `d`: start typing the final note, then close the task.
    BeginDone,
    /// `c`: start typing the title of a new task.
    BeginCreate,
    /// `e`: open the edit view on the selected task.
    Edit,
    /// `E`: open the task's file in the editor.
    Editor,
    /// `v`: show the task with `tasq view` (glow, the pager).
    View,
    /// `Enter` in normal mode: open a work session in this terminal.
    Launch,
    /// `Ctrl+Enter` / `Shift+Enter` in normal mode: open a work session in
    /// a new window, switching to it or not.
    LaunchDetached {
        /// Whether to switch to the new window.
        focus: bool,
    },
    /// `s`: run the sources that run by default.
    Sync,
    /// `S`: open the source picker.
    BeginSources,
    /// `r`: reload from the store.
    Reload,
    /// `?`: toggle the help overlay.
    Help,
    /// `Tab`: switch between list and detail.
    ToggleDetail,
    /// `z`/`Space`: fold or unfold the group under the cursor.
    ToggleGroup,
    /// `a`: show or hide the DONE group.
    ToggleDone,
    /// `T`: switch the Today view on or off.
    ToggleToday,
    /// `Right`: show the selected task's detail.
    ShowDetail,
    /// `Left`: hide the detail.
    HideDetail,
    /// `J`/`Ctrl+E`: scroll the detail one line down.
    DetailDown,
    /// `K`/`Ctrl+Y`: scroll the detail one line up.
    DetailUp,
    /// `q`: leave.
    Quit,
    /// The terminal is now this size.
    Resize(u16, u16),
    /// Tasks arrived from the store.
    Loaded(Vec<Task>),
    /// Select this task if it is visible (a task the UI just created).
    Select(TaskId),
    /// A command succeeded with something to say.
    Info(String),
    /// A command failed.
    Failed(String),
}

/// What the runtime does for the model, against the store, the clock and
/// the host. Every edit is one of the core `edit` functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Read the open tasks again.
    Load,
    /// `tasq set <id> <status>`.
    SetStatus(TaskId, Status),
    /// `tasq reopen <id> <status>`: the status picker on a done task.
    Reopen(TaskId, Status),
    /// `tasq set <id> <priority>`.
    SetPriority(TaskId, Priority),
    /// `tasq log <id> <note>`.
    Log(TaskId, String),
    /// `tasq done <id> [note]`.
    Done(TaskId, Option<String>),
    /// The create view's save: [`tasq_core::store::Store::create`] with
    /// the draft, the host's `worktree` with the typed branch (`None`:
    /// `worktree.on_create` decides), then its `after_create`.
    Create(Box<TaskDraft>, Option<String>),
    /// The edit view's save: [`tasq_core::edit::revise`] with the fields.
    Revise(TaskId, Box<Fields>),
    /// `tasq move <id> ...`: [`tasq_core::edit::move_task`] with the
    /// target; `Ctrl+K` / `Ctrl+J` send `Before` / `After` the visible
    /// neighbour in the task's status group.
    Move(TaskId, Target),
    /// Open the task's file in the editor (terminal released meanwhile).
    Editor(TaskId),
    /// `tasq view <id>` (terminal released meanwhile), then a pause for a
    /// key when the pager may quit at once (see [`Model::view_pauses`]).
    ///
    /// [`Model::view_pauses`]: crate::model::Model::view_pauses
    View(TaskId, bool),
    /// Open a work session on the task: in this terminal (released
    /// meanwhile) or in a new window (the UI keeps the screen).
    Launch(TaskId, LaunchTarget),
    /// `tasq sync`, or `tasq sync --source <name>...` for a non-empty list
    /// (terminal released meanwhile).
    Sync(Vec<String>),
}

/// Where a work session opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTarget {
    /// This terminal, with `launch.default`; the UI waits for it.
    Here,
    /// A new window, with `launch.detached`; the UI stays up.
    Detached {
        /// Whether to switch to the new window.
        focus: bool,
    },
}

impl Cmd {
    /// Whether the command runs something that needs the terminal for
    /// itself, so the runtime must leave the alternate screen first.
    pub fn releases_terminal(&self) -> bool {
        matches!(
            self,
            Self::Editor(_) | Self::View(..) | Self::Launch(_, LaunchTarget::Here) | Self::Sync(_)
        )
    }

    /// Whether the runtime should wait for a key before redrawing, so the
    /// command's output can be read (an editor needs no such pause; a
    /// view only when its pager may quit at once).
    pub fn pauses_after(&self) -> bool {
        match self {
            Self::View(_, pause) => *pause,
            _ => matches!(self, Self::Launch(_, LaunchTarget::Here) | Self::Sync(_)),
        }
    }
}

/// What a host action reported: a one-line result, shown in the status bar.
pub type HostResult = Result<String, String>;

/// The part of the outside world the TUI cannot reach through the
/// [`tasq_core::store::Store`]: an editor, the launchers, `sync` and the
/// `[hooks]` that follow a create or a close. The CLI implements it by
/// running itself (and its hooks in-process); tests record the calls.
pub trait Host {
    /// Opens `file` (the task's file) in the user's editor and waits.
    fn edit(&mut self, id: &TaskId, file: &std::path::Path) -> HostResult;

    /// Opens a work session on the task: `tasq pick <id>` in this terminal
    /// for [`LaunchTarget::Here`], waiting for it; `tasq pick <id>
    /// --detached [--no-focus]` for [`LaunchTarget::Detached`], with its
    /// output captured so the screen is untouched and its last line is
    /// the result.
    fn launch(&mut self, id: &TaskId, target: LaunchTarget) -> HostResult;

    /// Shows the task: `tasq view <id>` in this terminal, waiting for it.
    fn view(&mut self, id: &TaskId) -> HostResult;

    /// Runs the sources: `tasq sync` for an empty `sources`, else
    /// `tasq sync --source <name>` for each name.
    fn sync(&mut self, sources: &[String]) -> HostResult;

    /// Called after `task` was closed through the store (the `d` key), with
    /// the task as written; the CLI runs its `post-done` hooks here. The
    /// close already happened, so `Err` is a warning for the status bar,
    /// not a failure of the close.
    fn after_done(&mut self, task: &Task) -> Result<(), String>;

    /// Called after `task` was created through the store (the `c` key),
    /// before [`Host::after_create`]: the worktree to declare for it on
    /// `branch` (typed in the create view), or as `worktree.on_create`
    /// decides without one; `None` for no worktree. The CLI renders it
    /// like `tasq create --worktree`; the runtime stores it. `Err` is a
    /// warning: the task exists either way.
    fn worktree(&mut self, task: &Task, branch: Option<&str>) -> Result<Option<Worktree>, String>;

    /// Called after `task` was created through the store (the `c` key),
    /// with the task as written; the CLI runs its `post-create` hooks
    /// here. Like [`Host::after_done`], `Err` is a warning: the task
    /// exists either way.
    fn after_create(&mut self, task: &Task) -> Result<(), String>;
}

/// A [`Host`] that refuses everything, for front ends that only browse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoHost;

impl Host for NoHost {
    fn edit(&mut self, _id: &TaskId, _file: &std::path::Path) -> HostResult {
        Err("editing is not available here".to_owned())
    }

    fn launch(&mut self, _id: &TaskId, _target: LaunchTarget) -> HostResult {
        Err("launching is not available here".to_owned())
    }

    fn view(&mut self, _id: &TaskId) -> HostResult {
        Err("viewing is not available here".to_owned())
    }

    fn sync(&mut self, _sources: &[String]) -> HostResult {
        Err("sync is not available here".to_owned())
    }

    fn after_done(&mut self, _task: &Task) -> Result<(), String> {
        Ok(())
    }

    fn worktree(
        &mut self,
        _task: &Task,
        _branch: Option<&str>,
    ) -> Result<Option<Worktree>, String> {
        Ok(None)
    }

    fn after_create(&mut self, _task: &Task) -> Result<(), String> {
        Ok(())
    }
}

/// A [`Host`] that records what it was asked and answers with canned
/// results; the test double of this crate and of the CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordingHost {
    /// Every call, as `edit <id> <file>`, `launch <id>`, `launch <id>
    /// detached` / `launch <id> detached no-focus`, `view <id>`, `sync` /
    /// `sync <name>...`,
    /// `worktree <id> [branch]`, `after_done <id>` or `after_create <id>`.
    pub calls: Vec<String>,
    /// The answer to every call (`Ok` by default: `"ok"`).
    pub answer: Option<HostResult>,
    /// What `worktree` declares (it ignores `answer`).
    pub worktree: Option<Worktree>,
}

impl RecordingHost {
    fn reply(&mut self, call: String) -> HostResult {
        self.calls.push(call);
        self.answer.clone().unwrap_or_else(|| Ok("ok".to_owned()))
    }
}

impl Host for RecordingHost {
    fn edit(&mut self, id: &TaskId, file: &std::path::Path) -> HostResult {
        let file: PathBuf = file.to_path_buf();
        self.reply(format!("edit {id} {}", file.display()))
    }

    fn launch(&mut self, id: &TaskId, target: LaunchTarget) -> HostResult {
        let suffix = match target {
            LaunchTarget::Here => "",
            LaunchTarget::Detached { focus: true } => " detached",
            LaunchTarget::Detached { focus: false } => " detached no-focus",
        };
        self.reply(format!("launch {id}{suffix}"))
    }

    fn view(&mut self, id: &TaskId) -> HostResult {
        self.reply(format!("view {id}"))
    }

    fn sync(&mut self, sources: &[String]) -> HostResult {
        let mut call = "sync".to_owned();
        for name in sources {
            call.push(' ');
            call.push_str(name);
        }
        self.reply(call)
    }

    fn after_done(&mut self, task: &Task) -> Result<(), String> {
        self.reply(format!("after_done {}", task.id)).map(|_| ())
    }

    fn worktree(&mut self, task: &Task, branch: Option<&str>) -> Result<Option<Worktree>, String> {
        let branch = branch.map(|b| format!(" {b}")).unwrap_or_default();
        self.calls.push(format!("worktree {}{branch}", task.id));
        Ok(self.worktree.clone())
    }

    fn after_create(&mut self, task: &Task) -> Result<(), String> {
        self.reply(format!("after_create {}", task.id)).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_that_need_the_terminal() {
        let id = TaskId::from(1);
        assert!(Cmd::Editor(id.clone()).releases_terminal());
        // `tasq view`'s pager may quit at once (`less -F` on a short task).
        for pause in [true, false] {
            let view = Cmd::View(id.clone(), pause);
            assert!(view.releases_terminal(), "{view:?}");
            assert_eq!(view.pauses_after(), pause, "{view:?}");
        }
        assert!(Cmd::Launch(id.clone(), LaunchTarget::Here).releases_terminal());
        assert!(Cmd::Sync(Vec::new()).releases_terminal());
        for focus in [true, false] {
            let detached = Cmd::Launch(id.clone(), LaunchTarget::Detached { focus });
            assert!(!detached.releases_terminal(), "{detached:?}");
            assert!(!detached.pauses_after(), "{detached:?}");
        }
        assert!(!Cmd::Load.releases_terminal());
        let before = tasq_core::edit::Target::Before(TaskId::from(2));
        assert!(!Cmd::Move(id.clone(), before.clone()).releases_terminal());
        assert!(!Cmd::Move(id.clone(), before).pauses_after());
        assert!(!Cmd::SetStatus(id.clone(), Status::READY).releases_terminal());
        assert!(!Cmd::Reopen(id.clone(), Status::READY).releases_terminal());
        assert!(!Cmd::Reopen(id.clone(), Status::READY).pauses_after());
        assert!(!Cmd::SetPriority(id.clone(), Priority::A).releases_terminal());
        assert!(!Cmd::Log(id.clone(), "x".into()).releases_terminal());
        assert!(!Cmd::Done(id.clone(), None).releases_terminal());
        assert!(!Cmd::Create(Box::new(TaskDraft::new("x")), None).releases_terminal());
        let fields = Box::new(Fields::of(&Task::new(id.clone(), "x")));
        assert!(!Cmd::Revise(id.clone(), fields.clone()).releases_terminal());
        assert!(!Cmd::Revise(id.clone(), fields).pauses_after());
        assert!(Cmd::Launch(id.clone(), LaunchTarget::Here).pauses_after());
        assert!(Cmd::Sync(vec!["inbox".to_owned()]).pauses_after());
        assert!(!Cmd::Editor(id.clone()).pauses_after());
        assert!(!Cmd::Load.pauses_after());
        assert!(!Cmd::Create(Box::new(TaskDraft::new("x")), None).pauses_after());
    }

    #[test]
    fn hosts() {
        let id = TaskId::from(4);
        let mut none = NoHost;
        assert_eq!(
            none.edit(&id, std::path::Path::new("/f")).unwrap_err(),
            "editing is not available here"
        );
        assert_eq!(
            none.launch(&id, LaunchTarget::Here).unwrap_err(),
            "launching is not available here"
        );
        assert_eq!(none.sync(&[]).unwrap_err(), "sync is not available here");
        assert_eq!(none.view(&id).unwrap_err(), "viewing is not available here");
        // Nothing follows a close or a create for a host that only browses.
        assert_eq!(none.after_done(&Task::new(id.clone(), "T")), Ok(()));
        assert_eq!(none.after_create(&Task::new(id.clone(), "T")), Ok(()));

        let mut rec = RecordingHost::default();
        assert_eq!(rec.edit(&id, std::path::Path::new("/f")).unwrap(), "ok");
        assert_eq!(rec.launch(&id, LaunchTarget::Here).unwrap(), "ok");
        assert_eq!(rec.view(&id).unwrap(), "ok");
        assert_eq!(rec.after_done(&Task::new(id.clone(), "T")), Ok(()));
        assert_eq!(rec.after_create(&Task::new(id.clone(), "T")), Ok(()));
        rec.answer = Some(Err("boom".into()));
        assert_eq!(rec.sync(&[]).unwrap_err(), "boom");
        assert_eq!(
            rec.after_done(&Task::new(id.clone(), "T")).unwrap_err(),
            "boom"
        );
        assert_eq!(
            rec.after_create(&Task::new(id.clone(), "T")).unwrap_err(),
            "boom"
        );
        assert_eq!(
            rec.calls,
            vec![
                "edit 4 /f",
                "launch 4",
                "view 4",
                "after_done 4",
                "after_create 4",
                "sync",
                "after_done 4",
                "after_create 4"
            ]
        );
    }
}
