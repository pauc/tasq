//! The event loop and the command runner.
//!
//! [`dispatch`] runs one [`Cmd`] against the store, the clock and the
//! host and returns the messages for the model; it touches no terminal,
//! so it is tested with the in-memory store and a recording host. [`run`]
//! owns the terminal: raw mode, the alternate screen and bracketed paste,
//! released while an editor, a session or `sync` has the screen.

use std::io::{self, BufRead, Write};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use tasq_core::clock::Clock;
use tasq_core::edit::{self, Value};
use tasq_core::model::{TaskDraft, TaskId};
use tasq_core::query::Filter;
use tasq_core::store::Store;

use crate::keys::translate;
use crate::model::Model;
use crate::msg::{Cmd, Host, Msg};
use crate::update::update;
use crate::view::view;

/// Runs `cmd` and returns what the model should hear: the outcome, then a
/// fresh task list (every command but `Load` reloads, so the screen never
/// shows stale data after an edit, an editor or a sync, successful or not),
/// then, after a create, the new task's id to select.
pub fn dispatch(
    cmd: &Cmd,
    store: &mut dyn Store,
    clock: &dyn Clock,
    host: &mut dyn Host,
) -> Vec<Msg> {
    let mut created = None;
    let outcome: Result<String, String> = match cmd {
        Cmd::Load => return vec![reload(store)],
        Cmd::SetStatus(id, status) => {
            let value = Value::Status(status.clone());
            edit::set(store, id, &value, None, clock)
                .map(|_| format!("[{id}] -> {}", value.label()))
                .map_err(|e| e.to_string())
        }
        Cmd::SetPriority(id, priority) => {
            let value = Value::Priority(*priority);
            edit::set(store, id, &value, None, clock)
                .map(|_| format!("[{id}] -> {}", value.label()))
                .map_err(|e| e.to_string())
        }
        Cmd::Log(id, note) => edit::log(store, id, note, clock)
            .map(|_| format!("[{id}] logged: {note}"))
            .map_err(|e| e.to_string()),
        Cmd::Done(id, note) => close(store, clock, host, id, note.as_deref()),
        Cmd::Create(draft) => {
            let (id, result) = create(store, host, (**draft).clone());
            created = id;
            result
        }
        Cmd::Edit(id) => edit_file(store, host, id),
        Cmd::Launch(id) => host.launch(id),
        Cmd::Sync => host.sync(),
    };
    let first = match outcome {
        Ok(text) => Msg::Info(text),
        Err(text) => Msg::Failed(text),
    };
    let mut msgs = vec![first, reload(store)];
    msgs.extend(created.map(Msg::Select));
    msgs
}

/// `Store::create`, then the host's turn (the CLI's `post-create` hooks).
/// Returns the new id when the write succeeded, whatever the host said,
/// so the UI can select the task; a host warning is reported like a
/// failure but the task exists.
fn create(
    store: &mut dyn Store,
    host: &mut dyn Host,
    draft: TaskDraft,
) -> (Option<TaskId>, Result<String, String>) {
    let task = match store.create(draft) {
        Ok(task) => task,
        Err(e) => return (None, Err(e.to_string())),
    };
    let line = format!("[{}] created: {}", task.id, task.title);
    let result = match host.after_create(&task) {
        Ok(()) => Ok(line),
        Err(warning) => Err(format!("{line} ({warning})")),
    };
    (Some(task.id), result)
}

/// `edit::done`, then the host's turn (the CLI's `post-done` hooks). A
/// host warning does not undo the close: the task is reported done, with
/// the warning, as a failure-styled message so it is noticed.
fn close(
    store: &mut dyn Store,
    clock: &dyn Clock,
    host: &mut dyn Host,
    id: &TaskId,
    note: Option<&str>,
) -> Result<String, String> {
    let task = edit::done(store, id, note, clock).map_err(|e| e.to_string())?;
    let line = format!("[{id}] done: {}", task.title);
    match host.after_done(&task) {
        Ok(()) => Ok(line),
        Err(warning) => Err(format!("{line} ({warning})")),
    }
}

