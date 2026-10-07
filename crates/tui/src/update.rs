//! `update(model, msg) -> cmds`: the whole behaviour of the UI, with no
//! I/O, so a test is a sequence of messages and a look at the model.

use tasq_core::model::{Priority, TaskId};

use crate::calendar::Calendar;
use crate::form::{Field, Form, Text};
use crate::history::History;
use crate::model::{Message, Mode, Model, NoteTarget, PAGE};
use crate::msg::{Cmd, LaunchTarget, Msg};

/// Applies `msg` to `model` and returns the commands to run.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Resize(width, height) => {
            model.width = width;
            model.height = height;
            Vec::new()
        }
        Msg::Loaded(tasks) => {
            model.set_tasks(tasks);
            Vec::new()
        }
        Msg::Select(id) => {
            // A task hidden by the filter stays unselected: the message
            // line already says it was created.
            model.select_task(id);
            Vec::new()
        }
        Msg::Info(text) => {
            model.message = Some(Message::info(text));
            Vec::new()
        }
        Msg::Failed(text) => {
            model.message = Some(Message::error(text));
            Vec::new()
        }
        key => {
            model.message = None;
            match model.mode.clone() {
                Mode::Normal => normal(model, &key),
                Mode::Filter { input } => filter(model, input, key),
                Mode::Status { cursor } => status_picker(model, cursor, &key),
                Mode::Priority { cursor } => priority_picker(model, cursor, &key),
                Mode::Sources { cursor } => sources_picker(model, cursor, &key),
                Mode::Note { input, target } => note(model, input, target, key),
                Mode::Create { input } => create(model, input, key),
                Mode::Form(form) => form_mode(model, *form, key),
                Mode::Calendar { form, calendar } => calendar_mode(model, *form, calendar, &key),
                Mode::Help => {
                    if key == Msg::Quit {
                        model.quit = true;
                    } else {
                        model.mode = Mode::Normal;
                    }
                    Vec::new()
                }
            }
        }
    }
}

fn normal(model: &mut Model, msg: &Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Down => model.select_offset(1),
        Msg::Up => model.select_offset(-1),
        Msg::PageDown => model.select_offset(PAGE.cast_signed()),
        Msg::PageUp => model.select_offset(-PAGE.cast_signed()),
        Msg::Top => model.select_first(),
        Msg::Bottom => model.select_last(),
        Msg::ToggleGroup => model.toggle_group(),
        Msg::ToggleDone => model.toggle_done(),
        Msg::Escape => {
            if model.show_detail {
                model.show_detail = false;
            } else if !model.filter.is_empty() {
                model.set_filter(String::new());
            }
        }
        Msg::ToggleDetail => model.show_detail = !model.show_detail,
        Msg::ShowDetail => model.show_detail = true,
        Msg::HideDetail => model.show_detail = false,
        Msg::Help => model.mode = Mode::Help,
        Msg::Reload => return vec![Cmd::Load],
        Msg::BeginFilter => begin_filter(model),
        Msg::BeginPriority | Msg::BeginDone | Msg::Edit if refuse_done(model) => {}
        Msg::BeginStatus => {
            if let Some(task) = model.selected_task() {
                // A done task has no status: the picker reopens it, starting
                // at the status new tasks get.
                let current = if task.done {
                    Some(&model.default_status)
                } else {
                    task.status.as_ref()
                };
                let cursor = current
                    .and_then(|s| model.workflow.position(s))
                    .unwrap_or(0);
                model.mode = Mode::Status { cursor };
            } else {
                model.message = Some(Message::error(NO_SELECTION));
            }
        }
        Msg::BeginPriority => {
            if let Some(task) = model.selected_task() {
                let cursor = Model::priority_choices()
                    .iter()
                    .position(|p| *p == task.priority)
                    .unwrap_or(1);
                model.mode = Mode::Priority { cursor };
            } else {
                model.message = Some(Message::error(NO_SELECTION));
            }
        }
        Msg::BeginNote => begin_note(model, NoteTarget::Log),
        Msg::BeginDone => begin_note(model, NoteTarget::Done),
        Msg::BeginCreate => begin_create(model),
        Msg::Edit => {
            if let Some(task) = model.selected_task() {
                model.mode = Mode::Form(Box::new(Form::of(task, &model.workflow)));
            } else {
                model.message = Some(Message::error(NO_SELECTION));
            }
        }
        Msg::Editor => return with_selection(model, Cmd::Editor),
        Msg::Launch | Msg::Enter => {
            return with_selection(model, |id| Cmd::Launch(id, LaunchTarget::Here));
        }
        Msg::LaunchDetached { focus } => {
            return with_selection(model, |id| {
                Cmd::Launch(id, LaunchTarget::Detached { focus: *focus })
            });
        }
        Msg::Sync => return vec![Cmd::Sync(Vec::new())],
        Msg::BeginSources => {
            if model.sources.is_empty() {
                model.message = Some(Message::error(NO_SOURCES));
            } else {
                model.mode = Mode::Sources { cursor: 0 };
            }
        }
        Msg::Backspace
        | Msg::Char(_)
        | Msg::Paste(_)
        | Msg::Left
        | Msg::Right
        | Msg::Home
        | Msg::End
        | Msg::Delete
        | Msg::DeleteWord
        | Msg::KillToStart
        | Msg::KillToEnd
        | Msg::NextField
        | Msg::PrevField
        | Msg::Save
        | Msg::Today => {}
        Msg::Resize(..) | Msg::Loaded(_) | Msg::Select(_) | Msg::Info(_) | Msg::Failed(_) => {
            unreachable!("handled before the mode dispatch")
        }
    }
    Vec::new()
}

const NO_SELECTION: &str = "no task selected";
const NO_SOURCES: &str = "no [[source]] is configured (see docs/sources.md)";
const NO_SOURCE_CHECKED: &str = "no source checked (Space toggles, Esc closes)";

/// Opens the filter prompt on the applied filter, which is also the
/// draft its recall comes back to.
fn begin_filter(model: &mut Model) {
    model.history.filters.reset();
    model.mode = Mode::Filter {
        input: Text::single(&model.filter),
    };
}

fn begin_create(model: &mut Model) {
    model.history.titles.reset();
    model.mode = Mode::Create {
        input: Text::single(""),
    };
}

fn begin_note(model: &mut Model, target: NoteTarget) {
    if model.selected_task().is_some() {
        model.history.notes.reset();
        model.mode = Mode::Note {
            input: Text::single(""),
            target,
        };
    } else {
        model.message = Some(Message::error(NO_SELECTION));
    }
}

/// Whether the selected task is done, saying so in the status bar: the
/// priority picker, `done` and the edit view need an open task. A done
/// task can still be reopened (`t`), logged on, opened and launched.
fn refuse_done(model: &mut Model) -> bool {
    let Some(id) = model
        .selected_task()
        .filter(|task| task.done)
        .map(|task| task.id.clone())
    else {
        return false;
    };
    model.message = Some(Message::error(format!("[{id}] is done; t reopens it")));
    true
}

/// A command on the selected task (one that is visible, not merely a
/// remembered id), or the no-selection message.
fn with_selection(model: &mut Model, make: impl FnOnce(TaskId) -> Cmd) -> Vec<Cmd> {
    let Some(id) = model.selected_task().map(|task| task.id.clone()) else {
        model.message = Some(Message::error(NO_SELECTION));
        return Vec::new();
    };
    vec![make(id)]
}

/// The filter (`/`): the list follows every edit and every recalled
/// entry, `Enter` keeps the filter (and records it), `Esc` clears it.
fn filter(model: &mut Model, mut input: Text, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Enter => {
            model.history.filters.record(&input.text());
            model.mode = Mode::Normal;
        }
        Msg::Escape => {
            model.set_filter(String::new());
            model.mode = Mode::Normal;
        }
        msg => {
            edit_prompt(&mut input, &mut model.history.filters, &msg);
            model.set_filter(input.text());
            model.mode = Mode::Filter { input };
        }
    }
    Vec::new()
}

/// Applies a message to a status-bar prompt's `input`: `Up`/`Down`
/// replace it with an entry of `history` (or the draft), cursor at the
/// end; anything else goes to [`edit_line`].
fn edit_prompt(input: &mut Text, history: &mut History, msg: &Msg) {
    let recalled = match msg {
        Msg::Up => history.up(&input.text()),
        Msg::Down => history.down(),
        msg => {
            edit_line(input, msg);
            return;
        }
    };
    if let Some(text) = recalled {
        *input = Text::single(&text);
    }
}

