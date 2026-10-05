//! Rendering: the model onto a ratatui [`Frame`]. No state of its own;
//! the snapshot tests under `tests/` draw models onto a `TestBackend`.
//!
//! Layout (T-803): at [`TWO_PANE_MIN_WIDTH`](crate::model::TWO_PANE_MIN_WIDTH)
//! columns or more the list sits left and the selected task's detail right;
//! below that one pane shows the list, or the detail after `Tab`. The last
//! line is the status bar: the input being typed, the last message, or
//! the key hints.

use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use tasq_core::clock::{format_date, format_timestamp};
use tasq_core::model::{Priority, Task};
use tasq_core::theme::{self, group_label};

use crate::form::{Field, Form};
use crate::keys::{Action, KeyMap};
use crate::model::{LayoutKind, Mode, Model, NoteTarget, Row};

/// How many progress notes the detail pane shows (the most recent ones).
pub const PROGRESS_SHOWN: usize = 8;

/// The help overlay, one `(actions, what they do)` per line; the keys
/// come from the model's [`KeyMap`] (see [`help_rows`]).
pub const HELP: &[(&[Action], &str)] = &[
    (&[Action::Up, Action::Down], "move the selection"),
    (&[Action::Top, Action::Bottom], "first / last task"),
    (&[Action::PageUp, Action::PageDown], "move ten tasks"),
    (
        &[Action::Filter],
        "filter: text matches titles, #word a status, tag or priority",
    ),
    (
        &[Action::Cancel],
        "clear the filter, close the detail or a dialog",
    ),
    (
        &[Action::Create],
        "create a task from a title (then t, p to refine)",
    ),
    (
        &[Action::Status],
        "set the status (workflow statuses, pick by number)",
    ),
    (&[Action::Priority], "set the priority (A, B, C)"),
    (&[Action::Log], "log a progress note"),
    (&[Action::Done], "mark done, with an optional final note"),
    (
        &[Action::Edit],
        "edit the task in a form (title, status, priority, due, project, tags)",
    ),
    (&[Action::Editor], "open the task file in $EDITOR"),
    (&[Action::Launch], "open a work session here (tasq pick)"),
    (
        &[Action::LaunchDetached],
        "open it in a new window (tasq pick --detached)",
    ),
    (
        &[Action::LaunchDetachedStay],
        "open it in a new window without switching to it",
    ),
    (
        &[Action::Sync],
        "run the sources that run by default (tasq sync)",
    ),
    (
        &[Action::Sources],
        "pick the sources to run: Space toggles, Enter runs",
    ),
    (&[Action::Reload], "reload"),
    (
        &[Action::ToggleDetail],
        "narrow terminals: switch between list and detail",
    ),
    (&[Action::Confirm], "in a picker: apply the choice"),
    (&[Action::Help], "this help"),
    (&[Action::Quit], "quit"),
];

/// The help overlay's rows for `keys`: the keys of each action of a
/// [`HELP`] row joined by `, ` (`k/Up, j/Down`), and `Ctrl+C` next to
/// `quit` since it always quits.
pub fn help_rows(keys: &KeyMap) -> Vec<(String, &'static str)> {
    HELP.iter()
        .map(|(actions, what)| {
            let mut shown = actions
                .iter()
                .map(|a| keys.display(*a))
                .collect::<Vec<_>>()
                .join(", ");
            if *actions == [Action::Quit] {
                shown.push_str(", C-c");
            }
            (shown, *what)
        })
        .collect()
}

