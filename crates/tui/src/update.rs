//! `update(model, msg) -> cmds`: the whole behaviour of the UI, with no
//! I/O, so a test is a sequence of messages and a look at the model.

use tasq_core::model::{Priority, TaskId};

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
            if model.visible().iter().any(|t| t.id == id) {
                model.selected = Some(id);
            }
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
        Msg::Escape => {
            if model.show_detail {
                model.show_detail = false;
            } else if !model.filter.is_empty() {
                model.set_filter(String::new());
            }
        }
        Msg::ToggleDetail => model.show_detail = !model.show_detail,
        Msg::Help => model.mode = Mode::Help,
        Msg::Reload => return vec![Cmd::Load],
        Msg::BeginFilter => {
            model.mode = Mode::Filter {
                input: model.filter.clone(),
            };
        }
        Msg::BeginStatus => {
            if let Some(task) = model.selected_task() {
                let cursor = task
                    .status
                    .as_ref()
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
        Msg::BeginCreate => {
            model.mode = Mode::Create {
                input: String::new(),
            };
        }
        Msg::Edit => return with_selection(model, Cmd::Edit),
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
        Msg::Backspace | Msg::Char(_) | Msg::Paste(_) => {}
        Msg::Resize(..) | Msg::Loaded(_) | Msg::Select(_) | Msg::Info(_) | Msg::Failed(_) => {
            unreachable!("handled before the mode dispatch")
        }
    }
    Vec::new()
}

const NO_SELECTION: &str = "no task selected";
const NO_SOURCES: &str = "no [[source]] is configured (see docs/sources.md)";
const NO_SOURCE_CHECKED: &str = "no source checked (Space toggles, Esc closes)";

fn begin_note(model: &mut Model, target: NoteTarget) {
    if model.selected_task().is_some() {
        model.mode = Mode::Note {
            input: String::new(),
            target,
        };
    } else {
        model.message = Some(Message::error(NO_SELECTION));
    }
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

fn filter(model: &mut Model, mut input: String, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Enter => model.mode = Mode::Normal,
        Msg::Escape => {
            model.set_filter(String::new());
            model.mode = Mode::Normal;
        }
        Msg::Char(c) => retype(model, input, Some(c)),
        Msg::Paste(text) => {
            input.push_str(&one_line(&text));
            retype(model, input, None);
        }
        Msg::Backspace => {
            input.pop();
            retype(model, input, None);
        }
        _ => {}
    }
    Vec::new()
}

/// Applies the filter text as typed so the list follows every keystroke.
fn retype(model: &mut Model, mut input: String, c: Option<char>) {
    if let Some(c) = c {
        input.push(c);
    }
    model.set_filter(input.clone());
    model.mode = Mode::Filter { input };
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
    with_selection(model, |id| Cmd::SetStatus(id, status))
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

fn note(model: &mut Model, mut input: String, target: NoteTarget, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Escape => model.mode = Mode::Normal,
        Msg::Char(c) => {
            input.push(c);
            model.mode = Mode::Note { input, target };
        }
        Msg::Paste(text) => {
            input.push_str(&one_line(&text));
            model.mode = Mode::Note { input, target };
        }
        Msg::Backspace => {
            input.pop();
            model.mode = Mode::Note { input, target };
        }
        Msg::Enter => {
            let text = input.trim();
            match target {
                NoteTarget::Log if text.is_empty() => {
                    model.message = Some(Message::error("the note must not be empty"));
                }
                NoteTarget::Log => {
                    let note = text.to_owned();
                    model.mode = Mode::Normal;
                    return with_selection(model, |id| Cmd::Log(id, note));
                }
                NoteTarget::Done => {
                    let note = (!text.is_empty()).then(|| text.to_owned());
                    model.mode = Mode::Normal;
                    return with_selection(model, |id| Cmd::Done(id, note));
                }
            }
        }
        _ => {}
    }
    Vec::new()
}