/// Applies a line-editing message to a status-bar prompt's `input`:
/// typing, a paste (on one line, see [`one_line`]), `Backspace`,
/// `Delete`, the cursor keys and the kills (`Ctrl+W`, `Ctrl+U`, `Ctrl+K`).
/// Any other message leaves it as it is.
fn edit_line(input: &mut Text, msg: &Msg) {
    match msg {
        Msg::Char(c) => input.insert(*c),
        Msg::Paste(text) => input.paste(&one_line(text)),
        Msg::Backspace => input.backspace(),
        Msg::Delete => input.delete(),
        Msg::DeleteWord => input.delete_word(),
        Msg::KillToStart => input.kill_to_start(),
        Msg::KillToEnd => input.kill_to_end(),
        Msg::Left => {
            input.left();
        }
        Msg::Right => {
            input.right();
        }
        Msg::Home => input.home(),
        Msg::End => input.end(),
        _ => {}
    }
}

fn status_picker(model: &mut Model, cursor: usize, msg: &Msg) -> Vec<Cmd> {
    let count = model.status_choices().len();
    let chosen = match msg {
        Msg::Quit => {
            model.quit = true;
            return Vec::new();
        }
        Msg::Escape => {
            model.mode = Mode::Normal;
            return Vec::new();
        }
        Msg::Down => {
            model.mode = Mode::Status {
                cursor: (cursor + 1).min(count.saturating_sub(1)),
            };
            return Vec::new();
        }
        Msg::Up => {
            model.mode = Mode::Status {
                cursor: cursor.saturating_sub(1),
            };
            return Vec::new();
        }
        Msg::Enter => cursor,
        Msg::Char(c) => match c.to_digit(10) {
            Some(n) if n >= 1 && (n as usize) <= count => n as usize - 1,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let Some(status) = model.status_choices().get(chosen).cloned() else {
        return Vec::new();
    };
    model.mode = Mode::Normal;
    let reopen = model.selected_task().is_some_and(|task| task.done);
    with_selection(model, |id| {
        if reopen {
            Cmd::Reopen(id, status)
        } else {
            Cmd::SetStatus(id, status)
        }
    })
}

fn priority_picker(model: &mut Model, cursor: usize, msg: &Msg) -> Vec<Cmd> {
    let choices = Model::priority_choices();
    let chosen = match msg {
        Msg::Quit => {
            model.quit = true;
            return Vec::new();
        }
        Msg::Escape => {
            model.mode = Mode::Normal;
            return Vec::new();
        }
        Msg::Down => {
            model.mode = Mode::Priority {
                cursor: (cursor + 1).min(choices.len() - 1),
            };
            return Vec::new();
        }
        Msg::Up => {
            model.mode = Mode::Priority {
                cursor: cursor.saturating_sub(1),
            };
            return Vec::new();
        }
        Msg::Enter => choices[cursor],
        Msg::Char(c) => match c.to_ascii_uppercase() {
            'A' => Priority::A,
            'B' => Priority::B,
            'C' => Priority::C,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    model.mode = Mode::Normal;
    with_selection(model, |id| Cmd::SetPriority(id, chosen))
}

/// The source picker: `Space` or a digit toggles, `Enter` runs the checked
/// sources (at least one), `Esc` closes. The checked set stays in the
/// model for the next opening.
fn sources_picker(model: &mut Model, cursor: usize, msg: &Msg) -> Vec<Cmd> {
    let count = model.sources.len();
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Escape => model.mode = Mode::Normal,
        Msg::Down => {
            model.mode = Mode::Sources {
                cursor: (cursor + 1).min(count.saturating_sub(1)),
            };
        }
        Msg::Up => {
            model.mode = Mode::Sources {
                cursor: cursor.saturating_sub(1),
            };
        }
        Msg::Char(' ') => toggle_source(model, cursor),
        Msg::Char(c) => {
            if let Some(n) = c.to_digit(10)
                && n >= 1
                && (n as usize) <= count
            {
                toggle_source(model, n as usize - 1);
            }
        }
        Msg::Enter => {
            let names = model.checked_sources();
            if names.is_empty() {
                model.message = Some(Message::error(NO_SOURCE_CHECKED));
            } else {
                model.mode = Mode::Normal;
                return vec![Cmd::Sync(names)];
            }
        }
        _ => {}
    }
    Vec::new()
}

fn toggle_source(model: &mut Model, index: usize) {
    if let Some(checked) = model.checked.get_mut(index) {
        *checked = !*checked;
    }
}

/// A progress note (`l`) or the final note of `done` (`d`); a submitted
/// note is recorded in the notes history both share.
fn note(model: &mut Model, mut input: Text, target: NoteTarget, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Escape => model.mode = Mode::Normal,
        Msg::Enter => {
            let text = input.text();
            let text = text.trim();
            match target {
                NoteTarget::Log if text.is_empty() => {
                    model.message = Some(Message::error("the note must not be empty"));
                }
                NoteTarget::Log => {
                    model.history.notes.record(text);
                    let note = text.to_owned();
                    model.mode = Mode::Normal;
                    return with_selection(model, |id| Cmd::Log(id, note));
                }
                NoteTarget::Done => {
                    model.history.notes.record(text);
                    let note = (!text.is_empty()).then(|| text.to_owned());
                    model.mode = Mode::Normal;
                    return with_selection(model, |id| Cmd::Done(id, note));
                }
            }
        }
        msg => {
            edit_prompt(&mut input, &mut model.history.notes, &msg);
            model.mode = Mode::Note { input, target };
        }
    }
    Vec::new()
}

/// The title of a new task (`c`): typed like a note, written with the
/// model's draft (and recorded) on Enter. An empty title is refused and
/// the input stays open, as `tasq create` refuses it.
fn create(model: &mut Model, mut input: Text, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Escape => model.mode = Mode::Normal,
        Msg::Enter => {
            let title = input.text();
            let title = title.trim();
            if title.is_empty() {
                model.message = Some(Message::error("the title must not be empty"));
            } else {
                model.history.titles.record(title);
                let draft = model.draft(title);
                model.mode = Mode::Normal;
                return vec![Cmd::Create(Box::new(draft))];
            }
        }
        msg => {
            edit_prompt(&mut input, &mut model.history.titles, &msg);
            model.mode = Mode::Create { input };
        }
    }
    Vec::new()
}

/// The edit view (`e`): rows are typed or cycled in place, `Tab` and the
/// arrows move between them, `Enter` is a newline in the description,
/// the calendar picker on the Due box and the next row elsewhere;
/// `Ctrl+S` saves through [`Cmd::Revise`] when every row validates, else
/// the focus moves to the first bad row and the view stays; `Esc`
/// discards.
fn form_mode(model: &mut Model, mut form: Form, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => {
            model.quit = true;
            return Vec::new();
        }
        Msg::Escape => {
            model.mode = Mode::Normal;
            return Vec::new();
        }
        Msg::NextField => form.focus_next(),
        Msg::PrevField => form.focus_prev(),
        Msg::Up => form.up(),
        Msg::Down => form.down(),
        Msg::Left => form.left(&model.workflow),
        Msg::Right => form.right(&model.workflow),
        Msg::Home => form.home(),
        Msg::End => form.end(),
        Msg::Char(c) => form.insert(c),
        Msg::Paste(text) => form.paste(&text),
        Msg::Backspace => form.backspace(),
        Msg::Delete => form.delete(),
        Msg::DeleteWord => form.delete_word(),
        Msg::KillToStart => form.kill_to_start(),
        Msg::KillToEnd => form.kill_to_end(),
        Msg::Enter => match form.focus {
            Field::Description => form.newline(),
            Field::Due => {
                let calendar = Calendar::open(&form.due.text(), model.today);
                model.mode = Mode::Calendar {
                    form: Box::new(form),
                    calendar,
                };
                return Vec::new();
            }
            _ => form.focus_next(),
        },
        Msg::Save => match form.fields(&model.workflow, model.today) {
            Ok(fields) => {
                model.mode = Mode::Normal;
                return vec![Cmd::Revise(form.id, Box::new(fields))];
            }
            Err(e) => {
                form.focus = e.field;
                model.message = Some(Message::error(e.message));
            }
        },
        _ => {}
    }
    model.mode = Mode::Form(Box::new(form));
    Vec::new()
}