/// The key hints of the status bar in normal mode, for `keys`: the first
/// key of each action, unbound actions left out. Three lengths, picked by
/// [`hints`].
fn hint_lines(keys: &KeyMap) -> [String; 3] {
    let k = |action| keys.hint(action);
    let moves = match (k(Action::Down), k(Action::Up)) {
        (Some(down), Some(up)) => Some(format!("{down}/{up}")),
        (down, up) => down.or(up),
    };
    let labelled = |action, label: &str| k(action).map(|key| format!("{key} {label}"));
    let sync = match (k(Action::Sync), k(Action::Sources)) {
        (Some(all), Some(pick)) => Some(format!("{all}/{pick} sync")),
        (Some(key), None) | (None, Some(key)) => Some(format!("{key} sync")),
        (None, None) => None,
    };
    let tail = [
        labelled(Action::Launch, "open"),
        sync,
        labelled(Action::Help, "help"),
        labelled(Action::Quit, "quit"),
    ];
    let full = [
        moves.clone(),
        labelled(Action::Filter, "filter"),
        labelled(Action::Create, "new"),
        labelled(Action::Status, "status"),
        labelled(Action::Priority, "prio"),
        labelled(Action::Log, "log"),
        labelled(Action::Done, "done"),
        labelled(Action::Edit, "edit"),
    ]
    .into_iter()
    .chain(tail.iter().cloned());
    let letters = [
        Action::Create,
        Action::Status,
        Action::Priority,
        Action::Log,
        Action::Done,
        Action::Edit,
    ]
    .into_iter()
    .filter_map(k)
    .collect::<Vec<_>>()
    .join(" ");
    let short = [
        moves,
        k(Action::Filter),
        (!letters.is_empty()).then_some(letters),
    ]
    .into_iter()
    .chain(tail.iter().cloned());
    let tiny = tail[2..].iter().cloned();
    [join(full), join(short), join(tiny)]
}

/// Joins the present hints with two spaces.
fn join(parts: impl Iterator<Item = Option<String>>) -> String {
    parts.flatten().collect::<Vec<_>>().join("  ")
}

/// Draws `model` onto `frame`.
pub fn view(model: &Model, frame: &mut Frame) {
    let [main, bar] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    match model.layout() {
        LayoutKind::TwoPane => {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(main);
            render_list(model, frame, left);
            render_detail(model, frame, right);
        }
        LayoutKind::OnePane if model.show_detail => render_detail(model, frame, main),
        LayoutKind::OnePane => render_list(model, frame, main),
    }
    frame.render_widget(status_bar(model, bar.width), bar);
    match &model.mode {
        Mode::Help => render_help(model, frame, main),
        Mode::Status { cursor } => render_picker(
            model,
            frame,
            main,
            "Status",
            &model
                .status_choices()
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{} {}", i + 1, s.as_str()))
                .collect::<Vec<_>>(),
            *cursor,
        ),
        Mode::Priority { cursor } => render_picker(
            model,
            frame,
            main,
            "Priority",
            &Model::priority_choices()
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect::<Vec<_>>(),
            *cursor,
        ),
        Mode::Sources { cursor } => render_picker(
            model,
            frame,
            main,
            "Sync sources: Space toggles, Enter runs",
            &source_entries(model),
            *cursor,
        ),
        Mode::Form(form) => render_form(model, frame, main, form),
        Mode::Normal | Mode::Filter { .. } | Mode::Note { .. } | Mode::Create { .. } => {}
    }
}

/// The rows of the source picker: `[x] 1 name  kind`, names padded to
/// the longest so the kinds line up.
pub fn source_entries(model: &Model) -> Vec<String> {
    let width = model
        .sources
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(0);
    model
        .sources
        .iter()
        .zip(&model.checked)
        .enumerate()
        .map(|(i, (source, checked))| {
            let mark = if *checked { 'x' } else { ' ' };
            format!(
                "[{mark}] {} {:<width$}  {}",
                i + 1,
                source.name,
                source.kind
            )
        })
        .collect()
}

/// The style for a theme colour: plain when colours are off (only `Dim`
/// keeps its modifier).
pub fn colored(model: &Model, color: theme::Color) -> Style {
    let style = Style::new();
    match color {
        theme::Color::Dim => style.add_modifier(Modifier::DIM),
        _ if !model.color => style,
        theme::Color::Red => style.fg(Color::Red),
        theme::Color::Green => style.fg(Color::Green),
        theme::Color::Yellow => style.fg(Color::Yellow),
        theme::Color::Blue => style.fg(Color::Blue),
        theme::Color::Magenta => style.fg(Color::Magenta),
        theme::Color::Cyan => style.fg(Color::Cyan),
        theme::Color::White => style.fg(Color::White),
        theme::Color::Fixed(n) => style.fg(Color::Indexed(n)),
    }
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// The tag chip: white on dark blue like the CLI, or plain text.
fn chip(model: &Model) -> Style {
    if model.color {
        Style::new().bg(Color::Indexed(24)).fg(Color::Indexed(231))
    } else {
        Style::new()
    }
}

fn priority_span(model: &Model, priority: Priority) -> Span<'static> {
    match priority {
        Priority::A => Span::styled(
            "#A",
            colored(model, theme::Color::Red).add_modifier(Modifier::BOLD),
        ),
        Priority::B => Span::styled("#B", dim()),
        Priority::C => Span::styled("#C", dim()),
    }
}