/// The title of a new task (`c`): typed like a note, written with the
/// model's draft on Enter. An empty title is refused and the input stays
/// open, as `tasq create` refuses it.
fn create(model: &mut Model, mut input: String, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Quit => model.quit = true,
        Msg::Escape => model.mode = Mode::Normal,
        Msg::Char(c) => {
            input.push(c);
            model.mode = Mode::Create { input };
        }
        Msg::Paste(text) => {
            input.push_str(&one_line(&text));
            model.mode = Mode::Create { input };
        }
        Msg::Backspace => {
            input.pop();
            model.mode = Mode::Create { input };
        }
        Msg::Enter => {
            let title = input.trim();
            if title.is_empty() {
                model.message = Some(Message::error("the title must not be empty"));
            } else {
                let draft = model.draft(title);
                model.mode = Mode::Normal;
                return vec![Cmd::Create(Box::new(draft))];
            }
        }
        _ => {}
    }
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
    use crate::model::SourceChoice;
    use tasq_core::model::{Status, Task, TaskDraft, Workflow};
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
        assert_eq!(m.selected, Some(TaskId::from(1)));
        update(&mut m, Msg::Down);
        assert_eq!(m.selected, Some(TaskId::from(2)));
        update(&mut m, Msg::Up);
        assert_eq!(m.selected, Some(TaskId::from(1)));
        update(&mut m, Msg::Bottom);
        assert_eq!(m.selected, Some(TaskId::from(3)));
        update(&mut m, Msg::Top);
        assert_eq!(m.selected, Some(TaskId::from(1)));
        update(&mut m, Msg::PageDown);
        assert_eq!(m.selected, Some(TaskId::from(3)));
        update(&mut m, Msg::PageUp);
        assert_eq!(m.selected, Some(TaskId::from(1)));
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
            Some(TaskId::from(1)),
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
                input: String::new()
            }
        );
        feed(&mut m, chars("sec"));
        assert_eq!(m.filter, "sec");
        assert_eq!(m.selected, Some(TaskId::from(2)));
        assert_eq!(m.visible().len(), 1);
        update(&mut m, Msg::Backspace);
        assert_eq!(m.filter, "se");
        update(&mut m, Msg::Paste("cond\n".into()));
        assert_eq!(m.filter, "second");
        assert_eq!(
            m.mode,
            Mode::Filter {
                input: "second".into()
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
                input: "second".into()
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
                input: String::new(),
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
                input: "found the cause".into(),
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
                input: String::new()
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
                input: "  Call the bank".into()
            }
        );
        assert_eq!(update(&mut m, Msg::Down), Vec::new(), "ignored key");
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
            Some(TaskId::from(1)),
            "the reload keeps the first"
        );
        update(&mut m, Msg::Select(TaskId::from(7)));
        assert_eq!(m.selected, Some(TaskId::from(7)));
        assert_eq!(
            m.message,
            Some(Message::info("[7] created: New")),
            "selecting is a result, not a key: the message stays"
        );
        // A new task hidden by the filter is not selected.
        m.set_filter("First".into());
        update(&mut m, Msg::Select(TaskId::from(7)));
        assert_eq!(m.selected, Some(TaskId::from(1)));
        // An unknown id is ignored too.
        m.set_filter(String::new());
        update(&mut m, Msg::Select(TaskId::from(99)));
        assert_eq!(m.selected, Some(TaskId::from(1)));
    }

    #[test]
    fn host_actions_need_a_selection() {
        let mut m = model();
        assert_eq!(update(&mut m, Msg::Edit), vec![Cmd::Edit(TaskId::from(1))]);
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
            Msg::Edit,
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
        m.selected = Some(TaskId::from(42));
        assert_eq!(update(&mut m, Msg::Launch), Vec::new());
        assert_eq!(m.message, Some(Message::error("no task selected")));
    }

    #[test]
    fn pasted_text_is_one_line() {
        assert_eq!(one_line("a\nb\r\nc\n"), "a b c");
        assert_eq!(one_line("plain"), "plain");
        assert_eq!(one_line("\n"), "");
        assert_eq!(one_line("x\r"), "x");
    }
}
