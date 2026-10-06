//! Rendering: the model onto a ratatui [`Frame`]. No state of its own;
//! the snapshot tests under `tests/` draw models onto a `TestBackend`.
//!
//! Layout (T-803): the list alone until the detail is shown (`Right`,
//! `Tab`). At [`TWO_PANE_MIN_WIDTH`](crate::model::TWO_PANE_MIN_WIDTH)
//! columns or more the detail then sits right of the list; below that it
//! replaces the list. List rows wrap at the pane width, continuation
//! lines indented under the title. The last line is the status bar: the
//! input being typed, the last message, or the key hints.

use std::fmt::Write as _;
use std::ops::Range;

use chrono::{Datelike, NaiveDate};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Padding, Paragraph};
use tasq_core::clock::{format_date, format_timestamp};
use tasq_core::model::{Priority, Status, Task};
use tasq_core::theme::{self, group_label};

use crate::calendar::{self, Calendar, DAYS_PER_WEEK};
use crate::form::{Field, Form, Text};
use crate::keys::{Action, KeyMap};
use crate::model::{LayoutKind, Mode, Model, NoteTarget, Row, TWO_PANE_MIN_WIDTH};

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
        &[Action::ShowDetail, Action::HideDetail],
        "show / hide the selected task's detail",
    ),
    (&[Action::ToggleDetail], "switch between list and detail"),
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
    match &model.mode {
        Mode::Form(form) => {
            if let Some(position) = render_form(model, frame, main, form) {
                frame.set_cursor_position(position);
            }
            frame.render_widget(status_bar(model, bar.width), bar);
            return;
        }
        Mode::Calendar { form, calendar } => {
            // The view underneath keeps its look; the terminal cursor
            // stays hidden while the picker has the keys.
            render_form(model, frame, main, form);
            render_calendar(model, frame, main, *calendar);
            frame.render_widget(status_bar(model, bar.width), bar);
            return;
        }
        _ => {}
    }
    match model.layout() {
        LayoutKind::TwoPane if model.show_detail => {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(main);
            render_list(model, frame, left);
            render_detail(model, frame, right);
        }
        LayoutKind::OnePane if model.show_detail => render_detail(model, frame, main),
        LayoutKind::TwoPane | LayoutKind::OnePane => render_list(model, frame, main),
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
        Mode::Form(_) | Mode::Calendar { .. } => unreachable!("drawn above"),
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

/// One list row, in the CLI's shape (`  [id] #prio Title (due date)
/// chips`), wrapped to `width` columns: the title's words, the due date
/// and each chip are placed in turn, and a line that is full continues
/// on the next one, indented to where the title starts. A word wider
/// than the pane gets a line of its own (and is cut by the terminal).
pub fn task_lines<'a>(model: &Model, task: &'a Task, width: u16) -> Vec<Line<'a>> {
    let prefix = vec![
        Span::styled(format!("  [{:>2}] ", task.id.as_str()), dim()),
        priority_span(model, task.priority),
    ];
    let indent: usize = prefix.iter().map(Span::width).sum::<usize>() + 1;
    let mut atoms: Vec<Span<'a>> = task.title.split_whitespace().map(Span::raw).collect();
    if let Some(due) = task.due {
        atoms.push(Span::styled(format!("(due {})", format_date(due)), dim()));
    }
    for tag in &task.tags {
        atoms.push(Span::styled(format!(" {} ", tag.to_hash()), chip(model)));
    }
    let width = usize::from(width);
    let mut lines = Vec::new();
    let mut spans = prefix;
    let mut used = indent - 1;
    let mut atoms_on_line = 0;
    for atom in atoms {
        let atom_width = atom.width();
        if atoms_on_line > 0 && used + 1 + atom_width > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            spans.push(Span::raw(" ".repeat(indent)));
            used = indent;
            atoms_on_line = 0;
        }
        if atoms_on_line > 0 || lines.is_empty() {
            spans.push(Span::raw(" "));
            used += 1;
        }
        spans.push(atom);
        used += atom_width;
        atoms_on_line += 1;
    }
    lines.push(Line::from(spans));
    lines
}