fn edit_file(store: &mut dyn Store, host: &mut dyn Host, id: &TaskId) -> Result<String, String> {
    match store.file_of(id) {
        Ok(Some(path)) => host.edit(id, &path),
        Ok(None) => Err(format!("task {id} has no file to edit in this store")),
        Err(e) => Err(e.to_string()),
    }
}

fn reload(store: &dyn Store) -> Msg {
    match store.list(&Filter::default()) {
        Ok(tasks) => Msg::Loaded(tasks),
        Err(e) => Msg::Failed(e.to_string()),
    }
}

/// Runs the UI until the user quits. Returns the terminal to its previous
/// state on every exit path, panics included.
///
/// Reason: the event loop reads the real terminal; its logic is
/// [`update`], [`dispatch`] and [`view`], which are tested on their own.
#[mutants::skip]
pub fn run(
    mut model: Model,
    store: &mut dyn Store,
    clock: &dyn Clock,
    host: &mut dyn Host,
) -> io::Result<()> {
    let mut screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let size = terminal.size()?;
    update(&mut model, Msg::Resize(size.width, size.height));
    for msg in dispatch(&Cmd::Load, store, clock, host) {
        update(&mut model, msg);
    }
    while !model.quit {
        terminal.draw(|frame| view(&model, frame))?;
        let msg = match event::read()? {
            Event::Key(key) => translate(&model.mode, &key),
            Event::Paste(text) => Some(Msg::Paste(text)),
            Event::Resize(width, height) => Some(Msg::Resize(width, height)),
            Event::FocusGained | Event::FocusLost | Event::Mouse(_) => None,
        };
        let Some(msg) = msg else { continue };
        for cmd in update(&mut model, msg) {
            let results = if cmd.releases_terminal() {
                screen.leave()?;
                let results = dispatch(&cmd, store, clock, host);
                if cmd.pauses_after() {
                    pause()?;
                }
                screen.resume()?;
                terminal.clear()?;
                results
            } else {
                dispatch(&cmd, store, clock, host)
            };
            for msg in results {
                update(&mut model, msg);
            }
        }
    }
    screen.leave()
}

/// Waits for Enter after a command whose output the user should read.
///
/// Reason: reads stdin; nothing to assert without a terminal.
#[mutants::skip]
fn pause() -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\n[tasq] Press Enter to return. ")?;
    stdout.flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(())
}

/// Raw mode plus the alternate screen plus bracketed paste, restored on
/// drop and before a panic message is printed.
struct Screen {
    active: bool,
}

impl Screen {
    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn enter() -> io::Result<Self> {
        let mut screen = Self { active: false };
        screen.resume()?;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            previous(info);
        }));
        Ok(screen)
    }

    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            enable_raw_mode()?;
            execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
            self.active = true;
        }
        Ok(())
    }

    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn leave(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            restore_terminal()?;
        }
        Ok(())
    }
}

impl Drop for Screen {
    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

/// Reason: terminal mode switching; nothing to assert without a terminal.
#[mutants::skip]
fn restore_terminal() -> io::Result<()> {
    execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen)?;
    disable_raw_mode()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;
    use tasq_core::model::{Priority, Status, Task};
    use tasq_core::store::MemoryStore;

    use crate::msg::RecordingHost;

    fn store() -> MemoryStore {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        MemoryStore::new([a, Task::new(TaskId::from(2), "B")])
    }

    fn clock() -> FixedClock {
        FixedClock::at("2026-10-04 10:15")
    }

    fn open_ids(msgs: &[Msg]) -> Vec<&str> {
        match msgs.last() {
            Some(Msg::Loaded(tasks)) => tasks.iter().map(|t| t.id.as_str()).collect(),
            other => panic!("expected a Loaded last, got {other:?}"),
        }
    }

