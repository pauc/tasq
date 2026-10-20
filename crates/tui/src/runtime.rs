//! The event loop and the command runner.
//!
//! [`dispatch`] runs one [`Cmd`] against the store, the clock and the
//! host and returns the messages for the model; it touches no terminal,
//! so it is tested with the in-memory store and a recording host. [`run`]
//! owns the terminal: raw mode, the alternate screen, bracketed paste and,
//! where the terminal supports it, the kitty keyboard protocol (so
//! `Ctrl+Enter` and `Shift+Enter` are not plain `Enter`), released while
//! an editor, a session or `sync` has the screen.

use std::io::{self, BufRead, Write};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
    supports_keyboard_enhancement,
};
use tasq_core::clock::Clock;
use tasq_core::edit::{self, Target, Value};
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
        Cmd::SetStatus(id, status) => set(store, id, &Value::Status(status.clone()), clock),
        Cmd::Reopen(id, status) => edit::reopen(store, id, status, None, clock)
            .map(|_| format!("[{id}] reopened -> {status}"))
            .map_err(|e| e.to_string()),
        Cmd::SetPriority(id, priority) => set(store, id, &Value::Priority(*priority), clock),
        Cmd::Log(id, note) => edit::log(store, id, note, clock)
            .map(|_| format!("[{id}] logged: {note}"))
            .map_err(|e| e.to_string()),
        Cmd::Done(id, note) => close(store, clock, host, id, note.as_deref()),
        Cmd::Create(draft, branch) => {
            let (id, result) = create(store, host, (**draft).clone(), branch.as_deref());
            created = id;
            result
        }
        Cmd::Revise(id, fields) => revise(store, id, fields),
        Cmd::Move(id, target) => move_task(store, id, target),
        Cmd::Editor(id) => edit_file(store, host, id),
        Cmd::View(id, _) => host.view(id),
        Cmd::Launch(id, target) => host.launch(id, *target),
        Cmd::Sync(sources) => host.sync(sources),
    };
    let first = match outcome {
        Ok(text) => Msg::Info(text),
        Err(text) => Msg::Failed(text),
    };
    let mut msgs = vec![first, reload(store)];
    msgs.extend(created.map(Msg::Select));
    msgs
}