/// The list, headers included, as styled lines wrapped to `width`
/// columns, plus the range of lines the selected task takes.
pub fn list_lines(model: &Model, width: u16) -> (Vec<Line<'_>>, Option<Range<usize>>) {
    let selected = model.selected.as_ref();
    let mut lines = Vec::new();
    let mut selected_lines = None;
    for row in model.rows() {
        match row {
            Row::Header(status) => lines.push(Line::styled(
                group_label(status.as_ref()),
                colored(model, model.theme.status_color(status.as_ref()))
                    .add_modifier(Modifier::BOLD),
            )),
            Row::Task(task) => {
                let rows = task_lines(model, task, width);
                if selected == Some(&task.id) {
                    selected_lines = Some(lines.len()..lines.len() + rows.len());
                    lines.extend(
                        rows.into_iter()
                            .map(|line| line.style(Style::new().add_modifier(Modifier::REVERSED))),
                    );
                } else {
                    lines.extend(rows);
                }
            }
        }
    }
    (lines, selected_lines)
}

/// The first line to show so that the `selected` lines fit in `height`
/// lines; when they do not fit, their first line is shown.
pub fn scroll_offset(selected: Option<Range<usize>>, height: usize) -> usize {
    match selected {
        Some(range) if height > 0 => range.end.saturating_sub(height).min(range.start),
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
    let (lines, selected) = list_lines(model, inner.width);
    let offset = scroll_offset(selected, inner.height as usize);
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
    if model.layout() == LayoutKind::OnePane
        && let Some(key) = model.keys.hint(Action::HideDetail)
    {
        block = block.title_bottom(Line::from(format!(" {key}: list ")).right_aligned());
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
        Mode::Form(_) if model.message.is_none() => key_bar(FORM_HINTS, width),
        Mode::Calendar { .. } if model.message.is_none() => key_bar(CALENDAR_HINTS, width),
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

/// The status-bar key hints while the edit view is open (its keys are
/// fixed): each key bold, what it does dim.
pub const FORM_HINTS: &[(&str, &str)] = &[
    ("Tab/S-Tab", "row"),
    ("\u{2191}\u{2193}\u{2190}\u{2192}", "move"),
    ("Enter", "next / newline"),
    ("C-s", "save"),
    ("Esc", "cancel"),
];

/// The status-bar key hints while the calendar picker is open.
pub const CALENDAR_HINTS: &[(&str, &str)] = &[
    ("\u{2191}\u{2193}\u{2190}\u{2192}", "day / week"),
    ("PgUp/PgDn", "month"),
    ("t", "today"),
    ("Enter", "pick"),
    ("Esc", "close"),
];

/// `hints` as one line: keys and labels when they fit in `width`
/// columns, the keys alone otherwise.
pub fn key_bar(hints: &[(&str, &str)], width: u16) -> Line<'static> {
    let full: usize = hints
        .iter()
        .map(|(key, what)| key.chars().count() + 1 + what.len())
        .sum::<usize>()
        + 2 * hints.len().saturating_sub(1);
    let labelled = usize::from(width) >= full;
    let mut spans = Vec::new();
    for (i, (key, what)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled((*key).to_owned(), bold()));
        if labelled {
            spans.push(Span::styled(format!(" {what}"), dim()));
        }
    }
    Line::from(spans)
}

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

/// The edit view, the whole main area. Wide terminals get boxed fields:
/// a header line, the title box, status and priority side by side (every
/// choice visible, the chosen one marked), due, project and tags on one
/// row, and the description box taking the rest. Narrow terminals get
/// the compact rows ([`render_form_compact`]). The focused box has a
/// coloured border, a box the save refused a red one, and the terminal
/// cursor sits in the focused text.
/// Draws the edit view and returns where the terminal cursor belongs
/// (none on a choice row).
fn render_form(model: &Model, frame: &mut Frame, area: Rect, form: &Form) -> Option<(u16, u16)> {
    if area.width < TWO_PANE_MIN_WIDTH {
        return render_form_compact(model, frame, area, form);
    }
    let [header, title, choices, details, description] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(3),
    ])
    .areas(area);
    frame.render_widget(form_header(model, form), header);
    let mut cursor = None;
    text_box(model, frame, title, form, Field::Title, &mut cursor);
    let [status, priority] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .spacing(1)
            .areas(choices);
    choice_box(model, frame, status, form, Field::Status);
    choice_box(model, frame, priority, form, Field::Priority);
    let [due, project, tags] = Layout::horizontal([
        Constraint::Length(16),
        Constraint::Fill(3),
        Constraint::Fill(2),
    ])
    .spacing(1)
    .areas(details);
    text_box(model, frame, due, form, Field::Due, &mut cursor);
    text_box(model, frame, project, form, Field::Project, &mut cursor);
    text_box(model, frame, tags, form, Field::Tags, &mut cursor);
    let block = field_block(model, form, Field::Description);
    let inner = block.inner(description);
    frame.render_widget(block, description);
    render_description(frame, inner, form, &mut cursor);
    cursor
}