/// The calendar picker over the edit view: the arrows move the cursor by
/// a day or a week, `PageUp`/`PageDown` by a month, `t` to today;
/// `Enter` puts the day in the Due box as ISO and returns to the view,
/// `Esc` returns to it unchanged.
fn calendar_mode(model: &mut Model, mut form: Form, mut calendar: Calendar, msg: &Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => {
            model.quit = true;
            return Vec::new();
        }
        Msg::Escape => {
            model.mode = Mode::Form(Box::new(form));
            return Vec::new();
        }
        Msg::Enter => {
            form.due = Text::single(&calendar.picked());
            model.mode = Mode::Form(Box::new(form));
            return Vec::new();
        }
        Msg::Up => calendar.shift_days(-7),
        Msg::Down => calendar.shift_days(7),
        Msg::Left => calendar.shift_days(-1),
        Msg::Right => calendar.shift_days(1),
        Msg::PageUp => calendar.shift_months(-1),
        Msg::PageDown => calendar.shift_months(1),
        Msg::Today => calendar.today(model.today),
        _ => {}
    }
    model.mode = Mode::Calendar {
        form: Box::new(form),
        calendar,
    };
    Vec::new()
}

/// A progress entry is one line in the file: pasted line breaks become
/// spaces (a trailing newline from the clipboard disappears).
pub fn one_line(text: &str) -> String {
    text.trim_end_matches(['\n', '\r'])
        .replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::Text;
    use crate::model::{Cursor, SourceChoice};
    use tasq_core::edit::Fields;
    use tasq_core::model::{Status, Tag, Task, TaskDraft, Workflow};
    use tasq_core::theme::Theme;

    fn task(id: u64, title: &str, status: Option<Status>) -> Task {
        let mut t = Task::new(TaskId::from(id), title);
        t.status = status;
        t
    }

    fn model() -> Model {
        let mut m = Model::new(Workflow::default(), Theme::default(), true);
        update(
            &mut m,
            Msg::Loaded(vec![
                task(1, "First", Some(Status::IN_PROGRESS)),
                task(2, "Second", Some(Status::READY)),
                task(3, "Third", None),
            ]),
        );
        m
    }

    fn feed(m: &mut Model, msgs: impl IntoIterator<Item = Msg>) -> Vec<Cmd> {
        msgs.into_iter().flat_map(|msg| update(m, msg)).collect()
    }

    fn chars(text: &str) -> Vec<Msg> {
        text.chars().map(Msg::Char).collect()
    }

    #[test]
    fn a_done_task_refuses_the_open_only_edits_but_not_the_rest() {
        let mut m = model();
        let mut closed = task(9, "Closed", None);
        closed.mark_done();
        let mut tasks = m.tasks.clone();
        tasks.push(closed);
        feed(&mut m, [Msg::Loaded(tasks), Msg::ToggleDone, Msg::Bottom]);
        assert_eq!(m.selected_task().map(|t| t.id.as_str()), Some("9"));
        let refused = Some(Message::error("[9] is done; t reopens it"));
        for msg in [Msg::BeginPriority, Msg::BeginDone, Msg::Edit] {
            assert_eq!(update(&mut m, msg.clone()), Vec::new(), "{msg:?}");
            assert_eq!(m.mode, Mode::Normal, "{msg:?}");
            assert_eq!(m.message, refused, "{msg:?}");
        }
        // `t` reopens it: the picker starts at the default status and
        // confirming asks for `tasq reopen`, not `tasq set`.
        update(&mut m, Msg::BeginStatus);
        assert_eq!(m.mode, Mode::Status { cursor: 1 }, "ready, the default");
        assert_eq!(m.message, None);
        update(&mut m, Msg::Up);
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Reopen(TaskId::from(9), Status::IN_PROGRESS)]
        );
        assert_eq!(m.mode, Mode::Normal);
        m.default_status = Status::LATER;
        update(&mut m, Msg::BeginStatus);
        assert_eq!(m.mode, Mode::Status { cursor: 4 });
        update(&mut m, Msg::Escape);
        // Logging a note, the editor and a session still work on it.
        update(&mut m, Msg::BeginNote);
        assert!(matches!(
            m.mode,
            Mode::Note {
                target: NoteTarget::Log,
                ..
            }
        ));
        update(&mut m, Msg::Escape);
        assert_eq!(
            update(&mut m, Msg::Editor),
            vec![Cmd::Editor(TaskId::from(9))]
        );
        // An open task is not refused.
        update(&mut m, Msg::Top);
        update(&mut m, Msg::BeginStatus);
        assert!(matches!(m.mode, Mode::Status { .. }));
        assert_eq!(m.message, None);
    }

    #[test]
    fn resize_and_results() {
        let mut m = model();
        assert_eq!(update(&mut m, Msg::Resize(120, 40)), Vec::new());
        assert_eq!((m.width, m.height), (120, 40));
        update(&mut m, Msg::Info("done".into()));
        assert_eq!(m.message, Some(Message::info("done")));
        update(&mut m, Msg::Failed("boom".into()));
        assert_eq!(m.message, Some(Message::error("boom")));
        // The next key clears the message.
        update(&mut m, Msg::Down);
        assert_eq!(m.message, None);
        // Results do not clear it.
        update(&mut m, Msg::Failed("boom".into()));
        update(&mut m, Msg::Loaded(Vec::new()));
        assert_eq!(m.message, Some(Message::error("boom")));
        assert_eq!(m.selected, None);
    }

    #[test]
    fn navigation() {
        let mut m = model();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        update(&mut m, Msg::Down);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        update(&mut m, Msg::Up);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        update(&mut m, Msg::Bottom);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(3))));
        update(&mut m, Msg::Top);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        update(&mut m, Msg::PageDown);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(3))));
        update(&mut m, Msg::PageUp);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        assert_eq!(update(&mut m, Msg::Reload), vec![Cmd::Load]);
        assert!(!m.quit);
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    #[test]
    fn detail_toggle_and_escape() {
        let mut m = model();
        update(&mut m, Msg::ToggleDetail);
        assert!(m.show_detail);
        update(&mut m, Msg::Escape);
        assert!(!m.show_detail);
        update(&mut m, Msg::ToggleDetail);
        update(&mut m, Msg::ToggleDetail);
        assert!(!m.show_detail);
        update(&mut m, Msg::ShowDetail);
        assert!(m.show_detail);
        update(&mut m, Msg::ShowDetail);
        assert!(m.show_detail);
        update(&mut m, Msg::HideDetail);
        assert!(!m.show_detail);
        update(&mut m, Msg::HideDetail);
        assert!(!m.show_detail);
        feed(&mut m, [Msg::BeginFilter, Msg::Char('T'), Msg::Enter]);
        assert_eq!(m.filter, "T");
        update(&mut m, Msg::Escape);
        assert_eq!(m.filter, "");
        // Nothing to clear: still normal, no message.
        update(&mut m, Msg::Escape);
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.message, None);
        // Typing keys do nothing in normal mode.
        assert_eq!(
            feed(
                &mut m,
                [Msg::Char('x'), Msg::Backspace, Msg::Paste("y".into())]
            ),
            Vec::new()
        );
        assert_eq!(m.filter, "");
    }

    #[test]
    fn help_overlay() {
        let mut m = model();
        update(&mut m, Msg::Help);
        assert_eq!(m.mode, Mode::Help);
        update(&mut m, Msg::Down);
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(
            m.selected,
            Some(Cursor::Task(TaskId::from(1))),
            "the key only closed the help"
        );
        update(&mut m, Msg::Help);
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    #[test]
    fn filter_follows_typing() {
        let mut m = model();
        update(&mut m, Msg::BeginFilter);
        assert_eq!(
            m.mode,
            Mode::Filter {
                input: Text::single("")
            }
        );
        feed(&mut m, chars("sec"));
        assert_eq!(m.filter, "sec");
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        assert_eq!(m.visible().len(), 1);
        update(&mut m, Msg::Backspace);
        assert_eq!(m.filter, "se");
        update(&mut m, Msg::Paste("cond\n".into()));
        assert_eq!(m.filter, "second");
        assert_eq!(
            m.mode,
            Mode::Filter {
                input: Text::single("second")
            }
        );
        update(&mut m, Msg::Enter);
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.filter, "second");
        // Reopening starts from the applied text; Escape clears everything.
        update(&mut m, Msg::BeginFilter);
        assert_eq!(
            m.mode,
            Mode::Filter {
                input: Text::single("second")
            }
        );
        update(&mut m, Msg::Escape);
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.filter, "");
        assert_eq!(m.visible().len(), 3);
        // Other keys are ignored in the filter; Ctrl-C quits.
        update(&mut m, Msg::BeginFilter);
        assert_eq!(
            feed(&mut m, [Msg::Down, Msg::Launch, Msg::Help]),
            Vec::new()
        );
        assert!(matches!(m.mode, Mode::Filter { .. }));
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    /// The prompt's text and cursor column, in any of the prompt modes.
    fn prompt(m: &Model) -> (String, usize) {
        let input = match &m.mode {
            Mode::Filter { input } | Mode::Note { input, .. } | Mode::Create { input } => input,
            other => panic!("not a prompt: {other:?}"),
        };
        assert_eq!(input.lines().len(), 1, "a prompt is one line");
        (input.text(), input.cursor().1)
    }

    #[test]
    fn every_prompt_edits_in_the_middle() {
        for begin in [
            Msg::BeginFilter,
            Msg::BeginNote,
            Msg::BeginDone,
            Msg::BeginCreate,
        ] {
            let mut m = model();
            update(&mut m, begin.clone());
            feed(&mut m, chars("acd"));
            feed(&mut m, [Msg::Left, Msg::Left, Msg::Char('b')]);
            assert_eq!(prompt(&m), ("abcd".into(), 2), "{begin:?}: typed mid-line");
            feed(&mut m, [Msg::Home, Msg::Char('>')]);
            assert_eq!(prompt(&m), (">abcd".into(), 1), "{begin:?}: Home");
            update(&mut m, Msg::Delete);
            assert_eq!(prompt(&m), (">bcd".into(), 1), "{begin:?}: Delete");
            feed(&mut m, [Msg::End, Msg::Left, Msg::Backspace]);
            assert_eq!(
                prompt(&m),
                (">bd".into(), 2),
                "{begin:?}: Backspace mid-line"
            );
            update(&mut m, Msg::Right);
            assert_eq!(prompt(&m), (">bd".into(), 3), "{begin:?}: Right");
            update(&mut m, Msg::Right);
            assert_eq!(prompt(&m), (">bd".into(), 3), "{begin:?}: Right at the end");
            feed(&mut m, [Msg::Home, Msg::Right]);
            update(&mut m, Msg::Paste("x\r\ny\n".into()));
            assert_eq!(
                prompt(&m),
                (">x ybd".into(), 4),
                "{begin:?}: a paste in the middle, on one line"
            );
            assert_eq!(
                feed(&mut m, [Msg::Up, Msg::Down, Msg::NextField]),
                Vec::new()
            );
            assert_eq!(prompt(&m), (">x ybd".into(), 4), "{begin:?}: ignored keys");
            let cmds = update(&mut m, Msg::Enter);
            assert_eq!(m.mode, Mode::Normal, "{begin:?}");
            let expected = match begin {
                Msg::BeginFilter => Vec::new(),
                Msg::BeginNote => vec![Cmd::Log(TaskId::from(1), ">x ybd".into())],
                Msg::BeginDone => vec![Cmd::Done(TaskId::from(1), Some(">x ybd".into()))],
                _ => vec![Cmd::Create(Box::new(
                    TaskDraft::new(">x ybd").with_status(Some(Status::READY)),
                ))],
            };
            assert_eq!(cmds, expected, "{begin:?}");
        }
    }

    #[test]
    fn every_prompt_takes_the_readline_kills() {
        for begin in [
            Msg::BeginFilter,
            Msg::BeginNote,
            Msg::BeginDone,
            Msg::BeginCreate,
        ] {
            let mut m = model();
            update(&mut m, begin.clone());
            feed(&mut m, chars("one two  three"));
            assert_eq!(update(&mut m, Msg::DeleteWord), Vec::new());
            assert_eq!(prompt(&m), ("one two  ".into(), 9), "{begin:?}: Ctrl+W");
            update(&mut m, Msg::DeleteWord);
            assert_eq!(prompt(&m), ("one ".into(), 4), "{begin:?}: Ctrl+W again");
            feed(&mut m, chars("two"));
            feed(&mut m, [Msg::Left, Msg::Left]);
            assert_eq!(update(&mut m, Msg::KillToEnd), Vec::new());
            assert_eq!(prompt(&m), ("one t".into(), 5), "{begin:?}: Ctrl+K");
            update(&mut m, Msg::Left);
            assert_eq!(update(&mut m, Msg::KillToStart), Vec::new());
            assert_eq!(prompt(&m), ("t".into(), 0), "{begin:?}: Ctrl+U");
            update(&mut m, Msg::DeleteWord);
            assert_eq!(
                prompt(&m),
                ("t".into(), 0),
                "{begin:?}: Ctrl+W at the start"
            );
            let cmds = update(&mut m, Msg::Enter);
            let expected = match begin {
                Msg::BeginFilter => Vec::new(),
                Msg::BeginNote => vec![Cmd::Log(TaskId::from(1), "t".into())],
                Msg::BeginDone => vec![Cmd::Done(TaskId::from(1), Some("t".into()))],
                _ => vec![Cmd::Create(Box::new(
                    TaskDraft::new("t").with_status(Some(Status::READY)),
                ))],
            };
            assert_eq!(cmds, expected, "{begin:?}");
        }
    }

    #[test]
    fn the_filter_follows_the_kills() {
        let mut m = model();
        update(&mut m, Msg::BeginFilter);
        feed(&mut m, chars("second x"));
        assert_eq!(m.visible().len(), 0);
        update(&mut m, Msg::DeleteWord);
        assert_eq!(m.filter, "second ");
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        feed(&mut m, [Msg::Home, Msg::Right, Msg::Right, Msg::Right]);
        update(&mut m, Msg::KillToEnd);
        assert_eq!(m.filter, "sec");
        update(&mut m, Msg::KillToStart);
        assert_eq!(m.filter, "");
        assert_eq!(m.visible().len(), 3);
    }

    #[test]
    fn the_filter_follows_edits_in_the_middle() {
        let mut m = model();
        update(&mut m, Msg::BeginFilter);
        feed(&mut m, chars("scond"));
        assert_eq!(m.filter, "scond");
        assert_eq!(m.visible().len(), 0);
        feed(&mut m, [Msg::Home, Msg::Right]);
        assert_eq!(m.filter, "scond", "moving the cursor keeps the filter");
        update(&mut m, Msg::Char('e'));
        assert_eq!(m.filter, "second");
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        feed(&mut m, [Msg::End, Msg::Left, Msg::Left, Msg::Delete]);
        assert_eq!(m.filter, "secod");
        assert_eq!(m.visible().len(), 0);
        update(&mut m, Msg::Backspace);
        assert_eq!(m.filter, "secd");
        update(&mut m, Msg::Paste("on".into()));
        assert_eq!(m.filter, "second");
        assert_eq!(m.visible().len(), 1);
        feed(&mut m, [Msg::Home, Msg::Delete, Msg::Delete, Msg::Delete]);
        assert_eq!(m.filter, "ond");
        update(&mut m, Msg::Enter);
        assert_eq!(m.filter, "ond");
        update(&mut m, Msg::BeginFilter);
        assert_eq!(
            prompt(&m),
            ("ond".into(), 3),
            "reopened with the cursor at the end"
        );
    }

    #[test]
    fn status_picker_flow() {
        let mut m = model();
        update(&mut m, Msg::Down); // task 2, ready
        update(&mut m, Msg::BeginStatus);
        assert_eq!(m.mode, Mode::Status { cursor: 1 });
        update(&mut m, Msg::Down);
        update(&mut m, Msg::Down);
        assert_eq!(m.mode, Mode::Status { cursor: 3 });
        feed(&mut m, [Msg::Down, Msg::Down, Msg::Down]);
        assert_eq!(m.mode, Mode::Status { cursor: 4 }, "clamped at the end");
        feed(&mut m, std::iter::repeat_n(Msg::Up, 9));
        assert_eq!(m.mode, Mode::Status { cursor: 0 });
        let cmds = update(&mut m, Msg::Enter);
        assert_eq!(
            cmds,
            vec![Cmd::SetStatus(TaskId::from(2), Status::IN_PROGRESS)]
        );
        assert_eq!(m.mode, Mode::Normal);

        // Digits pick directly; out-of-range digits and letters do nothing.
        update(&mut m, Msg::BeginStatus);
        assert_eq!(
            feed(&mut m, [Msg::Char('9'), Msg::Char('0'), Msg::Char('x')]),
            Vec::new()
        );
        assert!(matches!(m.mode, Mode::Status { .. }));
        assert_eq!(
            update(&mut m, Msg::Char('4')),
            vec![Cmd::SetStatus(TaskId::from(2), Status::BLOCKED)]
        );

        // Escape leaves without a command; ignored keys stay; Ctrl-C quits.
        update(&mut m, Msg::BeginStatus);
        assert_eq!(update(&mut m, Msg::Launch), Vec::new());
        assert_eq!(update(&mut m, Msg::Escape), Vec::new());
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginStatus);
        update(&mut m, Msg::Quit);
        assert!(m.quit);

        // A task without a status opens at the first entry.
        let mut m = model();
        update(&mut m, Msg::Bottom);
        update(&mut m, Msg::BeginStatus);
        assert_eq!(m.mode, Mode::Status { cursor: 0 });
    }

    fn sources() -> Vec<SourceChoice> {
        let choice = |name: &str, kind: &str, auto| SourceChoice {
            name: name.to_owned(),
            kind: kind.to_owned(),
            auto,
        };
        vec![
            choice("gitlab", "gitlab-review-requests", true),
            choice("issues", "gitlab-work-items", true),
            choice("inbox", "llm-bridge", false),
        ]
    }

    #[test]
    fn source_picker_flow() {
        let mut m = model().with_sources(sources());
        assert_eq!(
            m.checked,
            vec![true, true, false],
            "auto sources start checked"
        );
        update(&mut m, Msg::BeginSources);
        assert_eq!(m.mode, Mode::Sources { cursor: 0 });
        feed(&mut m, [Msg::Down, Msg::Down, Msg::Down]);
        assert_eq!(m.mode, Mode::Sources { cursor: 2 }, "clamped at the end");
        assert_eq!(update(&mut m, Msg::Char(' ')), Vec::new());
        assert_eq!(
            m.checked,
            vec![true, true, true],
            "Space toggles under the cursor"
        );
        feed(&mut m, [Msg::Up, Msg::Up, Msg::Up]);
        assert_eq!(m.mode, Mode::Sources { cursor: 0 });
        update(&mut m, Msg::Char('1'));
        assert_eq!(
            m.checked,
            vec![false, true, true],
            "digits toggle by position"
        );
        feed(
            &mut m,
            [
                Msg::Char('3'),
                Msg::Char('4'),
                Msg::Char('0'),
                Msg::Char('x'),
            ],
        );
        assert_eq!(
            m.checked,
            vec![false, true, false],
            "the last digit works; out-of-range digits and letters do nothing"
        );
        assert_eq!(update(&mut m, Msg::Launch), Vec::new(), "ignored keys stay");
        assert!(matches!(m.mode, Mode::Sources { .. }));
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Sync(vec!["issues".to_owned()])]
        );
        assert_eq!(m.mode, Mode::Normal);

        // The checked set is remembered; Escape keeps the toggles made meanwhile.
        update(&mut m, Msg::BeginSources);
        assert_eq!(m.checked, vec![false, true, false]);
        update(&mut m, Msg::Char(' '));
        assert_eq!(update(&mut m, Msg::Escape), Vec::new());
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.checked, vec![true, true, false]);

        // Enter with nothing checked says so and stays open; Ctrl-C quits.
        update(&mut m, Msg::BeginSources);
        feed(&mut m, [Msg::Char('1'), Msg::Char('2')]);
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(m.mode, Mode::Sources { cursor: 0 });
        assert_eq!(
            m.message,
            Some(Message::error(
                "no source checked (Space toggles, Esc closes)"
            ))
        );
        update(&mut m, Msg::Quit);
        assert!(m.quit);

        // Without sources the picker does not open.
        let mut m = model();
        assert_eq!(update(&mut m, Msg::BeginSources), Vec::new());
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(
            m.message,
            Some(Message::error(
                "no [[source]] is configured (see docs/sources.md)"
            ))
        );
        // `s` is a bare sync whatever is checked.
        let mut m = model().with_sources(sources());
        update(&mut m, Msg::BeginSources);
        feed(&mut m, [Msg::Char('1'), Msg::Escape]);
        assert_eq!(update(&mut m, Msg::Sync), vec![Cmd::Sync(Vec::new())]);
    }

    #[test]
    fn status_picker_with_an_empty_workflow_does_nothing() {
        let mut m = Model::new(Workflow::new(Vec::new()), Theme::default(), true);
        update(&mut m, Msg::Loaded(vec![task(1, "x", None)]));
        update(&mut m, Msg::BeginStatus);
        assert_eq!(update(&mut m, Msg::Down), Vec::new());
        assert_eq!(m.mode, Mode::Status { cursor: 0 });
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(m.mode, Mode::Status { cursor: 0 });
    }

    #[test]
    fn priority_picker_flow() {
        let mut m = model();
        update(&mut m, Msg::BeginPriority);
        assert_eq!(
            m.mode,
            Mode::Priority { cursor: 1 },
            "B is the current priority"
        );
        feed(&mut m, [Msg::Down, Msg::Down]);
        assert_eq!(m.mode, Mode::Priority { cursor: 2 });
        feed(&mut m, [Msg::Up, Msg::Up, Msg::Up]);
        assert_eq!(m.mode, Mode::Priority { cursor: 0 });
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::SetPriority(TaskId::from(1), Priority::A)]
        );
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginPriority);
        assert_eq!(update(&mut m, Msg::Char('x')), Vec::new());
        assert_eq!(update(&mut m, Msg::Launch), Vec::new());
        assert_eq!(
            update(&mut m, Msg::Char('c')),
            vec![Cmd::SetPriority(TaskId::from(1), Priority::C)]
        );
        update(&mut m, Msg::BeginPriority);
        assert_eq!(
            update(&mut m, Msg::Char('a')),
            vec![Cmd::SetPriority(TaskId::from(1), Priority::A)]
        );
        update(&mut m, Msg::BeginPriority);
        assert_eq!(
            update(&mut m, Msg::Char('A')),
            vec![Cmd::SetPriority(TaskId::from(1), Priority::A)]
        );
        update(&mut m, Msg::BeginPriority);
        assert_eq!(
            update(&mut m, Msg::Char('B')),
            vec![Cmd::SetPriority(TaskId::from(1), Priority::B)]
        );
        update(&mut m, Msg::BeginPriority);
        update(&mut m, Msg::Escape);
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginPriority);
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    #[test]
    fn note_flow() {
        let mut m = model();
        update(&mut m, Msg::BeginNote);
        assert_eq!(
            m.mode,
            Mode::Note {
                input: Text::single(""),
                target: NoteTarget::Log
            }
        );
        // Empty notes are refused and the input stays open.
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(
            m.message,
            Some(Message::error("the note must not be empty"))
        );
        assert!(matches!(m.mode, Mode::Note { .. }));
        feed(&mut m, chars("found "));
        update(&mut m, Msg::Paste("the\r\ncause\n".into()));
        update(&mut m, Msg::Char('!'));
        update(&mut m, Msg::Backspace);
        assert_eq!(
            m.mode,
            Mode::Note {
                input: Text::single("found the cause"),
                target: NoteTarget::Log
            }
        );
        assert_eq!(update(&mut m, Msg::Down), Vec::new());
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Log(TaskId::from(1), "found the cause".into())]
        );
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginNote);
        update(&mut m, Msg::Char('x'));
        update(&mut m, Msg::Escape);
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginNote);
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    #[test]
    fn done_flow() {
        let mut m = model();
        update(&mut m, Msg::BeginDone);
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Done(TaskId::from(1), None)]
        );
        update(&mut m, Msg::BeginDone);
        feed(&mut m, chars("  merged "));
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Done(TaskId::from(1), Some("merged".into()))]
        );
    }

    #[test]
    fn create_flow() {
        let mut m = model();
        update(&mut m, Msg::BeginCreate);
        assert_eq!(
            m.mode,
            Mode::Create {
                input: Text::single("")
            }
        );
        // An empty title is refused and the input stays open.
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(
            m.message,
            Some(Message::error("the title must not be empty"))
        );
        assert!(matches!(m.mode, Mode::Create { .. }));
        feed(&mut m, chars("  Call "));
        update(&mut m, Msg::Paste("the\nbank\n".into()));
        update(&mut m, Msg::Char('!'));
        update(&mut m, Msg::Backspace);
        assert_eq!(
            m.mode,
            Mode::Create {
                input: Text::single("  Call the bank")
            }
        );
        assert_eq!(update(&mut m, Msg::Down), Vec::new(), "nothing to recall");
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Create(Box::new(
                TaskDraft::new("Call the bank").with_status(Some(Status::READY))
            ))]
        );
        assert_eq!(m.mode, Mode::Normal);
        // The configured default status goes into the draft.
        let mut m = model().with_default_status(Status::LATER);
        feed(&mut m, [Msg::BeginCreate, Msg::Char('x')]);
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Create(Box::new(
                TaskDraft::new("x").with_status(Some(Status::LATER))
            ))]
        );
        // Escape drops the typed title; Ctrl-C quits.
        update(&mut m, Msg::BeginCreate);
        update(&mut m, Msg::Char('x'));
        update(&mut m, Msg::Escape);
        assert_eq!(m.mode, Mode::Normal);
        update(&mut m, Msg::BeginCreate);
        update(&mut m, Msg::Quit);
        assert!(m.quit);
    }

    #[test]
    fn create_needs_no_selection_and_the_new_task_gets_selected() {
        let mut m = model();
        update(&mut m, Msg::Loaded(Vec::new()));
        update(&mut m, Msg::BeginCreate);
        assert_eq!(m.message, None);
        assert!(matches!(m.mode, Mode::Create { .. }));
        feed(&mut m, chars("New"));
        assert_eq!(update(&mut m, Msg::Enter).len(), 1);
        // What dispatch sends back after the write.
        update(&mut m, Msg::Info("[7] created: New".into()));
        update(
            &mut m,
            Msg::Loaded(vec![
                task(1, "First", Some(Status::IN_PROGRESS)),
                task(7, "New", Some(Status::READY)),
            ]),
        );
        assert_eq!(
            m.selected,
            Some(Cursor::Task(TaskId::from(1))),
            "the reload keeps the first"
        );
        update(&mut m, Msg::Select(TaskId::from(7)));
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(7))));
        assert_eq!(
            m.message,
            Some(Message::info("[7] created: New")),
            "selecting is a result, not a key: the message stays"
        );
        // A new task hidden by the filter is not selected.
        m.set_filter("First".into());
        update(&mut m, Msg::Select(TaskId::from(7)));
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        // An unknown id is ignored too.
        m.set_filter(String::new());
        update(&mut m, Msg::Select(TaskId::from(99)));
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
    }

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    }

    fn form_of(m: &Model) -> Form {
        match &m.mode {
            Mode::Form(form) => (**form).clone(),
            other => panic!("not in the form: {other:?}"),
        }
    }

    #[test]
    fn form_flow() {
        let mut m = model().with_today(today());
        m.set_filter("nothing".into());
        assert_eq!(update(&mut m, Msg::Edit), Vec::new());
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.message, Some(Message::error(NO_SELECTION)));
        m.set_filter(String::new());
        assert_eq!(update(&mut m, Msg::Edit), Vec::new());
        let form = form_of(&m);
        assert_eq!(form.id, TaskId::from(1));
        assert_eq!(form.title.text(), "First");
        assert_eq!(form.focus, Field::Title);
        assert_eq!(m.message, None);

        assert_eq!(feed(&mut m, chars(" bis")), Vec::new());
        assert_eq!(form_of(&m).title.text(), "First bis");
        assert_eq!(feed(&mut m, [Msg::Home, Msg::Delete, Msg::End]), Vec::new());
        assert_eq!(form_of(&m).title.text(), "irst bis");
        assert_eq!(
            feed(&mut m, [Msg::Left, Msg::Char('!'), Msg::Right]),
            Vec::new()
        );
        assert_eq!(form_of(&m).title.text(), "irst bi!s");
        let mut killed = m.clone();
        assert_eq!(update(&mut killed, Msg::DeleteWord), Vec::new());
        assert_eq!(form_of(&killed).title.text(), "irst ", "Ctrl+W");
        feed(&mut killed, [Msg::Left, Msg::Left]);
        assert_eq!(update(&mut killed, Msg::KillToEnd), Vec::new());
        assert_eq!(form_of(&killed).title.text(), "irs", "Ctrl+K");
        update(&mut killed, Msg::Left);
        assert_eq!(update(&mut killed, Msg::KillToStart), Vec::new());
        assert_eq!(form_of(&killed).title.text(), "s", "Ctrl+U");
        update(&mut killed, Msg::NextField);
        let before = form_of(&killed);
        assert_eq!(before.focus, Field::Status);
        assert_eq!(
            feed(
                &mut killed,
                [Msg::DeleteWord, Msg::KillToStart, Msg::KillToEnd]
            ),
            Vec::new()
        );
        assert_eq!(form_of(&killed), before, "a choice row ignores the kills");
        assert_eq!(feed(&mut m, [Msg::Enter, Msg::Right]), Vec::new());
        let form = form_of(&m);
        assert_eq!(form.focus, Field::Status, "Enter moves on");
        assert_eq!(form.chosen_status(&m.workflow), Some(Status::READY));
        assert_eq!(feed(&mut m, [Msg::Down, Msg::Left]), Vec::new());
        assert_eq!(form_of(&m).chosen_priority(), Priority::A);
        assert_eq!(update(&mut m, Msg::NextField), Vec::new());
        assert_eq!(feed(&mut m, chars("tomorrow")), Vec::new());
        assert_eq!(
            feed(&mut m, [Msg::NextField, Msg::Paste("/p\n".into())]),
            Vec::new()
        );
        assert_eq!(form_of(&m).project.text(), "/p");
        assert_eq!(update(&mut m, Msg::Down), Vec::new());
        assert_eq!(feed(&mut m, chars("a #b")), Vec::new());
        assert_eq!(feed(&mut m, [Msg::Down, Msg::Top]), Vec::new());
        assert_eq!(form_of(&m).focus, Field::Description);
        assert_eq!(feed(&mut m, chars("why")), Vec::new());
        assert_eq!(feed(&mut m, [Msg::Enter, Msg::Enter]), Vec::new());
        assert_eq!(feed(&mut m, [Msg::Paste("how\n".into())]), Vec::new());
        assert_eq!(feed(&mut m, [Msg::Down, Msg::NextField]), Vec::new());
        assert_eq!(
            form_of(&m).focus,
            Field::Description,
            "the bottom row stays"
        );
        assert_eq!(form_of(&m).description.lines(), ["why", "", "how", ""]);
        assert_eq!(feed(&mut m, [Msg::PrevField, Msg::Up]), Vec::new());
        assert_eq!(form_of(&m).focus, Field::Project);

        let cmds = update(&mut m, Msg::Save);
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(
            cmds,
            vec![Cmd::Revise(
                TaskId::from(1),
                Box::new(Fields {
                    title: "irst bi!s".into(),
                    description: Some("why\n\nhow".into()),
                    status: Some(Status::READY),
                    priority: Priority::A,
                    due: Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()),
                    project: Some("/p".into()),
                    tags: vec![Tag::new("a").unwrap(), Tag::new("b").unwrap()],
                })
            )]
        );
    }

    #[test]
    fn form_stays_open_on_bad_input_and_closes_on_escape() {
        let mut m = model().with_today(today());
        update(&mut m, Msg::Edit);
        assert_eq!(feed(&mut m, [Msg::Down, Msg::Down, Msg::Down]), Vec::new());
        assert_eq!(form_of(&m).focus, Field::Due);
        feed(&mut m, chars("soon"));
        assert_eq!(update(&mut m, Msg::Up), Vec::new());
        assert_eq!(form_of(&m).focus, Field::Priority);
        assert_eq!(update(&mut m, Msg::Save), Vec::new());
        let form = form_of(&m);
        assert_eq!(form.focus, Field::Due, "the focus goes to the bad row");
        assert_eq!(form.due.text(), "soon");
        let message = m.message.clone().expect("an error");
        assert!(message.is_error);
        assert!(message.text.starts_with("due: "), "{}", message.text);
        for _ in 0..4 {
            update(&mut m, Msg::Backspace);
        }
        assert_eq!(m.message, None, "typing clears the message");
        feed(&mut m, [Msg::Up, Msg::Up, Msg::Up, Msg::Up]);
        assert_eq!(form_of(&m).focus, Field::Title, "the top row stays");
        for _ in 0..5 {
            update(&mut m, Msg::Backspace);
        }
        assert_eq!(form_of(&m).title.text(), "");
        assert_eq!(update(&mut m, Msg::Save), Vec::new());
        assert_eq!(
            m.message,
            Some(Message::error("the title must not be empty"))
        );
        assert_eq!(form_of(&m).focus, Field::Title);
        assert_eq!(update(&mut m, Msg::Escape), Vec::new());
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.message, None);
        assert_eq!(
            m.tasks[0].title, "First",
            "the view never touches the model's tasks"
        );
        update(&mut m, Msg::Edit);
        assert_eq!(update(&mut m, Msg::Quit), Vec::new());
        assert!(m.quit);
        let mut m = model();
        assert_eq!(
            feed(
                &mut m,
                [
                    Msg::Left,
                    Msg::Right,
                    Msg::Home,
                    Msg::End,
                    Msg::Delete,
                    Msg::DeleteWord,
                    Msg::KillToStart,
                    Msg::KillToEnd,
                    Msg::NextField,
                    Msg::PrevField,
                    Msg::Save
                ]
            ),
            Vec::new()
        );
        assert_eq!(m.mode, Mode::Normal, "nothing in normal mode");
        let _ = Text::single;
    }

    fn form_of_calendar(m: &Model) -> Form {
        calendar_of(m).0
    }

    fn calendar_of(m: &Model) -> (Form, Calendar) {
        match &m.mode {
            Mode::Calendar { form, calendar } => ((**form).clone(), *calendar),
            other => panic!("not in the calendar: {other:?}"),
        }
    }

    fn day(text: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn calendar_flow() {
        let mut m = model().with_today(today());
        update(&mut m, Msg::Edit);
        feed(&mut m, [Msg::NextField, Msg::NextField, Msg::NextField]);
        assert_eq!(form_of(&m).focus, Field::Due);
        // An empty Due box opens on today.
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        let (form, calendar) = calendar_of(&m);
        assert_eq!(form.focus, Field::Due);
        assert_eq!(form.due.text(), "");
        assert_eq!(calendar.day, today());
        assert_eq!(m.message, None);
        // The keys move the cursor; the view underneath is untouched.
        assert_eq!(feed(&mut m, [Msg::Down, Msg::Right]), Vec::new());
        assert_eq!(calendar_of(&m).1.day, day("2026-10-13"));
        assert_eq!(feed(&mut m, [Msg::Up, Msg::Left]), Vec::new());
        assert_eq!(calendar_of(&m).1.day, today());
        assert_eq!(feed(&mut m, [Msg::PageDown, Msg::PageDown]), Vec::new());
        assert_eq!(calendar_of(&m).1.day, day("2026-12-05"));
        assert_eq!(update(&mut m, Msg::PageUp), Vec::new());
        assert_eq!(calendar_of(&m).1.day, day("2026-11-05"));
        assert_eq!(update(&mut m, Msg::Today), Vec::new());
        assert_eq!(calendar_of(&m).1.day, today());
        assert_eq!(update(&mut m, Msg::Right), Vec::new());
        assert_eq!(
            feed(
                &mut m,
                [
                    Msg::Char('x'),
                    Msg::Paste("y".into()),
                    Msg::Backspace,
                    Msg::Delete,
                    Msg::Home,
                    Msg::End,
                    Msg::NextField,
                    Msg::PrevField,
                    Msg::Save,
                    Msg::Top,
                    Msg::Bottom,
                    Msg::Help,
                ]
            ),
            Vec::new()
        );
        let (form, calendar) = calendar_of(&m);
        assert_eq!(calendar.day, day("2026-10-06"), "the other keys do nothing");
        assert_eq!(form.due.text(), "", "typing never reaches the view");
        assert_eq!(form.focus, Field::Due);
        // Enter puts the day in the Due box as ISO and returns to the view.
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        let form = form_of(&m);
        assert_eq!(form.due.text(), "2026-10-06");
        assert_eq!(form.due.cursor(), (0, 10));
        assert_eq!(form.focus, Field::Due);
        // Reopening starts on the typed day; Esc keeps the box as it was.
        feed(
            &mut m,
            [
                Msg::Backspace,
                Msg::Backspace,
                Msg::Char('3'),
                Msg::Char('1'),
            ],
        );
        assert_eq!(form_of(&m).due.text(), "2026-10-31");
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(calendar_of(&m).1.day, day("2026-10-31"));
        assert_eq!(update(&mut m, Msg::Down), Vec::new());
        assert_eq!(calendar_of(&m).1.day, day("2026-11-07"));
        assert_eq!(update(&mut m, Msg::Escape), Vec::new());
        let form = form_of(&m);
        assert_eq!(form.due.text(), "2026-10-31");
        assert_eq!(form.focus, Field::Due);
        // A word the box takes opens on its day; one it refuses, on today.
        for text in ["tomorrow", "nonsense"] {
            for _ in 0..10 {
                update(&mut m, Msg::Backspace);
            }
            feed(&mut m, chars(text));
            update(&mut m, Msg::Enter);
            let expected = if text == "tomorrow" {
                day("2026-10-06")
            } else {
                today()
            };
            assert_eq!(calendar_of(&m).1.day, expected, "{text}");
            update(&mut m, Msg::Escape);
        }
    }

    #[test]
    fn calendar_after_a_refusal_and_the_keys_elsewhere() {
        let mut m = model().with_today(today());
        update(&mut m, Msg::Edit);
        feed(&mut m, [Msg::NextField, Msg::NextField, Msg::NextField]);
        feed(&mut m, chars("nonsense"));
        // A message is cleared by any key, as everywhere; a picked day
        // that replaces a refused one saves.
        update(&mut m, Msg::Save);
        assert!(m.message.as_ref().is_some_and(|msg| msg.is_error));
        assert_eq!(form_of(&m).focus, Field::Due);
        update(&mut m, Msg::Enter);
        assert_eq!(m.message, None);
        assert_eq!(calendar_of(&m).1.day, today());
        assert_eq!(
            form_of_calendar(&m).due.text(),
            "nonsense",
            "the box keeps the typed word until a day is picked"
        );
        feed(&mut m, [Msg::Enter, Msg::Save]);
        assert_eq!(m.mode, Mode::Normal);
        // Ctrl+C quits from the picker too.
        let mut m = model().with_today(today());
        update(&mut m, Msg::Edit);
        feed(
            &mut m,
            [Msg::NextField, Msg::NextField, Msg::NextField, Msg::Enter],
        );
        assert!(matches!(m.mode, Mode::Calendar { .. }));
        assert_eq!(update(&mut m, Msg::Quit), Vec::new());
        assert!(m.quit);
        // Enter on the other rows still moves on (the description: a newline).
        let mut m = model();
        update(&mut m, Msg::Edit);
        update(&mut m, Msg::Enter);
        assert_eq!(form_of(&m).focus, Field::Status);
        assert_eq!(update(&mut m, Msg::Today), Vec::new());
        assert_eq!(
            form_of(&m).focus,
            Field::Status,
            "Today means nothing in the view"
        );
    }

    #[test]
    fn host_actions_need_a_selection() {
        let mut m = model();
        assert_eq!(
            update(&mut m, Msg::Editor),
            vec![Cmd::Editor(TaskId::from(1))]
        );
        assert_eq!(
            update(&mut m, Msg::Launch),
            vec![Cmd::Launch(TaskId::from(1), LaunchTarget::Here)]
        );
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::Launch(TaskId::from(1), LaunchTarget::Here)]
        );
        for focus in [true, false] {
            assert_eq!(
                update(&mut m, Msg::LaunchDetached { focus }),
                vec![Cmd::Launch(
                    TaskId::from(1),
                    LaunchTarget::Detached { focus }
                )]
            );
            assert_eq!(m.mode, Mode::Normal);
        }
        assert_eq!(update(&mut m, Msg::Sync), vec![Cmd::Sync(Vec::new())]);
        update(&mut m, Msg::Loaded(Vec::new()));
        for msg in [
            Msg::Editor,
            Msg::Launch,
            Msg::LaunchDetached { focus: true },
            Msg::LaunchDetached { focus: false },
            Msg::BeginStatus,
            Msg::BeginPriority,
            Msg::BeginNote,
            Msg::BeginDone,
        ] {
            assert_eq!(update(&mut m, msg.clone()), Vec::new(), "{msg:?}");
            assert_eq!(m.mode, Mode::Normal, "{msg:?}");
            assert_eq!(
                m.message,
                Some(Message::error("no task selected")),
                "{msg:?}"
            );
        }
        assert_eq!(update(&mut m, Msg::Sync), vec![Cmd::Sync(Vec::new())]);
    }

    #[test]
    fn a_selection_that_vanished_before_the_pick_is_reported() {
        let mut m = model();
        update(&mut m, Msg::BeginStatus);
        update(&mut m, Msg::Loaded(vec![task(9, "Other", None)]));
        // The reload moved the selection to the only task.
        assert_eq!(
            update(&mut m, Msg::Enter),
            vec![Cmd::SetStatus(TaskId::from(9), Status::IN_PROGRESS)]
        );
        update(&mut m, Msg::BeginNote);
        update(&mut m, Msg::Char('n'));
        update(&mut m, Msg::Loaded(Vec::new()));
        assert_eq!(update(&mut m, Msg::Enter), Vec::new());
        assert_eq!(m.message, Some(Message::error("no task selected")));
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn a_stale_selected_id_counts_as_no_selection() {
        let mut m = model();
        m.selected = Some(Cursor::Task(TaskId::from(42)));
        assert_eq!(update(&mut m, Msg::Launch), Vec::new());
        assert_eq!(m.message, Some(Message::error("no task selected")));
    }

    fn entries(history: &History) -> Vec<&str> {
        history.entries().iter().map(String::as_str).collect()
    }

    #[test]
    fn filters_are_recorded_on_enter_and_recalled_onto_the_list() {
        let mut m = model();
        feed(&mut m, [Msg::BeginFilter, Msg::Char('s'), Msg::Char('e')]);
        update(&mut m, Msg::Enter);
        // Reopening holds the applied filter; it is the draft.
        feed(&mut m, [Msg::BeginFilter, Msg::KillToStart]);
        feed(&mut m, chars(" thi "));
        update(&mut m, Msg::Enter);
        // Escape clears the filter and records nothing.
        feed(&mut m, [Msg::BeginFilter, Msg::Char('x'), Msg::Escape]);
        assert_eq!(entries(&m.history.filters), ["se", "thi"]);
        assert_eq!(m.filter, "");

        feed(&mut m, [Msg::BeginFilter, Msg::Char('f'), Msg::Left]);
        assert_eq!(update(&mut m, Msg::Up), Vec::new());
        assert_eq!(prompt(&m), ("thi".into(), 3), "cursor at the end");
        assert_eq!(m.filter, "thi");
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(3))));
        update(&mut m, Msg::Up);
        assert_eq!(prompt(&m), ("se".into(), 2));
        assert_eq!(m.filter, "se");
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        update(&mut m, Msg::Up);
        assert_eq!(prompt(&m), ("se".into(), 2), "at the oldest");
        update(&mut m, Msg::Down);
        assert_eq!(prompt(&m), ("thi".into(), 3));
        update(&mut m, Msg::Down);
        assert_eq!(prompt(&m), ("f".into(), 1), "the draft, cursor at the end");
        assert_eq!(m.filter, "f");
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        update(&mut m, Msg::Down);
        assert_eq!(prompt(&m), ("f".into(), 1), "at the draft");
        update(&mut m, Msg::Enter);
        assert_eq!(entries(&m.history.filters), ["se", "thi", "f"]);
        assert_eq!(entries(&m.history.notes), Vec::<&str>::new());
        assert_eq!(entries(&m.history.titles), Vec::<&str>::new());

        // A prompt opens at its draft, wherever the last one stopped.
        feed(&mut m, [Msg::BeginFilter, Msg::Up, Msg::Up, Msg::Escape]);
        feed(&mut m, [Msg::BeginFilter, Msg::Up]);
        assert_eq!(prompt(&m), ("f".into(), 1));
    }

    #[test]
    fn notes_are_recorded_by_log_and_done_and_recalled_by_both() {
        let mut m = model();
        // A refused empty note, an empty done note and Escape record nothing.
        feed(&mut m, [Msg::BeginNote, Msg::Enter, Msg::Escape]);
        feed(&mut m, [Msg::BeginDone, Msg::Enter]);
        feed(&mut m, [Msg::BeginNote, Msg::Char('x'), Msg::Escape]);
        assert_eq!(entries(&m.history.notes), Vec::<&str>::new());

        update(&mut m, Msg::BeginNote);
        feed(&mut m, chars(" found it "));
        update(&mut m, Msg::Enter);
        update(&mut m, Msg::BeginDone);
        feed(&mut m, chars("merged"));
        update(&mut m, Msg::Enter);
        assert_eq!(entries(&m.history.notes), ["found it", "merged"]);

        feed(&mut m, [Msg::BeginDone, Msg::Up, Msg::Up]);
        assert_eq!(prompt(&m), ("found it".into(), 8));
        update(&mut m, Msg::Escape);
        feed(&mut m, [Msg::BeginNote, Msg::Char('d')]);
        update(&mut m, Msg::Up);
        assert_eq!(prompt(&m), ("merged".into(), 6), "opened at the draft");
        update(&mut m, Msg::Down);
        assert_eq!(
            m.mode,
            Mode::Note {
                input: Text::single("d"),
                target: NoteTarget::Log
            }
        );
        assert_eq!(entries(&m.history.filters), Vec::<&str>::new());
        assert_eq!(entries(&m.history.titles), Vec::<&str>::new());
    }

    #[test]
    fn titles_are_recorded_when_accepted_and_kept_apart_from_filters() {
        let mut m = model();
        feed(&mut m, [Msg::BeginFilter, Msg::Char('s'), Msg::Enter]);
        // A refused empty title and Escape record nothing.
        feed(&mut m, [Msg::BeginCreate, Msg::Enter, Msg::Escape]);
        assert_eq!(entries(&m.history.titles), Vec::<&str>::new());
        update(&mut m, Msg::BeginCreate);
        feed(&mut m, chars(" Call the bank "));
        update(&mut m, Msg::Enter);
        feed(&mut m, [Msg::BeginCreate, Msg::Char('W'), Msg::Enter]);
        assert_eq!(entries(&m.history.titles), ["Call the bank", "W"]);
        assert_eq!(entries(&m.history.filters), ["s"]);

        feed(&mut m, [Msg::BeginCreate, Msg::Up, Msg::Up, Msg::Escape]);
        feed(&mut m, [Msg::BeginCreate, Msg::Char('x'), Msg::Up]);
        assert_eq!(prompt(&m), ("W".into(), 1), "opened at the draft");
        update(&mut m, Msg::Up);
        assert_eq!(
            m.mode,
            Mode::Create {
                input: Text::single("Call the bank")
            }
        );
        update(&mut m, Msg::Up);
        assert_eq!(prompt(&m), ("Call the bank".into(), 13), "no filters");
        feed(&mut m, [Msg::Down, Msg::Down]);
        assert_eq!(prompt(&m), ("x".into(), 1));
    }

    #[test]
    fn pasted_text_is_one_line() {
        assert_eq!(one_line("a\nb\r\nc\n"), "a b c");
        assert_eq!(one_line("plain"), "plain");
        assert_eq!(one_line("\n"), "");
        assert_eq!(one_line("x\r"), "x");
    }
}
