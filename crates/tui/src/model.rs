//! The application state: what is on screen and what the keys mean right
//! now. Pure data with pure helpers; [`crate::update()`] mutates it and
//! [`crate::view()`] reads it.

use std::path::PathBuf;

use chrono::{NaiveDate, Weekday};
use tasq_core::config::DueFormat;
use tasq_core::model::{Priority, Status, Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::{self, Filter, Group};
use tasq_core::theme::Theme;

use crate::calendar::Calendar;
use crate::form::{Form, Text};
use crate::history::Histories;
use crate::keys::KeyMap;

/// Terminal width from which the list and the detail pane sit side by
/// side (T-803: single pane below 100 columns).
pub const TWO_PANE_MIN_WIDTH: u16 = 100;

/// How many rows `PageUp`/`PageDown` move.
pub const PAGE: usize = 10;

/// What the keys do right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the list.
    Normal,
    /// Typing a filter (`/`); the list follows every keystroke.
    Filter {
        /// Text typed so far, with its cursor.
        input: Text,
    },
    /// Choosing a status for the selected task (`t`).
    Status {
        /// Highlighted entry of the workflow's status list.
        cursor: usize,
    },
    /// Choosing a priority for the selected task (`p`).
    Priority {
        /// Highlighted entry of `A`, `B`, `C`.
        cursor: usize,
    },
    /// Choosing the sources to run (`S`); which ones are checked lives in
    /// [`Model::checked`], so it survives closing the picker.
    Sources {
        /// Highlighted entry of [`Model::sources`].
        cursor: usize,
    },
    /// Typing a progress note (`l`) or the final note of `done` (`d`).
    Note {
        /// Text typed so far, with its cursor.
        input: Text,
        /// What the note is for.
        target: NoteTarget,
    },
    /// Typing the title of a new task (`c`).
    Create {
        /// Text typed so far, with its cursor.
        input: Text,
    },
    /// Editing the selected task's fields in the form (`e`).
    Form(Box<Form>),
    /// The calendar picker over the edit view (`Enter` on the Due box).
    Calendar {
        /// The edit view underneath, as it was when the picker opened.
        form: Box<Form>,
        /// The picker: the day under the cursor.
        calendar: Calendar,
    },
    /// The key help overlay (`?`).
    Help,
}

/// A source the picker (`S`) can run: an enabled `[[source]]` of the
/// configuration, handed over by the front end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChoice {
    /// `[[source]] name`, what `tasq sync --source` takes.
    pub name: String,
    /// `[[source]] kind`, shown next to the name.
    pub kind: String,
    /// `[[source]] auto`: whether a bare sync runs it. The picker starts
    /// with these checked.
    pub auto: bool,
}

/// What a typed note is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTarget {
    /// `tasq log <id> <note>`.
    Log,
    /// `tasq done <id> [note]`; an empty note just closes the task.
    Done,
}

/// The line shown in the status bar after an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The text.
    pub text: String,
    /// Whether it reports a failure (rendered in red).
    pub is_error: bool,
}

impl Message {
    /// An informational message.
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    /// An error message.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// How the screen is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// Wide enough for the detail beside the list.
    TwoPane,
    /// One pane: the list, or the detail of the selected task.
    OnePane,
}

/// One line of the task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row<'a> {
    /// A status group header (`None` is the no-status group) and the
    /// number of tasks in the group.
    Header(Option<Status>, usize),
    /// A task of the group above.
    Task(&'a Task),
}

/// The whole state of the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Open tasks as last loaded from the store.
    pub tasks: Vec<Task>,
    /// The configured statuses, for grouping and the status picker.
    pub workflow: Workflow,
    /// The status a task created with `c` starts in (`workflow.default_status`).
    pub default_status: Status,
    /// The project a task created with `c` tracks (the CLI passes the
    /// directory `tasq ui` runs in); `None` leaves the draft without one.
    pub default_project: Option<PathBuf>,
    /// Group colours (`[ui.colors]`).
    pub theme: Theme,
    /// Whether colours are used at all (`false` under `NO_COLOR`).
    pub color: bool,
    /// What the keys are (`[ui.keys]` over the defaults).
    pub keys: KeyMap,
    /// The enabled sources, for the source picker; empty when the front
    /// end passed none.
    pub sources: Vec<SourceChoice>,
    /// Which of [`Model::sources`] are checked, kept across openings of
    /// the picker; starts as each source's `auto`.
    pub checked: Vec<bool>,
    /// The day a due date typed as `today` in the form resolves to; the
    /// front end sets it from its clock.
    pub today: NaiveDate,
    /// The first column of the calendar picker (`ui.week_start`).
    pub week_start: Weekday,
    /// How list rows show a due date (`ui.due_format`).
    pub due_format: DueFormat,
    /// The applied filter text (see [`Model::filter`]).
    pub filter: String,
    /// The selected task, when any is visible.
    pub selected: Option<TaskId>,
    /// What the keys do.
    pub mode: Mode,
    /// What the prompts submitted this session, for `Up`/`Down`.
    pub history: Histories,
    /// Result of the last action.
    pub message: Option<Message>,
    /// Terminal width.
    pub width: u16,
    /// Terminal height.
    pub height: u16,
    /// Show the selected task's detail: beside the list in the two-pane
    /// layout, instead of it in the one-pane layout. Off until `Right`
    /// (or `Tab`); `Left` and `Esc` turn it off again.
    pub show_detail: bool,
    /// Set by `q`; the runtime stops.
    pub quit: bool,
}