/// `Edit [id]` and the task's title as it is on disk, so the user knows
/// what they are editing while they retype the title.
fn form_header(model: &Model, form: &Form) -> Line<'static> {
    let stored = model
        .tasks
        .iter()
        .find(|t| t.id == form.id)
        .map(|t| t.title.clone())
        .unwrap_or_default();
    Line::from(vec![
        Span::styled(format!(" Edit [{}]", form.id), bold()),
        Span::styled(format!("  {stored}"), dim()),
    ])
}

/// The box around a field: its label as the title, the border coloured
/// when the field has the focus (red when the last save refused it),
/// dim otherwise; one column of padding inside.
fn field_block(model: &Model, form: &Form, field: Field) -> Block<'static> {
    let focused = field == form.focus;
    let refused = focused && model.message.as_ref().is_some_and(|m| m.is_error);
    let border = if refused {
        colored(model, theme::Color::Red)
    } else if focused {
        colored(model, theme::Color::Cyan)
    } else {
        dim()
    };
    let title = if focused {
        border.add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    Block::bordered()
        .border_style(border)
        .title(Span::styled(format!(" {} ", field.label()), title))
        .padding(Padding::horizontal(1))
}

/// A single-line text field in its box; the text scrolls under the
/// cursor when it is longer than the box.
fn text_box(
    model: &Model,
    frame: &mut Frame,
    area: Rect,
    form: &Form,
    field: Field,
    cursor: &mut Option<(u16, u16)>,
) {
    let block = field_block(model, form, field);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(text) = form.text(field) else {
        return;
    };
    let width = usize::from(inner.width).max(1);
    let (_, col) = text.cursor();
    let start = window(col, width);
    let shown: String = text.lines()[0].chars().skip(start).take(width).collect();
    frame.render_widget(Paragraph::new(shown), inner);
    if field == form.focus {
        *cursor = Some((inner.x + narrow(col - start), inner.y));
    }
}