/// `Store::create`, the worktree the host declares (stored on the task),
/// then the host's turn (the CLI's `post-create` hooks). Returns the new
/// id when the write succeeded, whatever followed, so the UI can select
/// the task; a worktree or host warning is reported like a failure but
/// the task exists.
fn create(
    store: &mut dyn Store,
    host: &mut dyn Host,
    draft: TaskDraft,
    branch: Option<&str>,
) -> (Option<TaskId>, Result<String, String>) {
    let mut task = match store.create(draft) {
        Ok(task) => task,
        Err(e) => return (None, Err(e.to_string())),
    };
    let mut line = format!("[{}] created: {}", task.id, task.title);
    let mut warnings = Vec::new();
    match host.worktree(&task, branch) {
        Ok(Some(worktree)) => {
            task.add_worktree(worktree.clone());
            match store.update(&task).and_then(|()| store.get(&task.id)) {
                Ok(stored) => {
                    task = stored;
                    line = format!("{line}, worktree {}", worktree.path.display());
                }
                Err(e) => warnings.push(e.to_string()),
            }
        }
        Ok(None) => {}
        Err(warning) => warnings.push(warning),
    }
    if let Err(warning) = host.after_create(&task) {
        warnings.push(warning);
    }
    let result = if warnings.is_empty() {
        Ok(line)
    } else {
        Err(format!("{line} ({})", warnings.join("; ")))
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

/// `edit::set`: `[id] -> value`, or `[id] <field> unchanged` when the task
/// already has the value (nothing was written then), as `tasq set` says.
fn set(
    store: &mut dyn Store,
    id: &TaskId,
    value: &Value,
    clock: &dyn Clock,
) -> Result<String, String> {
    let revised = edit::set(store, id, value, None, clock).map_err(|e| e.to_string())?;
    if revised.changed.is_empty() {
        Ok(format!("[{id}] {} unchanged", value.field()))
    } else {
        Ok(format!("[{id}] -> {}", value.label()))
    }
}

/// `edit::revise`: the status line names the changed fields, or says
/// `unchanged` when the form was saved as it was opened.
fn revise(store: &mut dyn Store, id: &TaskId, fields: &edit::Fields) -> Result<String, String> {
    let revised = edit::revise(store, id, fields).map_err(|e| e.to_string())?;
    if revised.changed.is_empty() {
        Ok(format!("[{id}] unchanged"))
    } else {
        Ok(format!("[{id}] updated: {}", revised.changed.join(", ")))
    }
}

/// `edit::move_task`; the status line is what `tasq move` prints
/// ([`edit::Moved::describe`]): `[3] moved before task 1`, `already
/// before` when nothing was written, ` (also ranked: ...)` for the other
/// tasks written.
fn move_task(store: &mut dyn Store, id: &TaskId, target: &Target) -> Result<String, String> {
    edit::move_task(store, id, target)
        .map(|moved| moved.describe(target))
        .map_err(|e| e.to_string())
}

fn edit_file(store: &mut dyn Store, host: &mut dyn Host, id: &TaskId) -> Result<String, String> {
    match store.file_of(id) {
        Ok(Some(path)) => host.edit(id, &path),
        Ok(None) => Err(format!("task {id} has no file to edit in this store")),
        Err(e) => Err(e.to_string()),
    }
}

/// Open and done tasks: the model decides what the list shows, so the
/// DONE group (`a`) needs no store call of its own.
fn reload(store: &dyn Store) -> Msg {
    match store.list(&Filter::default().any_done()) {
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
            Event::Key(key) => translate(&model.keys, &model.mode, &key),
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
    screen.quit()
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

/// Whether the terminal accepted the keyboard enhancement flags, so the
/// restore path knows to pop them (a terminal that never got the push
/// would print the pop sequence as garbage).
static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

/// Raw mode plus the alternate screen plus bracketed paste, restored on
/// drop and before a panic message is printed. When the terminal speaks
/// the kitty keyboard protocol, its disambiguation mode is on too, so
/// `Ctrl+Enter` and `Shift+Enter` reach the key map as such.
struct Screen {
    active: bool,
    /// The terminal size before the alternate screen, waited for on quit.
    before: Option<(u16, u16)>,
}

impl Screen {
    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn enter() -> io::Result<Self> {
        let mut screen = Self {
            active: false,
            before: size().ok(),
        };
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
            if matches!(supports_keyboard_enhancement(), Ok(true)) {
                execute!(
                    io::stdout(),
                    PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    )
                )?;
                KEYBOARD_ENHANCED.store(true, Ordering::Relaxed);
            }
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

    /// Leaves for good, then waits up to 250ms for the terminal to be
    /// back to its size from before the alternate screen. Some multiplexers
    /// (herdr) give a full-screen program one more column than the shell
    /// and shrink the pane back only after it leaves; a shell that reads
    /// the wider size pads its `PROMPT_SP` line past the edge and shows a
    /// stray `%`.
    ///
    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn quit(&mut self) -> io::Result<()> {
        if self.active {
            self.leave()?;
            let deadline = Instant::now() + Duration::from_millis(250);
            while self.before.is_some() && size().ok() != self.before && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    }
}

impl Drop for Screen {
    /// Reason: terminal mode switching; nothing to assert without a terminal.
    #[mutants::skip]
    fn drop(&mut self) {
        let _ = self.quit();
    }
}

/// Reason: terminal mode switching; nothing to assert without a terminal.
#[mutants::skip]
fn restore_terminal() -> io::Result<()> {
    if KEYBOARD_ENHANCED.swap(false, Ordering::Relaxed) {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen)?;
    disable_raw_mode()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;
    use tasq_core::model::{Priority, Status, Task, Worktree};
    use tasq_core::store::MemoryStore;

    use crate::msg::{LaunchTarget, RecordingHost};

    fn store() -> MemoryStore {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        MemoryStore::new([a, Task::new(TaskId::from(2), "B")])
    }

    fn clock() -> FixedClock {
        FixedClock::at("2026-10-04 10:15")
    }

    fn loaded_ids(msgs: &[Msg]) -> Vec<&str> {
        match msgs.last() {
            Some(Msg::Loaded(tasks)) => tasks.iter().map(|t| t.id.as_str()).collect(),
            other => panic!("expected a Loaded last, got {other:?}"),
        }
    }

    #[test]
    fn load_lists_open_and_done_tasks() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let msgs = dispatch(&Cmd::Load, &mut store, &clock(), &mut host);
        assert_eq!(msgs.len(), 1);
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        store.set_done(&TaskId::from(2), true).unwrap();
        let msgs = dispatch(&Cmd::Load, &mut store, &clock(), &mut host);
        assert_eq!(loaded_ids(&msgs), ["1", "2"], "the model hides done tasks");
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
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);

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
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);

        let msgs = dispatch(
            &Cmd::Done(TaskId::from(2), None),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[2] done: B".into()));
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        assert!(store.get(&TaskId::from(2)).unwrap().done);
        // Only a close reaches the host, once per task, after the write.
        assert_eq!(host.calls, vec!["after_done 1", "after_done 2"]);
    }

    #[test]
    fn setting_the_value_a_task_has_says_unchanged_and_writes_nothing() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let id = TaskId::from(1);
        let before = store.get(&id).unwrap();
        let msgs = dispatch(
            &Cmd::SetStatus(id.clone(), Status::READY),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] status unchanged".into()));
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        let msgs = dispatch(
            &Cmd::SetPriority(id.clone(), Priority::B),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] priority unchanged".into()));
        assert_eq!(store.get(&id).unwrap(), before);
    }

    #[test]
    fn a_move_goes_through_edit_move_task_and_says_where_the_task_went() {
        use tasq_core::edit::Target;
        let three = || {
            let mut c = Task::new(TaskId::from(3), "C");
            c.set_status(Status::READY);
            let mut tasks = store().tasks().to_vec();
            tasks.push(c);
            MemoryStore::new(tasks)
        };
        let mut store = three();
        let mut host = RecordingHost::default();
        let before = |id: u64| Cmd::Move(TaskId::from(3), Target::Before(TaskId::from(id)));
        let msgs = dispatch(&before(1), &mut store, &clock(), &mut host);
        assert_eq!(
            msgs,
            vec![
                Msg::Info("[3] moved before task 1".into()),
                Msg::Loaded(store.list(&Filter::default().any_done()).unwrap()),
            ]
        );
        assert_eq!(store.get(&TaskId::from(3)).unwrap().rank, Some(1024));
        assert_eq!(loaded_ids(&msgs), ["1", "2", "3"], "the store lists by id");
        // Already there: nothing written, said as such.
        let msgs = dispatch(&before(1), &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("[3] already before task 1".into()));
        let after = Cmd::Move(TaskId::from(1), Target::After(TaskId::from(3)));
        let msgs = dispatch(&after, &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("[1] already after task 3".into()));
        assert_eq!(store.get(&TaskId::from(1)).unwrap().rank, None);
        // A move that ranks other tasks names them, like `tasq move`.
        let mut fresh = three();
        let msgs = dispatch(&after, &mut fresh, &clock(), &mut host);
        assert_eq!(
            msgs[0],
            Msg::Info("[1] moved after task 3 (also ranked: 3)".into())
        );
        assert_eq!(fresh.get(&TaskId::from(3)).unwrap().rank, Some(1024));
        assert_eq!(fresh.get(&TaskId::from(1)).unwrap().rank, Some(2048));
        // Core's refusals come through as failures, and still reload.
        let other = Cmd::Move(TaskId::from(1), Target::Before(TaskId::from(2)));
        let msgs = dispatch(&other, &mut store, &clock(), &mut host);
        assert_eq!(
            msgs[0],
            Msg::Failed(
                "task 2 is no-status but task 1 is ready; a move stays within one status group"
                    .into()
            )
        );
        assert_eq!(loaded_ids(&msgs), ["1", "2", "3"]);
        let missing = Cmd::Move(TaskId::from(9), Target::Top);
        let msgs = dispatch(&missing, &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Failed("no task with id 9".into()));
        assert_eq!(host.calls, Vec::<String>::new());
    }

    #[test]
    fn reopen_goes_through_edit_reopen() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let id = TaskId::from(2);
        store.set_done(&id, true).unwrap();
        let msgs = dispatch(
            &Cmd::Reopen(id.clone(), Status::WAITING),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[2] reopened -> waiting".into()));
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        let task = store.get(&id).unwrap();
        assert!(!task.done);
        assert_eq!(task.status, Some(Status::WAITING));
        assert_eq!(task.progress.last().unwrap().note, "reopened");
        // An open task is refused by core, and nothing reaches the host.
        let msgs = dispatch(
            &Cmd::Reopen(id, Status::READY),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("task 2 is not done".into()));
        assert_eq!(host.calls, Vec::<String>::new());
    }

    #[test]
    fn revise_writes_the_fields_and_names_the_changed_ones() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let id = TaskId::from(1);
        let task = store.get(&id).unwrap();
        let same = Box::new(edit::Fields::of(&task));
        let msgs = dispatch(
            &Cmd::Revise(id.clone(), same),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] unchanged".into()));
        assert_eq!(msgs.len(), 2);
        let changed = Box::new(edit::Fields {
            title: "Renamed".into(),
            priority: Priority::A,
            ..edit::Fields::of(&task)
        });
        let msgs = dispatch(
            &Cmd::Revise(id.clone(), changed),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[1] updated: title, priority".into()));
        assert!(matches!(&msgs[1], Msg::Loaded(tasks) if tasks[0].title == "Renamed"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(store.get(&id).unwrap().priority, Priority::A);
        let blank = Box::new(edit::Fields {
            title: String::new(),
            ..edit::Fields::of(&task)
        });
        let msgs = dispatch(
            &Cmd::Revise(id.clone(), blank),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("the title must not be empty".into()));
        let gone = Box::new(edit::Fields::of(&task));
        let msgs = dispatch(
            &Cmd::Revise(TaskId::from(9), gone),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("no task with id 9".into()));
        assert_eq!(
            host.calls,
            Vec::<String>::new(),
            "no host action follows an edit"
        );
    }

    #[test]
    fn create_writes_the_draft_tells_the_host_and_selects_the_task() {
        let mut store = store();
        let mut host = RecordingHost::default();
        let draft = TaskDraft::new("C").with_status(Some(Status::LATER));
        let msgs = dispatch(
            &Cmd::Create(Box::new(draft), None),
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
        assert_eq!(loaded_ids(&msgs[..2]), ["1", "2", "3"]);
        assert_eq!(host.calls, vec!["worktree 3", "after_create 3"]);
    }

    #[test]
    fn a_declared_worktree_is_stored_before_the_hooks() {
        let mut store = store();
        let worktree = Worktree::on_branch("/wt", "b");
        let mut host = RecordingHost {
            worktree: Some(worktree.clone()),
            ..RecordingHost::default()
        };
        let msgs = dispatch(
            &Cmd::Create(Box::new(TaskDraft::new("C")), Some("b".into())),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("[3] created: C, worktree /wt".into()));
        assert_eq!(
            store.get(&TaskId::from(3)).unwrap().worktrees,
            vec![worktree]
        );
        assert_eq!(host.calls, vec!["worktree 3 b", "after_create 3"]);
    }

    #[test]
    fn a_host_warning_after_a_create_is_shown_but_the_task_exists() {
        let mut store = store();
        let mut host = RecordingHost {
            answer: Some(Err("post-create hook \"x\" failed: exit status 4".into())),
            ..RecordingHost::default()
        };
        let msgs = dispatch(
            &Cmd::Create(Box::new(TaskDraft::new("C")), None),
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
        assert_eq!(host.calls, vec!["worktree 3", "after_create 3"]);
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
            fn take_warnings(&mut self) -> Vec<String> {
                Vec::new()
            }
            fn doctor(&self) -> Vec<tasq_core::doctor::Check> {
                Vec::new()
            }
        }
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::Create(Box::new(TaskDraft::new("C")), None),
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
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
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
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
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
            &Cmd::Launch(TaskId::from(1), LaunchTarget::Here),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        for focus in [true, false] {
            let msgs = dispatch(
                &Cmd::Launch(TaskId::from(1), LaunchTarget::Window { focus }),
                &mut store,
                &clock(),
                &mut host,
            );
            assert_eq!(msgs[0], Msg::Info("ok".into()));
        }
        let msgs = dispatch(
            &Cmd::View(TaskId::from(2), true),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
        let msgs = dispatch(&Cmd::Sync(Vec::new()), &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        let picked = Cmd::Sync(vec!["gitlab".to_owned(), "inbox".to_owned()]);
        let msgs = dispatch(&picked, &mut store, &clock(), &mut host);
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        // The memory store keeps no files, so there is nothing to edit.
        let msgs = dispatch(
            &Cmd::Editor(TaskId::from(1)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(
            msgs[0],
            Msg::Failed("task 1 has no file to edit in this store".into())
        );
        let msgs = dispatch(
            &Cmd::Editor(TaskId::from(9)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Failed("no task with id 9".into()));
        assert_eq!(
            host.calls,
            vec![
                "launch 1",
                "launch 1 window",
                "launch 1 window no-focus",
                "view 2",
                "sync",
                "sync gitlab inbox"
            ]
        );

        host.answer = Some(Err("claude exited with 1".into()));
        let msgs = dispatch(
            &Cmd::Launch(TaskId::from(2), LaunchTarget::Here),
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
            fn take_warnings(&mut self) -> Vec<String> {
                self.0.take_warnings()
            }
            fn doctor(&self) -> Vec<tasq_core::doctor::Check> {
                self.0.doctor()
            }
            fn file_of(
                &self,
                id: &TaskId,
            ) -> Result<Option<std::path::PathBuf>, tasq_core::store::StoreError> {
                self.0.get(id)?;
                Ok(Some(format!("/store/{id}.md").into()))
            }
        }
        let mut store = FileStore(store());
        let mut host = RecordingHost::default();
        let msgs = dispatch(
            &Cmd::Editor(TaskId::from(2)),
            &mut store,
            &clock(),
            &mut host,
        );
        assert_eq!(msgs[0], Msg::Info("ok".into()));
        assert_eq!(host.calls, vec!["edit 2 /store/2.md"]);
        assert_eq!(loaded_ids(&msgs), ["1", "2"]);
    }
}