impl Model {
    /// An empty model (no tasks loaded yet) for a terminal of unknown size.
    /// New tasks start as `ready`, the script's default, until
    /// [`Model::with_default_status`] says otherwise.
    pub fn new(workflow: Workflow, theme: Theme, color: bool) -> Self {
        Self {
            tasks: Vec::new(),
            workflow,
            default_status: Status::READY,
            default_project: None,
            theme,
            color,
            keys: KeyMap::default(),
            sources: Vec::new(),
            checked: Vec::new(),
            today: NaiveDate::default(),
            week_start: Weekday::Mon,
            due_format: DueFormat::Relative,
            filter: String::new(),
            selected: None,
            mode: Mode::Normal,
            history: Histories::default(),
            message: None,
            width: 0,
            height: 0,
            show_detail: false,
            quit: false,
        }
    }

    /// The status new tasks start in (`workflow.default_status` in the
    /// configuration).
    #[must_use]
    pub fn with_default_status(mut self, status: Status) -> Self {
        self.default_status = status;
        self
    }

    /// The key bindings (`[ui.keys]` laid over the defaults).
    #[must_use]
    pub fn with_keys(mut self, keys: KeyMap) -> Self {
        self.keys = keys;
        self
    }

    /// The day the form's `today` means (from the front end's clock).
    #[must_use]
    pub fn with_today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self
    }

    /// The first column of the calendar picker (`ui.week_start`).
    #[must_use]
    pub fn with_week_start(mut self, week_start: Weekday) -> Self {
        self.week_start = week_start;
        self
    }

    /// How list rows show a due date (`ui.due_format`).
    #[must_use]
    pub fn with_due_format(mut self, due_format: DueFormat) -> Self {
        self.due_format = due_format;
        self
    }

    /// The project new tasks track (the current directory, from the CLI).
    #[must_use]
    pub fn with_default_project(mut self, project: Option<PathBuf>) -> Self {
        self.default_project = project;
        self
    }

    /// The sources the picker offers (the enabled `[[source]]` blocks, from
    /// the CLI); the `auto` ones start checked.
    #[must_use]
    pub fn with_sources(mut self, sources: Vec<SourceChoice>) -> Self {
        self.checked = sources.iter().map(|s| s.auto).collect();
        self.sources = sources;
        self
    }

    /// The names of the checked sources, in config order.
    pub fn checked_sources(&self) -> Vec<String> {
        self.sources
            .iter()
            .zip(&self.checked)
            .filter(|(_, checked)| **checked)
            .map(|(source, _)| source.name.clone())
            .collect()
    }

    /// The draft for a task created from the UI: `title`, the default
    /// status, the default project, and the script's other defaults
    /// (priority `B`, no note). Priority, tags and the rest are set
    /// afterwards with `t`, `p` or the CLI.
    pub fn draft(&self, title: &str) -> TaskDraft {
        let draft = TaskDraft::new(title).with_status(Some(self.default_status.clone()));
        match &self.default_project {
            Some(project) => draft.with_project(project.clone()),
            None => draft,
        }
    }

    /// The layout for the current width.
    pub fn layout(&self) -> LayoutKind {
        if self.width >= TWO_PANE_MIN_WIDTH {
            LayoutKind::TwoPane
        } else {
            LayoutKind::OnePane
        }
    }

    /// The core filter for the filter text: `#word` is a status, priority
    /// or tag like `tasq <word>` (an invalid word matches nothing),
    /// anything else matches titles case-insensitively.
    pub fn filter(&self) -> Filter {
        let text = self.filter.trim();
        match text.strip_prefix('#') {
            // A lone `#` is not a word, so it falls back to a title search.
            Some(word) => Filter::from_word(word, &self.workflow)
                .unwrap_or_else(|_| Filter::default().text(text)),
            None => Filter::default().text(text),
        }
    }

    /// The status groups of the visible tasks, in CLI order.
    pub fn groups(&self) -> Vec<Group<'_>> {
        query::list(&self.tasks, &self.filter(), &self.workflow)
    }

    /// The list, headers included.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        for group in self.groups() {
            rows.push(Row::Header(group.status.clone(), group.tasks.len()));
            rows.extend(group.tasks.into_iter().map(Row::Task));
        }
        rows
    }

    /// The visible tasks in list order.
    pub fn visible(&self) -> Vec<&Task> {
        self.groups()
            .into_iter()
            .flat_map(|g| g.tasks.into_iter())
            .collect()
    }

    /// Position of the selected task among [`Model::visible`].
    pub fn selected_index(&self) -> Option<usize> {
        let id = self.selected.as_ref()?;
        self.visible().iter().position(|t| t.id == *id)
    }

    /// The selected task.
    pub fn selected_task(&self) -> Option<&Task> {
        let id = self.selected.as_ref()?;
        self.visible().into_iter().find(|t| t.id == *id)
    }

    /// Position of the selected task's row among [`Model::rows`].
    pub fn selected_row(&self) -> Option<usize> {
        let id = self.selected.as_ref()?;
        self.rows()
            .iter()
            .position(|row| matches!(row, Row::Task(t) if t.id == *id))
    }

    /// Replaces the tasks, keeping the selection when it is still visible
    /// and otherwise selecting the first visible task.
    pub fn set_tasks(&mut self, tasks: Vec<Task>) {
        self.tasks = tasks;
        self.fix_selection();
    }

    /// Replaces the filter text and keeps the selection visible.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.fix_selection();
    }

    /// Makes sure the selection names a visible task when there is one.
    pub fn fix_selection(&mut self) {
        let visible = self.visible();
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|id| visible.iter().any(|t| t.id == *id));
        if !still_there {
            self.selected = visible.first().map(|t| t.id.clone());
        }
    }

    /// Moves the selection by `delta` rows, clamped to the list.
    pub fn select_offset(&mut self, delta: isize) {
        let visible = self.visible();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let last = visible.len() - 1;
        let current = self.selected_index().unwrap_or(0);
        let target = current.saturating_add_signed(delta).min(last);
        self.selected = Some(visible[target].id.clone());
    }

    /// Selects the first visible task.
    pub fn select_first(&mut self) {
        self.selected = self.visible().first().map(|t| t.id.clone());
    }

    /// Selects the last visible task.
    pub fn select_last(&mut self) {
        self.selected = self.visible().last().map(|t| t.id.clone());
    }

    /// The status picker entries: the workflow's statuses.
    pub fn status_choices(&self) -> &[Status] {
        &self.workflow.statuses
    }

    /// The priority picker entries.
    pub fn priority_choices() -> [Priority; 3] {
        [Priority::A, Priority::B, Priority::C]
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use tasq_core::model::Tag;

    fn task(id: u64, title: &str, status: Option<Status>) -> Task {
        let mut t = Task::new(TaskId::from(id), title);
        t.status = status;
        t
    }

    fn model() -> Model {
        let mut m = Model::new(Workflow::default(), Theme::default(), true);
        let mut tagged = task(3, "Tagged thing", Some(Status::READY));
        tagged.add_tag(Tag::new("gitlab").unwrap());
        m.set_tasks(vec![
            task(1, "First", Some(Status::IN_PROGRESS)),
            task(2, "Second", Some(Status::READY)),
            tagged,
            task(4, "Loose", None),
        ]);
        m
    }

    #[test]
    fn layout_follows_the_width() {
        let mut m = Model::new(Workflow::default(), Theme::default(), true);
        assert_eq!(m.layout(), LayoutKind::OnePane);
        m.width = 99;
        assert_eq!(m.layout(), LayoutKind::OnePane);
        m.width = 100;
        assert_eq!(m.layout(), LayoutKind::TwoPane);
    }

    #[test]
    fn rows_are_headers_and_tasks_in_cli_order() {
        let m = model();
        let rows = m.rows();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0], Row::Header(Some(Status::IN_PROGRESS), 1));
        assert!(matches!(rows[1], Row::Task(t) if t.id == TaskId::from(1)));
        assert_eq!(rows[2], Row::Header(Some(Status::READY), 2));
        assert!(matches!(rows[3], Row::Task(t) if t.id == TaskId::from(2)));
        assert!(matches!(rows[4], Row::Task(t) if t.id == TaskId::from(3)));
        assert_eq!(rows[5], Row::Header(None, 1));
        assert!(matches!(rows[6], Row::Task(t) if t.id == TaskId::from(4)));
        let ids: Vec<&str> = m.visible().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3", "4"]);
    }

    #[test]
    fn filter_text_matches_titles_and_hash_words_are_cli_words() {
        let mut m = model();
        m.set_filter("SEC".into());
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.visible()[0].id, TaskId::from(2));
        m.set_filter("#ready".into());
        let ids: Vec<&str> = m.visible().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["2", "3"]);
        m.set_filter("#gitlab".into());
        assert_eq!(m.visible().len(), 1);
        m.set_filter("#B".into());
        assert_eq!(m.visible().len(), 4);
        m.set_filter(" # ".into());
        assert_eq!(m.visible().len(), 0, "a title containing '#'");
        m.set_filter("##bad".into());
        assert_eq!(m.visible().len(), 0);
        m.set_filter(String::new());
        assert_eq!(m.visible().len(), 4);
        assert_eq!(m.filter(), Filter::default().text(""));
    }

    #[test]
    fn selection_survives_reloads_and_filters() {
        let mut m = model();
        assert_eq!(m.selected, Some(TaskId::from(1)));
        m.selected = Some(TaskId::from(3));
        m.set_tasks(vec![
            task(3, "Tagged thing", Some(Status::READY)),
            task(9, "New", None),
        ]);
        assert_eq!(m.selected, Some(TaskId::from(3)));
        assert_eq!(m.selected_index(), Some(0));
        assert_eq!(m.selected_row(), Some(1));
        m.set_filter("New".into());
        assert_eq!(m.selected, Some(TaskId::from(9)));
        m.set_filter("nothing matches".into());
        assert_eq!(m.selected, None);
        assert_eq!(m.selected_task(), None);
        assert_eq!(m.selected_index(), None);
        assert_eq!(m.selected_row(), None);
        m.set_tasks(Vec::new());
        assert_eq!(m.selected, None);
    }

    #[test]
    fn movement_is_clamped() {
        let mut m = model();
        m.select_offset(1);
        assert_eq!(m.selected, Some(TaskId::from(2)));
        m.select_offset(-5);
        assert_eq!(m.selected, Some(TaskId::from(1)));
        m.select_offset(50);
        assert_eq!(m.selected, Some(TaskId::from(4)));
        m.select_first();
        assert_eq!(m.selected, Some(TaskId::from(1)));
        m.select_last();
        assert_eq!(m.selected, Some(TaskId::from(4)));
        m.selected = None;
        m.select_offset(1);
        assert_eq!(
            m.selected,
            Some(TaskId::from(2)),
            "from the top when nothing is selected"
        );
        m.set_tasks(Vec::new());
        m.select_offset(1);
        assert_eq!(m.selected, None);
        m.select_first();
        assert_eq!(m.selected, None);
        m.select_last();
        assert_eq!(m.selected, None);
    }

    #[test]
    fn drafts_take_the_default_status() {
        let m = model();
        assert_eq!(m.default_status, Status::READY);
        let d = m.draft("New");
        assert_eq!(d.title, "New");
        assert_eq!(d.status, Some(Status::READY));
        assert_eq!(d.priority, Priority::B);
        assert_eq!(d.note, None);
        assert!(!d.done);
        let m = m.with_default_status(Status::LATER);
        assert_eq!(m.draft("x").status, Some(Status::LATER));
        assert_eq!(m.draft("x").project, None);
        let m = m.with_default_project(Some(PathBuf::from("/work")));
        assert_eq!(m.draft("x").project.as_deref(), Some(Path::new("/work")));
        let m = m.with_default_project(None);
        assert_eq!(m.draft("x").project, None);
    }

    #[test]
    fn today_comes_from_the_front_end() {
        let m = Model::new(Workflow::default(), Theme::default(), true);
        assert_eq!(m.today, NaiveDate::default());
        let day = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(m.with_today(day).today, day);
        let m = Model::new(Workflow::default(), Theme::default(), true);
        assert_eq!(m.week_start, Weekday::Mon);
        assert_eq!(m.with_week_start(Weekday::Sun).week_start, Weekday::Sun);
        let m = Model::new(Workflow::default(), Theme::default(), true);
        assert_eq!(m.due_format, DueFormat::Relative);
        assert_eq!(
            m.with_due_format(DueFormat::Both).due_format,
            DueFormat::Both
        );
    }

    #[test]
    fn picker_choices() {
        let m = model();
        assert_eq!(m.status_choices(), &Status::DEFAULTS);
        assert_eq!(
            Model::priority_choices(),
            [Priority::A, Priority::B, Priority::C]
        );
        assert_eq!(
            Message::info("x"),
            Message {
                text: "x".into(),
                is_error: false
            }
        );
        assert_eq!(
            Message::error("x"),
            Message {
                text: "x".into(),
                is_error: true
            }
        );
    }
}