/// A choice field in its box: every option on one line, the chosen one
/// marked (reversed when the field has the focus, bold otherwise), the
/// others dim.
fn choice_box(model: &Model, frame: &mut Frame, area: Rect, form: &Form, field: Field) {
    let block = field_block(model, form, field);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chosen = form.value(field, &model.workflow);
    let options: Vec<String> = match field {
        Field::Status => Form::status_choices(&model.workflow)
            .iter()
            .map(|s| s.as_ref().map_or("none", Status::as_str).to_owned())
            .collect(),
        _ => Model::priority_choices()
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
    };
    let focused = field == form.focus;
    let mut spans = Vec::new();
    for (i, option) in options.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let style = if option != chosen {
            dim()
        } else if focused {
            bold().add_modifier(Modifier::REVERSED)
        } else {
            bold()
        };
        spans.push(Span::styled(format!(" {option} "), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

/// The description, wrapped to `area` and scrolled to keep the cursor in
/// view.
fn render_description(frame: &mut Frame, area: Rect, form: &Form, cursor: &mut Option<(u16, u16)>) {
    let width = usize::from(area.width).max(1);
    let lines = wrapped(form.description.lines(), width);
    let (vrow, vcol) = wrapped_cursor(&form.description, width);
    let offset = scroll_offset(Some(vrow..vrow + 1), usize::from(area.height));
    let shown: Vec<Line<'_>> = lines.into_iter().skip(offset).map(Line::raw).collect();
    frame.render_widget(Paragraph::new(shown), area);
    if form.focus == Field::Description {
        *cursor = Some((area.x + narrow(vcol), area.y + narrow(vrow - offset)));
    }
}

/// The edit view on a narrow terminal: label and value per row, the
/// description below a rule, no boxes.
fn render_form_compact(
    model: &Model,
    frame: &mut Frame,
    area: Rect,
    form: &Form,
) -> Option<(u16, u16)> {
    let block = Block::bordered().title(format!(" Edit [{}] ", form.id));
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let [rows_area, _gap, rule, body] = Layout::vertical([
        Constraint::Length(SINGLE_ROWS.len().try_into().unwrap_or(u16::MAX)),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    let value_width = usize::from(rows_area.width)
        .saturating_sub(LABEL_WIDTH)
        .max(1);
    let mut cursor = None;
    let rows: Vec<Line<'_>> = SINGLE_ROWS
        .iter()
        .enumerate()
        .map(|(i, &field)| {
            let focused = field == form.focus;
            let label = Span::styled(
                format!("{:<LABEL_WIDTH$}", field.label()),
                label_style(model, focused),
            );
            let value = match form.text(field) {
                Some(text) => {
                    let (_, col) = text.cursor();
                    let start = window(col, value_width);
                    let shown: String = text.lines()[0]
                        .chars()
                        .skip(start)
                        .take(value_width)
                        .collect();
                    if focused {
                        cursor = Some((
                            rows_area.x + narrow(LABEL_WIDTH + col - start),
                            rows_area.y + narrow(i),
                        ));
                    }
                    Span::raw(shown)
                }
                None if focused => Span::styled(
                    format!("\u{2039} {} \u{203a}", form.value(field, &model.workflow)),
                    bold(),
                ),
                None => Span::raw(form.value(field, &model.workflow)),
            };
            Line::from(vec![label, value])
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), rows_area);

    let focused = form.focus == Field::Description;
    let label = format!("{} ", Field::Description.label());
    let filler = "\u{2500}".repeat(usize::from(rule.width).saturating_sub(label.len()));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, label_style(model, focused)),
            Span::styled(filler, dim()),
        ])),
        rule,
    );
    render_description(frame, body, form, &mut cursor);
    cursor
}