/// One list row, in the CLI's shape: `  [id] #prio Title (due date) chips`.
pub fn task_line<'a>(model: &Model, task: &'a Task) -> Line<'a> {
    let mut spans = vec![
        Span::styled(format!("  [{:>2}] ", task.id.as_str()), dim()),
        priority_span(model, task.priority),
        Span::raw(" "),
        Span::raw(task.title.as_str()),
    ];
    if let Some(due) = task.due {
        spans.push(Span::styled(format!(" (due {})", format_date(due)), dim()));
    }
    for tag in &task.tags {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!(" {} ", tag.to_hash()), chip(model)));
    }
    Line::from(spans)
}

/// The list, headers included, as styled lines.
pub fn list_lines(model: &Model) -> Vec<Line<'_>> {
    let selected = model.selected.as_ref();
    model
        .rows()
        .into_iter()
        .map(|row| match row {
            Row::Header(status) => Line::styled(
                group_label(status.as_ref()),
                colored(model, model.theme.status_color(status.as_ref()))
                    .add_modifier(Modifier::BOLD),
            ),
            Row::Task(task) => {
                let line = task_line(model, task);
                if selected == Some(&task.id) {
                    line.style(Style::new().add_modifier(Modifier::REVERSED))
                } else {
                    line
                }
            }
        })
        .collect()
}

/// The first row to show so that the selected row fits in `height` rows.
pub fn scroll_offset(selected_row: Option<usize>, height: usize) -> usize {
    match selected_row {
        Some(row) if height > 0 && row >= height => row + 1 - height,
        _ => 0,
    }
}

fn render_list(model: &Model, frame: &mut Frame, area: Rect) {
    let shown = model.visible().len();
    let mut title = format!(" Tasks ({shown}/{}) ", model.tasks.len());
    if !model.filter.is_empty() {
        let _ = write!(title, "/{} ", model.filter);
    }
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if model.tasks.is_empty() {
        frame.render_widget(Paragraph::new("No open todos."), inner);
        return;
    }
    if shown == 0 {
        frame.render_widget(
            Paragraph::new(format!("No open todos match /{}.", model.filter)),
            inner,
        );
        return;
    }
    let lines = list_lines(model);
    let offset = scroll_offset(model.selected_row(), inner.height as usize);
    let visible: Vec<Line<'_>> = lines.into_iter().skip(offset).collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

/// The detail pane lines for `task`.
pub fn detail_lines<'a>(model: &Model, task: &'a Task) -> Vec<Line<'a>> {
    let mut lines = vec![Line::styled(task.title.as_str(), bold())];
    let mut head = vec![
        Span::styled(format!("[{}]", task.id.as_str()), dim()),
        Span::raw("  "),
        priority_span(model, task.priority),
    ];
    match &task.status {
        Some(status) => {
            head.push(Span::raw("  "));
            head.push(Span::styled(
                status.as_str(),
                colored(model, model.theme.status_color(Some(status))),
            ));
        }
        None if task.done => {
            head.push(Span::raw("  "));
            head.push(Span::styled("done", dim()));
        }
        None => {}
    }
    if let Some(due) = task.due {
        head.push(Span::raw("  "));
        head.push(Span::styled(format!("due {}", format_date(due)), dim()));
    }
    lines.push(Line::from(head));
    if !task.tags.is_empty() {
        let mut spans = Vec::new();
        for tag in &task.tags {
            spans.push(Span::styled(format!(" {} ", tag.to_hash()), chip(model)));
            spans.push(Span::raw(" "));
        }
        lines.push(Line::from(spans));
    }
    if let Some(project) = &task.project {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("Project: ", bold()),
            Span::raw(project.display().to_string()),
        ]));
    }
    if let Some(description) = &task.description {
        lines.push(Line::default());
        lines.push(Line::styled("Description", bold()));
        lines.extend(description.lines().map(Line::raw));
    }
    for (heading, items) in link_sections(task) {
        if !items.is_empty() {
            lines.push(Line::default());
            lines.push(Line::styled(heading, bold()));
            lines.extend(items.into_iter().map(|item| Line::raw(format!("- {item}"))));
        }
    }
    if !task.progress.is_empty() {
        lines.push(Line::default());
        let skipped = task.progress.len().saturating_sub(PROGRESS_SHOWN);
        let heading = if skipped > 0 {
            format!(
                "Progress (last {PROGRESS_SHOWN} of {})",
                task.progress.len()
            )
        } else {
            "Progress".to_owned()
        };
        lines.push(Line::styled(heading, bold()));
        for entry in task.progress.iter().skip(skipped) {
            lines.push(Line::from(vec![
                Span::styled(format!("- {}: ", entry.at), dim()),
                Span::raw(entry.note.as_str()),
            ]));
        }
    }
    lines
}