    #[test]
    fn load_lists_open_tasks() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let msgs = dispatch(&Cmd::Load, &mut store, &clock(), &mut host);
        assert_eq!(msgs.len(), 1);
        assert_eq!(open_ids(&msgs), ["1", "2"]);
        assert_eq!(host.calls, Vec::<String>::new());
    }

    #[test]
    fn edits_go_through_core_and_reload() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let id = TaskId::from(1);
        let msgs = dispatch(
            &Cmd::SetStatus(id.clone(), Status::BLOCKED),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] -> blocked".into()));
        assert_eq!(store.get(&id).unwrap().status, Some(Status::BLOCKED));
        assert_eq!(open_ids(&msgs), ["1", "2"]);

        let msgs = dispatch(
            &Cmd::SetPriority(id.clone(), Priority::A),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] -> priority #A".into()));
        assert_eq!(store.get(&id).unwrap().priority, Priority::A);

        let msgs = dispatch(
            &Cmd::Log(id.clone(), "found it".into()),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] logged: found it".into()));
        let task = store.get(&id).unwrap();
        assert_eq!(task.progress.len(), 1);
        assert_eq!(task.progress[0].note, "found it");
        assert_eq!(task.progress[0].at.to_string(), "2026-10-04 10:15");

        let msgs = dispatch(
            &Cmd::Done(id.clone(), Some("merged".into())),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] done: A".into()));
        let task = store.get(&id).unwrap();
        assert!(task.done);
        assert_eq!(task.progress.len(), 2);
        assert_eq!(open_ids(&msgs), ["2"]);

        let msgs = dispatch(
            &Cmd::Done(TaskId::from(2), None),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[2] done: B".into()));
        assert_eq!(open_ids(&msgs), Vec::<&str>::new());
        // Only a close reaches the host, once per task, after the write.
        assert_eq!(host.calls, vec!["after_done 1", "after_done 2"]);
    }

    #[test]
    fn create_writes_the_draft_tells_the_host_and_selects_the_task() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let draft = TaskDraft::new("C").with_status(Some(Status::LATER));
        let msgs = dispatch(
            &Cmd::Create(Box::new(draft)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(
            msgs,
            vec![
                Msg::Info("[3] created: C".into()),
                Msg::Loaded(store.list(&Filter::default()).unwrap()),
                Msg::Select(TaskId::from(3)),
            ]
        );
        let task = store.get(&TaskId::from(3)).unwrap();
        assert_eq!(task.title, "C");
        assert_eq!(task.status, Some(Status::LATER));
        assert_eq!(task.priority, Priority::B);
        assert_eq!(task.progress, Vec::new());
        assert_eq!(open_ids(&msgs[..2]), ["1", "2", "3"]);
        assert_eq!(host.calls, vec!["after_create 3"]);
    }

    #[test]
    fn a_host_warning_after_a_create_is_shown_but_the_task_exists() {
        let mut store = store();
        let mut host = RecordingHost {
            answer: Some(Err("post-create hook \"x\" failed: exit status 4".into())),
            ..RecordingHost::default()
        };
        let msgs = dispatch(
            &Cmd::Create(Box::new(TaskDraft::new("C"))),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(
            msgs[0],
            Msg::Failed("[3] created: C (post-create hook \"x\" failed: exit status 4)".into())
        );
        assert_eq!(msgs[2], Msg::Select(TaskId::from(3)));
        assert_eq!(store.get(&TaskId::from(3)).unwrap().title, "C");
        assert_eq!(host.calls, vec!["after_create 3"]);
    }

    #[test]
    fn a_failed_create_does_not_reach_the_host_and_selects_nothing() {
        struct Full;
        impl Store for Full {
            fn list(&self, _f: &Filter) -> Result<Vec<Task>, tasq_core::store::StoreError> {
                Ok(Vec::new())
            }
            fn get(&self, id: &TaskId) -> Result<Task, tasq_core::store::StoreError> {
                Err(tasq_core::store::StoreError::NotFound(id.clone()))
            }
            fn create(&mut self, _d: TaskDraft) -> Result<Task, tasq_core::store::StoreError> {
                Err(tasq_core::store::StoreError::Unsupported {
                    operation: "read-only store".into(),
                })
            }
            fn update(&mut self, t: &Task) -> Result<(), tasq_core::store::StoreError> {
                Err(tasq_core::store::StoreError::NotFound(t.id.clone()))
            }
            fn set_done(
                &mut self,
                id: &TaskId,
                _d: bool,
            ) -> Result<(), tasq_core::store::StoreError> {
                Err(tasq_core::store::StoreError::NotFound(id.clone()))
            }
            fn describe(&self) -> tasq_core::store::StoreInfo {
                MemoryStore::default().describe()
            }
        }
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::Create(Box::new(TaskDraft::new("C"))),
            &mut Full,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs.len(), 2, "{msgs:?}");
        assert!(matches!(&msgs[0], Msg::Failed(text) if text.contains("read-only store")));
        assert_eq!(host.calls, Vec::<String>::new());
    }

    #[test]
    fn a_host_warning_after_a_close_is_shown_but_the_task_stays_closed() {
        let mut store = store();
        let mut host = RecordingHost {
            answer: Some(Err("post-done hook \"x\" failed: exit status 4".into())),
            ..RecordingHost::default()
        };
        let msgs = dispatch(
            &Cmd::Done(TaskId::from(1), None),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(
            msgs[0],
            Msg::Failed("[1] done: A (post-done hook \"x\" failed: exit status 4)".into())
        );
        assert!(store.get(&TaskId::from(1)).unwrap().done);
        assert_eq!(open_ids(&msgs), ["2"]);
        assert_eq!(host.calls, vec!["after_done 1"]);
    }

    #[test]
    fn a_failed_close_does_not_reach_the_host() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::Done(TaskId::from(9), None),
            &mut store,
            &clock(),
            &mut host,
        );
        assert!(matches!(msgs[0], Msg::Failed(_)), "{:?}", msgs[0]);
        assert_eq!(host.calls, Vec::<String>::new());
    }

    #[test]
    fn failures_are_reported_and_still_reload() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::SetStatus(TaskId::from(9), Status::READY),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("no task with id 9".into()));
        assert_eq!(open_ids(&msgs), ["1", "2"]);
        let msgs = dispatch(
            &Cmd::Log(TaskId::from(1), "  ".into()),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("the note must not be empty".into()));
    }

    #[test]
    fn host_actions() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::Launch(TaskId::from(1)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        assert_eq!(open_ids(&msgs), ["1", "2"]);
        let msgs = dispatch(&Cmd::Sync, &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        // The memory store keeps no files, so there is nothing to edit.
        let msgs = dispatch(&Cmd::Edit(TaskId::from(1)), &mut store, &clock(), &mut host);
        assert_eq!(
            msgs[0],
            Msg::Failed("task 1 has no file to edit in this store".into())
        );
        let msgs = dispatch(&Cmd::Edit(TaskId::from(9)), &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Failed("no task with id 9".into()));
        assert_eq!(host.calls, vec!["launch 1", "sync"]);

        host.answer = Some(Err("claude exited with 1".into()));
        let msgs = dispatch(
            &Cmd::Launch(TaskId::from(2)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("claude exited with 1".into()));
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn edit_asks_the_host_with_the_file() {
        struct FileStore(MemoryStore);
        impl Store for FileStore {
            fn list(&self, f: &Filter) -> Result<Vec<Task>, tasq_core::store::StoreError> {
                self.0.list(f)
            }
            fn get(&self, id: &TaskId) -> Result<Task, tasq_core::store::StoreError> {
                self.0.get(id)
            }
            fn create(
                &mut self,
                d: tasq_core::model::TaskDraft,
            ) -> Result<Task, tasq_core::store::StoreError> {
                self.0.create(d)
            }
            fn update(&mut self, t: &Task) -> Result<(), tasq_core::store::StoreError> {
                self.0.update(t)
            }
            fn set_done(
                &mut self,
                id: &TaskId,
                d: bool,
            ) -> Result<(), tasq_core::store::StoreError> {
                self.0.set_done(id, d)
            }
            fn describe(&self) -> tasq_core::store::StoreInfo {
                self.0.describe()
            }
            fn file_of(
                &self,
                id: &TaskId,
            ) -> Result<Option<std::path::PathBuf>, tasq_core::store::StoreError> {
                self.0.get(id)?;
                Ok(Some(format!("/nb/{id}.todo.md").into()))
            }
        }
        let mut store = FileStore(store());
        let mut host = RecordingHost::default();
        let msgs = dispatch(&Cmd::Edit(TaskId::from(2)), &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        assert_eq!(host.calls, vec!["edit 2 /nb/2.todo.md"]);
        assert_eq!(open_ids(&msgs), ["1", "2"]);
    }
}