/// The month of the calendar's day, centred over `area`: the month and
/// year above the weekday header (starting on the model's `week_start`),
/// the day under the cursor reversed, today bold, weekends dim, a cyan
/// border like the focused box.
fn render_calendar(model: &Model, frame: &mut Frame, area: Rect, calendar: Calendar) {
    let grid = calendar::month_grid(calendar.day, model.week_start);
    let mut lines = vec![
        Line::styled(grid.title, bold()).centered(),
        Line::styled(grid.header, dim()),
    ];
    for week in &grid.weeks {
        let mut spans = Vec::with_capacity(2 * DAYS_PER_WEEK);
        for cell in week {
            spans.push(Span::raw(" "));
            spans.push(match cell {
                Some(date) => Span::styled(
                    format!("{:2}", date.day()),
                    day_style(*date, calendar.day, model.today),
                ),
                None => Span::raw("  "),
            });
        }
        lines.push(Line::from(spans));
    }
    let block = Block::bordered()
        .border_style(colored(model, theme::Color::Cyan))
        .title(Span::styled(
            format!(" {} ", Field::Due.label()),
            colored(model, theme::Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1));
    let width = narrow(3 * DAYS_PER_WEEK + 4);
    let height = narrow(lines.len() + 2);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// How the picker draws a day: the cursor reversed and bold, today bold,
/// Saturdays and Sundays dim.
fn day_style(date: NaiveDate, cursor: NaiveDate, today: NaiveDate) -> Style {
    let mut style = Style::new();
    if calendar::is_weekend(date) {
        style = style.add_modifier(Modifier::DIM);
    }
    if date == today {
        style = style.add_modifier(Modifier::BOLD);
    }
    if date == cursor {
        style = style.add_modifier(Modifier::REVERSED);
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// The rows above the description, top to bottom.
const SINGLE_ROWS: [Field; 6] = [
    Field::Title,
    Field::Status,
    Field::Priority,
    Field::Due,
    Field::Project,
    Field::Tags,
];

/// Columns a row label takes, value column included.
const LABEL_WIDTH: usize = 11;

/// A row label: the focused one stands out, the others are plain.
fn label_style(model: &Model, focused: bool) -> Style {
    if focused {
        colored(model, theme::Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    }
}

/// The first character shown of a single-line value `width` columns wide
/// so that the cursor at `col` is visible: the text scrolls once the
/// cursor passes the right edge.
pub fn window(col: usize, width: usize) -> usize {
    col.saturating_sub(width.saturating_sub(1))
}

/// `lines` cut into pieces of at most `width` characters. A line gets one
/// piece more than it fills completely (so an empty line is one empty
/// piece and a line of exactly `width` characters leaves an empty piece
/// after it), which is where the cursor goes at the end of such a line.
pub fn wrapped(lines: &[String], width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        for piece in 0..=chars.len() / width {
            let start = piece * width;
            let end = (start + width).min(chars.len());
            out.push(chars[start..end].iter().collect());
        }
    }
    out
}

/// Where the cursor of `text` lands among [`wrapped`] lines of `width`.
pub fn wrapped_cursor(text: &Text, width: usize) -> (usize, usize) {
    let width = width.max(1);
    let (row, col) = text.cursor();
    let above: usize = text.lines()[..row]
        .iter()
        .map(|line| line.chars().count() / width + 1)
        .sum();
    (above + col / width, col % width)
}

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
        assert_eq!(scroll_offset(Some(3..4), 10), 0);
        assert_eq!(scroll_offset(Some(9..10), 10), 0);
        assert_eq!(scroll_offset(Some(10..11), 10), 1);
        assert_eq!(scroll_offset(Some(25..26), 10), 16);
        assert_eq!(scroll_offset(Some(5..6), 0), 0);
        // A wrapped row scrolls until its last line is in view...
        assert_eq!(scroll_offset(Some(8..12), 10), 2);
        // ...unless it is taller than the pane: then its first line is.
        assert_eq!(scroll_offset(Some(8..30), 10), 8);
    }

    #[test]
    fn rows_wrap_at_the_pane_width() {
        use tasq_core::model::{TaskId, Workflow};
        let model = Model::new(Workflow::default(), theme::Theme::default(), false);
        let task = Task::new(TaskId::from(7), "one two three");
        let text = |lines: Vec<Line<'_>>| -> Vec<String> {
            lines
                .iter()
                .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                .collect()
        };
        // 23 columns: the row fits exactly; one less wraps the last word.
        assert_eq!(
            text(task_lines(&model, &task, 23)),
            ["  [ 7] #B one two three"]
        );
        assert_eq!(
            text(task_lines(&model, &task, 22)),
            ["  [ 7] #B one two", "          three"]
        );
        // A word wider than the pane keeps a line of its own; width 0 never loops.
        assert_eq!(
            text(task_lines(&model, &task, 0)),
            ["  [ 7] #B one", "          two", "          three"]
        );
    }

    #[test]
    fn wrapping_and_the_cursor_within_it() {
        let lines = ["abcdefgh".to_owned(), String::new(), "xy".to_owned()];
        assert_eq!(wrapped(&lines, 3), ["abc", "def", "gh", "", "xy"]);
        assert_eq!(
            wrapped(&["abc".to_owned()], 3),
            ["abc", ""],
            "a full line leaves a piece for the cursor after it"
        );
        assert_eq!(
            wrapped(&lines, 0),
            wrapped(&lines, 1),
            "width is at least one"
        );
        assert_eq!(wrapped(&[], 5), Vec::<String>::new());
        let mut text = Text::multi("abcdefgh\n\nxy");
        assert_eq!(wrapped_cursor(&text, 3), (0, 0));
        text.end();
        assert_eq!(
            wrapped_cursor(&text, 3),
            (2, 2),
            "col 8 is piece 2, column 2"
        );
        text.down();
        text.down();
        text.right();
        assert_eq!(
            wrapped_cursor(&text, 3),
            (4, 1),
            "three pieces, one empty line, then col 1"
        );
        text.right();
        text.right();
        assert_eq!(wrapped_cursor(&text, 3), (4, 2));
        assert_eq!(wrapped_cursor(&text, 0), wrapped_cursor(&text, 1));
        assert_eq!(window(3, 25), 0);
        assert_eq!(window(24, 25), 0);
        assert_eq!(window(25, 25), 1);
        assert_eq!(window(29, 25), 5);
        assert_eq!(window(7, 0), 7);
    }

    #[test]
    fn key_bars_drop_their_labels_when_narrow() {
        let text = |hints, width| {
            key_bar(hints, width)
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
        };
        let full = "Tab/S-Tab row  \u{2191}\u{2193}\u{2190}\u{2192} move  Enter next / newline  C-s save  Esc cancel";
        assert_eq!(text(FORM_HINTS, 200), full);
        assert_eq!(full.chars().count(), 68);
        assert_eq!(text(FORM_HINTS, 68), full);
        assert_eq!(
            text(FORM_HINTS, 67),
            "Tab/S-Tab  \u{2191}\u{2193}\u{2190}\u{2192}  Enter  C-s  Esc"
        );
        let full = "\u{2191}\u{2193}\u{2190}\u{2192} day / week  PgUp/PgDn month  t today  Enter pick  Esc close";
        assert_eq!(text(CALENDAR_HINTS, 200), full);
        assert_eq!(full.chars().count(), 64);
        assert_eq!(text(CALENDAR_HINTS, 64), full);
        assert_eq!(
            text(CALENDAR_HINTS, 63),
            "\u{2191}\u{2193}\u{2190}\u{2192}  PgUp/PgDn  t  Enter  Esc"
        );
        assert_eq!(text(&[], 10), "");
        assert_eq!(text(&[("x", "y")], 3), "x y");
        assert_eq!(text(&[("x", "y")], 2), "x");
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
            rows[18],
            (
                "Right, Left".to_owned(),
                "show / hide the selected task's detail"
            )
        );
        assert_eq!(
            rows[20],
            ("Enter".to_owned(), "in a picker: apply the choice")
        );
        assert_eq!(rows[22], ("q, C-c".to_owned(), "quit"));
        let mut table = BTreeMap::new();
        table.insert("quit".to_owned(), KeySpec::Many(Vec::new()));
        table.insert("sync".to_owned(), KeySpec::One("f5".to_owned()));
        let rows = help_rows(&KeyMap::from_config(&table).unwrap());
        assert_eq!(rows[22], ("none, C-c".to_owned(), "quit"));
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