/// The list sections of the detail pane: related links, merge requests,
/// worktrees and sessions, each as `(heading, entries)`.
pub fn link_sections(task: &Task) -> Vec<(&'static str, Vec<String>)> {
    let as_text = |l: &tasq_core::model::Link| match &l.label {
        Some(label) => format!("{label} <{}>", l.url),
        None => l.url.clone(),
    };
    vec![
        ("Related", task.related.iter().map(as_text).collect()),
        (
            "Merge requests",
            task.merge_requests.iter().map(as_text).collect(),
        ),
        (
            "Worktrees",
            task.worktrees
                .iter()
                .map(|w| match &w.branch {
                    Some(branch) => format!("{} ({branch})", w.path.display()),
                    None => w.path.display().to_string(),
                })
                .collect(),
        ),
        (
            "Sessions",
            task.sessions
                .iter()
                .map(|s| {
                    let mut text = format!("{} {}", format_timestamp(s.at), s.id);
                    if let Some(description) = &s.description {
                        text.push_str(" \u{2014} ");
                        text.push_str(description);
                    }
                    text
                })
                .collect(),
        ),
    ]
}

fn render_detail(model: &Model, frame: &mut Frame, area: Rect) {
    let (title, lines) = match model.selected_task() {
        Some(task) => (
            format!(" [{}] ", task.id.as_str()),
            detail_lines(model, task),
        ),
        None => (
            " Task ".to_owned(),
            vec![Line::styled("No task selected.", dim())],
        ),
    };
    let mut block = Block::bordered().title(title);
    if model.layout() == LayoutKind::OnePane {
        block = block.title_bottom(Line::from(" Tab: list ").right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// The bottom line: the input being typed, else the last message, else
/// the key hints.
pub fn status_bar(model: &Model, width: u16) -> Paragraph<'_> {
    let line = match &model.mode {
        Mode::Filter { input } => Line::from(vec![
            Span::styled("/", bold()),
            Span::raw(input.as_str()),
            Span::styled("\u{2581}", dim()),
        ]),
        Mode::Note { input, target } => {
            let prompt = match target {
                NoteTarget::Log => "log: ",
                NoteTarget::Done => "done, final note (Enter alone just closes): ",
            };
            Line::from(vec![
                Span::styled(prompt, bold()),
                Span::raw(input.as_str()),
                Span::styled("\u{2581}", dim()),
            ])
        }
        Mode::Create { input } => Line::from(vec![
            Span::styled(
                format!("new task ({}): ", model.default_status.as_str()),
                bold(),
            ),
            Span::raw(input.as_str()),
            Span::styled("\u{2581}", dim()),
        ]),
        Mode::Form(_) if model.message.is_none() => Line::styled(FORM_HINTS, dim()),
        _ => match &model.message {
            Some(message) if message.is_error => Line::styled(
                message.text.as_str(),
                colored(model, theme::Color::Red).add_modifier(Modifier::BOLD),
            ),
            Some(message) => Line::raw(message.text.as_str()),
            None => Line::styled(hints(&model.keys, width), dim()),
        },
    };
    Paragraph::new(line)
}

/// The status-bar hints while the form is open (its keys are fixed).
pub const FORM_HINTS: &str = "Up/Down, Tab: next row  Left/Right: change  Enter: save  Esc: cancel";

/// The longest hint line for `keys` that fits in `width` columns.
pub fn hints(keys: &KeyMap, width: u16) -> String {
    let width = usize::from(width);
    let [full, short, tiny] = hint_lines(keys);
    if width >= full.len() {
        full
    } else if width >= short.len() {
        short
    } else {
        tiny
    }
}

/// A rectangle of at most `width` x `height` centred in `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// A length as a terminal dimension; anything larger than a terminal can
/// be is clamped.
fn narrow(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

fn render_help(model: &Model, frame: &mut Frame, area: Rect) {
    let rows = help_rows(&model.keys);
    let key_width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let lines: Vec<Line<'_>> = rows
        .iter()
        .map(|(keys, action)| {
            Line::from(vec![
                Span::styled(format!("{keys:<key_width$}  "), bold()),
                Span::raw(*action),
            ])
        })
        .collect();
    let width = narrow(lines.iter().map(Line::width).max().unwrap_or(0) + 4);
    let height = narrow(lines.len() + 2);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Keys ")
        .title_bottom(Line::from(" any key closes ").right_aligned())
        .style(colored(model, theme::Color::Dim).remove_modifier(Modifier::DIM));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The edit form: one row per field, the focused row's label reversed; a
/// text row shows the cursor after its text, a choice row shows its value
/// between `<` and `>`.
fn render_form(model: &Model, frame: &mut Frame, area: Rect, form: &Form) {
    let lines: Vec<Line<'_>> = Field::ALL
        .into_iter()
        .map(|field| {
            let label = format!(" {:<9}", field.label());
            let value = form.value(field, &model.workflow);
            let focused = field == form.focus;
            let label_style = if focused {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                bold()
            };
            let mut spans = vec![Span::styled(label, label_style), Span::raw(" ")];
            match (field.is_text(), focused) {
                (true, true) => {
                    spans.push(Span::raw(value));
                    spans.push(Span::styled("\u{2581}", dim()));
                }
                (false, true) => spans.push(Span::raw(format!("< {value} >"))),
                (_, false) => spans.push(Span::raw(value)),
            }
            Line::from(spans)
        })
        .collect();
    let width = narrow(
        lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(FORM_MIN_WIDTH)
            + 3,
    );
    let height = narrow(lines.len() + 2);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(format!(" Edit [{}] ", form.id));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The form is at least this wide inside its border, so short values
/// leave room to type (and the title always fits).
const FORM_MIN_WIDTH: usize = 50;

fn render_picker(
    model: &Model,
    frame: &mut Frame,
    area: Rect,
    title: &str,
    entries: &[String],
    cursor: usize,
) {
    let lines: Vec<Line<'_>> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let text = format!(" {entry} ");
            if i == cursor {
                Line::styled(text, Style::new().add_modifier(Modifier::REVERSED))
            } else {
                Line::raw(text)
            }
        })
        .collect();
    let width = narrow(
        lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(title.len() + 2)
            + 2,
    );
    let height = narrow(lines.len() + 2);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(format!(" {title} "));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let _ = model;
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tasq_core::config::KeySpec;

    use super::*;

    #[test]
    fn scroll_keeps_the_selection_in_view() {
        assert_eq!(scroll_offset(None, 10), 0);
        assert_eq!(scroll_offset(Some(3), 10), 0);
        assert_eq!(scroll_offset(Some(9), 10), 0);
        assert_eq!(scroll_offset(Some(10), 10), 1);
        assert_eq!(scroll_offset(Some(25), 10), 16);
        assert_eq!(scroll_offset(Some(5), 0), 0);
    }

    #[test]
    fn hints_shrink_with_the_terminal() {
        let keys = KeyMap::default();
        let full = "j/k  / filter  c new  t status  p prio  l log  d done  e edit  Enter open  s/S sync  ? help  q quit";
        let short = "j/k  /  c t p l d e  Enter open  s/S sync  ? help  q quit";
        assert_eq!(full.len(), 99, "the full hints must fit 100 columns");
        assert_eq!(short.len(), 57);
        assert_eq!(hints(&keys, 200), full);
        assert_eq!(hints(&keys, 99), full);
        assert_eq!(hints(&keys, 98), short);
        assert_eq!(hints(&keys, 57), short);
        assert_eq!(hints(&keys, 56), "? help  q quit");
        assert_eq!(hints(&keys, 0), "? help  q quit");
    }

    #[test]
    fn hints_follow_the_map() {
        let table = |entries: &[(&str, &[&str])]| {
            entries
                .iter()
                .map(|(k, v)| {
                    (
                        (*k).to_owned(),
                        KeySpec::Many(v.iter().map(|s| (*s).to_owned()).collect()),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        let keys = KeyMap::from_config(&table(&[
            ("launch", &["ctrl+o"]),
            ("down", &["n"]),
            ("sync", &[]),
            ("status", &[]),
            ("filter", &["f3"]),
        ]))
        .unwrap();
        assert_eq!(
            hints(&keys, 200),
            "n/k  F3 filter  c new  p prio  l log  d done  e edit  C-o open  S sync  ? help  q quit",
            "with `sync` unbound the picker's key alone labels sync"
        );
        assert_eq!(
            hints(&keys, 60),
            "n/k  F3  c p l d e  C-o open  S sync  ? help  q quit"
        );
        let keys = KeyMap::from_config(&table(&[
            ("down", &[]),
            ("up", &["k"]),
            ("quit", &[]),
            ("filter", &[]),
            ("create", &[]),
            ("status", &[]),
            ("priority", &[]),
            ("log", &[]),
            ("done", &[]),
            ("edit", &[]),
        ]))
        .unwrap();
        assert_eq!(hints(&keys, 200), "k  Enter open  s/S sync  ? help");
        assert_eq!(hints(&keys, 31), "k  Enter open  s/S sync  ? help");
        assert_eq!(hints(&keys, 30), "? help", "full and short coincide");
        let keys = KeyMap::from_config(&table(&[("down", &[]), ("up", &[])])).unwrap();
        assert_eq!(
            hints(&keys, 200),
            "/ filter  c new  t status  p prio  l log  d done  e edit  Enter open  s/S sync  ? help  q quit"
        );
        let keys = KeyMap::from_config(&table(&[("sources", &[])])).unwrap();
        assert_eq!(
            hints(&keys, 200),
            "j/k  / filter  c new  t status  p prio  l log  d done  e edit  Enter open  s sync  ? help  q quit"
        );
        let keys = KeyMap::from_config(&table(&[("sources", &[]), ("sync", &[])])).unwrap();
        assert_eq!(
            hints(&keys, 200),
            "j/k  / filter  c new  t status  p prio  l log  d done  e edit  Enter open  ? help  q quit"
        );
    }

    #[test]
    fn help_rows_follow_the_map() {
        let rows = help_rows(&KeyMap::default());
        assert_eq!(rows.len(), HELP.len());
        assert_eq!(rows[0], ("k/Up, j/Down".to_owned(), "move the selection"));
        assert_eq!(rows[1], ("g/Home, G/End".to_owned(), "first / last task"));
        assert_eq!(rows[2], ("C-u/PgUp, C-d/PgDn".to_owned(), "move ten tasks"));
        assert_eq!(
            rows[10],
            (
                "e".to_owned(),
                "edit the task in a form (title, status, priority, due, project, tags)"
            )
        );
        assert_eq!(rows[11], ("E".to_owned(), "open the task file in $EDITOR"));
        assert_eq!(rows[13].0, "C-Enter");
        assert_eq!(rows[14].0, "S-Enter");
        assert_eq!(
            rows[15],
            (
                "s".to_owned(),
                "run the sources that run by default (tasq sync)"
            )
        );
        assert_eq!(
            rows[16],
            (
                "S".to_owned(),
                "pick the sources to run: Space toggles, Enter runs"
            )
        );
        assert_eq!(
            rows[19],
            ("Enter".to_owned(), "in a picker: apply the choice")
        );
        assert_eq!(rows[21], ("q, C-c".to_owned(), "quit"));
        let mut table = BTreeMap::new();
        table.insert("quit".to_owned(), KeySpec::Many(Vec::new()));
        table.insert("sync".to_owned(), KeySpec::One("f5".to_owned()));
        let rows = help_rows(&KeyMap::from_config(&table).unwrap());
        assert_eq!(rows[21], ("none, C-c".to_owned(), "quit"));
        assert_eq!(rows[15].0, "F5");
    }

    #[test]
    fn centred_rectangles_are_clamped() {
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(centered(area, 40, 10), Rect::new(30, 10, 40, 10));
        assert_eq!(centered(area, 200, 60), area);
        let offset = Rect::new(5, 5, 20, 20);
        assert_eq!(centered(offset, 10, 10), Rect::new(10, 10, 10, 10));
    }
}
