//! Messages into [`crate::update()`] and the commands it asks the runtime to
//! run (the Elm shape: `update(model, msg) -> cmds`).

use std::path::PathBuf;

use tasq_core::model::{Priority, Status, Task, TaskId};

/// Something that happened: a key, translated by [`crate::keys`] for the
/// current mode, a terminal resize, or the result of a [`Cmd`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// Move the selection or the picker cursor up one.
    Up,
    /// Move the selection or the picker cursor down one.
    Down,
    /// Move the selection [`crate::model::PAGE`] rows up.
    PageUp,
    /// Move the selection [`crate::model::PAGE`] rows down.
    PageDown,
    /// Select the first task (`g`).
    Top,
    /// Select the last task (`G`).
    Bottom,
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
    /// `s`: open the status picker.
    BeginStatus,
    /// `p`: open the priority picker.
    BeginPriority,
    /// `l`: start typing a progress note.
    BeginNote,
    /// `d`: start typing the final note, then close the task.
    BeginDone,
    /// `e`: open the task's file in the editor.
    Edit,
    /// `Enter` in normal mode: open a work session.
    Launch,
    /// `S`: run the configured sources.
    Sync,
    /// `r`: reload from the store.
    Reload,
    /// `?`: toggle the help overlay.
    Help,
    /// `Tab`: in the one-pane layout, switch between list and detail.
    ToggleDetail,
    /// `q`: leave.
    Quit,
    /// The terminal is now this size.
    Resize(u16, u16),
    /// Tasks arrived from the store.
    Loaded(Vec<Task>),
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
    /// `tasq set <id> <priority>`.
    SetPriority(TaskId, Priority),
    /// `tasq log <id> <note>`.
    Log(TaskId, String),
    /// `tasq done <id> [note]`.
    Done(TaskId, Option<String>),
    /// Open the task's file in the editor (terminal released meanwhile).
    Edit(TaskId),
    /// Open a work session on the task (terminal released meanwhile).
    Launch(TaskId),
    /// `tasq sync` (terminal released meanwhile).
    Sync,
}

impl Cmd {
    /// Whether the command runs something that needs the terminal for
    /// itself, so the runtime must leave the alternate screen first.
    pub fn releases_terminal(&self) -> bool {
        matches!(self, Self::Edit(_) | Self::Launch(_) | Self::Sync)
    }

    /// Whether the runtime should wait for a key before redrawing, so the
    /// command's output can be read (an editor needs no such pause).
    pub fn pauses_after(&self) -> bool {
        matches!(self, Self::Launch(_) | Self::Sync)
    }
}

/// What a host action reported: a one-line result, shown in the status bar.
pub type HostResult = Result<String, String>;

/// The part of the outside world the TUI cannot reach through the
/// [`tasq_core::store::Store`]: an editor, the launchers and `sync`. The
/// CLI implements it by running itself; tests record the calls.
pub trait Host {
    /// Opens `file` (the task's file) in the user's editor and waits.
    fn edit(&mut self, id: &TaskId, file: &std::path::Path) -> HostResult;

    /// Opens a work session on the task (`tasq pick <id>`) and waits for it.
    fn launch(&mut self, id: &TaskId) -> HostResult;

    /// Runs the configured sources (`tasq sync`).
    fn sync(&mut self) -> HostResult;
}

/// A [`Host`] that refuses everything, for front ends that only browse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoHost;

impl Host for NoHost {
    fn edit(&mut self, _id: &TaskId, _file: &std::path::Path) -> HostResult {
        Err("editing is not available here".to_owned())
    }

    fn launch(&mut self, _id: &TaskId) -> HostResult {
        Err("launching is not available here".to_owned())
    }

    fn sync(&mut self) -> HostResult {
        Err("sync is not available here".to_owned())
    }
}

/// A [`Host`] that records what it was asked and answers with canned
/// results; the test double of this crate and of the CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordingHost {
    /// Every call, as `edit <id> <file>`, `launch <id>` or `sync`.
    pub calls: Vec<String>,
    /// The answer to every call (`Ok` by default: `"ok"`).
    pub answer: Option<HostResult>,
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

    fn launch(&mut self, id: &TaskId) -> HostResult {
        self.reply(format!("launch {id}"))
    }

    fn sync(&mut self) -> HostResult {
        self.reply("sync".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_that_need_the_terminal() {
        let id = TaskId::from(1);
        assert!(Cmd::Edit(id.clone()).releases_terminal());
        assert!(Cmd::Launch(id.clone()).releases_terminal());
        assert!(Cmd::Sync.releases_terminal());
        assert!(!Cmd::Load.releases_terminal());
        assert!(!Cmd::SetStatus(id.clone(), Status::READY).releases_terminal());
        assert!(!Cmd::SetPriority(id.clone(), Priority::A).releases_terminal());
        assert!(!Cmd::Log(id.clone(), "x".into()).releases_terminal());
        assert!(!Cmd::Done(id.clone(), None).releases_terminal());
        assert!(Cmd::Launch(id.clone()).pauses_after());
        assert!(Cmd::Sync.pauses_after());
        assert!(!Cmd::Edit(id.clone()).pauses_after());
        assert!(!Cmd::Load.pauses_after());
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
            none.launch(&id).unwrap_err(),
            "launching is not available here"
        );
        assert_eq!(none.sync().unwrap_err(), "sync is not available here");

        let mut rec = RecordingHost::default();
        assert_eq!(rec.edit(&id, std::path::Path::new("/f")).unwrap(), "ok");
        assert_eq!(rec.launch(&id).unwrap(), "ok");
        rec.answer = Some(Err("boom".into()));
        assert_eq!(rec.sync().unwrap_err(), "boom");
        assert_eq!(rec.calls, vec!["edit 4 /f", "launch 4", "sync"]);
    }
}
