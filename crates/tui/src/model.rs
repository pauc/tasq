//! The application state: what is on screen and what the keys mean right
//! now. Pure data with pure helpers; [`crate::update()`] mutates it and
//! [`crate::view()`] reads it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{NaiveDate, Weekday};
use tasq_core::config::{DetailPosition, DueFormat};
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

/// Terminal height from which the detail sits under the list, with
/// `ui.detail_position = "bottom"` (ADR-0021).
pub const STACKED_MIN_HEIGHT: u16 = 20;

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
    /// Tall enough for the detail under the list (`ui.detail_position =
    /// "bottom"`).
    Stacked,
    /// One pane: the list, or the detail of the selected task.
    OnePane,
}

/// Which group of the list: a status group or the DONE group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupKey {
    /// The open tasks in a status (`None` is the no-status group).
    Status(Option<Status>),
    /// The done tasks (`a`), newest first.
    Done,
}

/// One group of the list, never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGroup<'a> {
    /// Which group.
    pub key: GroupKey,
    /// Its tasks, in list order.
    pub tasks: Vec<&'a Task>,
}

/// One line of the task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row<'a> {
    /// A group header and the number of tasks in the group, folded or not.
    Header(GroupKey, usize),
    /// A task of the group above.
    Task(&'a Task),
}

/// Where the list cursor rests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    /// A task of an unfolded group.
    Task(TaskId),
    /// The header of a folded group.
    Group(GroupKey),
}

/// The list views switched on and off by a key, shown in the list title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Toggles {
    /// The DONE group is listed (`a`, `+done`).
    pub done: bool,
    /// The Today view (`T`, `+today`): only open tasks in the first
    /// status or due by [`Model::today`] ([`query::is_today`]), and the
    /// tasks closed that day in the DONE group.
    pub today: bool,
}

/// The whole state of the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Open and done tasks as last loaded from the store; the list shows
    /// the done ones only with [`Toggles::done`].
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
    /// Where the detail goes (`ui.detail_position`).
    pub detail_position: DetailPosition,
    /// The applied filter text (see [`Model::filter`]).
    pub filter: String,
    /// The list cursor, when anything is visible.
    pub selected: Option<Cursor>,
    /// The groups folded to their header (`z`), kept for the session.
    pub collapsed: BTreeSet<GroupKey>,
    /// The list views that are on; all off at start.
    pub toggles: Toggles,
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
            detail_position: DetailPosition::Right,
            filter: String::new(),
            selected: None,
            collapsed: BTreeSet::new(),
            toggles: Toggles::default(),
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
        match self.detail_position {
            DetailPosition::Right if self.width >= TWO_PANE_MIN_WIDTH => LayoutKind::TwoPane,
            DetailPosition::Bottom if self.height >= STACKED_MIN_HEIGHT => LayoutKind::Stacked,
            DetailPosition::Right | DetailPosition::Bottom => LayoutKind::OnePane,
        }
    }

    /// Where the detail goes (`ui.detail_position`).
    #[must_use]
    pub fn with_detail_position(mut self, detail_position: DetailPosition) -> Self {
        self.detail_position = detail_position;
        self
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

    /// The groups of the visible tasks: the status groups of the open
    /// ones in CLI order, then, with [`Toggles::done`], the done ones
    /// that pass the same filter, newest first ([`query::sort_done`]).
    pub fn groups(&self) -> Vec<ListGroup<'_>> {
        let filter = self.filter();
        let in_scope = self.tasks.iter().filter(|t| self.in_scope(t));
        let mut groups: Vec<ListGroup<'_>> = query::list(in_scope.clone(), &filter, &self.workflow)
            .into_iter()
            .map(|Group { status, tasks }| ListGroup {
                key: GroupKey::Status(status),
                tasks,
            })
            .collect();
        if self.toggles.done {
            let done = query::sort_done(query::filter(in_scope, &filter.done(true)));
            if !done.is_empty() {
                groups.push(ListGroup {
                    key: GroupKey::Done,
                    tasks: done,
                });
            }
        }
        groups
    }

    /// Whether the toggles let `task` into the list, before the filter:
    /// open tasks (only today's with [`Toggles::today`]), and done ones
    /// with [`Toggles::done`] (only those closed today with both).
    fn in_scope(&self, task: &Task) -> bool {
        match (task.done, self.toggles.today) {
            (false, false) => true,
            (false, true) => query::is_today(task, self.today, &self.workflow),
            (true, false) => self.toggles.done,
            (true, true) => self.toggles.done && query::closed_on(task, self.today),
        }
    }

    /// How many tasks the list could show with no filter: the tasks the
    /// toggles let in.
    pub fn total(&self) -> usize {
        self.tasks.iter().filter(|t| self.in_scope(t)).count()
    }

    /// Switches the Today view (`T`) on or off.
    pub fn toggle_today(&mut self) {
        self.toggles.today = !self.toggles.today;
        self.fix_selection();
    }

    /// Shows or hides the DONE group (`a`). It always appears unfolded,
    /// even when it was folded with `z` before it was hidden: the point of
    /// `a` is to see the done tasks.
    pub fn toggle_done(&mut self) {
        self.toggles.done = !self.toggles.done;
        if self.toggles.done {
            self.collapsed.remove(&GroupKey::Done);
        }
        self.fix_selection();
    }

    /// The list, headers included; a folded group is its header alone.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut rows = Vec::new();
        for group in self.groups() {
            rows.push(Row::Header(group.key.clone(), group.tasks.len()));
            if !self.collapsed.contains(&group.key) {
                rows.extend(group.tasks.into_iter().map(Row::Task));
            }
        }
        rows
    }

    /// Where the cursor can rest, in list order: the tasks of unfolded
    /// groups and the headers of folded ones.
    pub fn stops(&self) -> Vec<Cursor> {
        let mut stops = Vec::new();
        for group in self.groups() {
            if self.collapsed.contains(&group.key) {
                stops.push(Cursor::Group(group.key));
            } else {
                stops.extend(group.tasks.iter().map(|t| Cursor::Task(t.id.clone())));
            }
        }
        stops
    }

    /// The group the visible task `id` is listed under.
    fn group_of(&self, id: &TaskId) -> Option<ListGroup<'_>> {
        self.groups()
            .into_iter()
            .find(|g| g.tasks.iter().any(|t| t.id == *id))
    }

    /// The visible tasks in list order.
    pub fn visible(&self) -> Vec<&Task> {
        self.groups()
            .into_iter()
            .flat_map(|g| g.tasks.into_iter())
            .collect()
    }

    /// Position of the cursor among [`Model::stops`].
    pub fn selected_index(&self) -> Option<usize> {
        let cursor = self.selected.as_ref()?;
        self.stops().iter().position(|c| c == cursor)
    }

    /// The selected task; `None` on a folded group's header.
    pub fn selected_task(&self) -> Option<&Task> {
        let Some(Cursor::Task(id)) = &self.selected else {
            return None;
        };
        self.visible().into_iter().find(|t| t.id == *id)
    }

    /// Position of the cursor's row among [`Model::rows`].
    pub fn selected_row(&self) -> Option<usize> {
        let cursor = self.selected.as_ref()?;
        self.rows().iter().position(|row| match (row, cursor) {
            (Row::Task(t), Cursor::Task(id)) => t.id == *id,
            (Row::Header(key, _), Cursor::Group(group)) => key == group,
            _ => false,
        })
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

    /// Makes sure the cursor is on a stop when there is one: a task that
    /// moved into a folded group leaves it on that group's header, and a
    /// cursor on something no longer listed goes to the first stop.
    pub fn fix_selection(&mut self) {
        let cursor = self.selected.take().map(|cursor| match cursor {
            Cursor::Task(id) => match self.group_of(&id) {
                Some(group) if self.collapsed.contains(&group.key) => Cursor::Group(group.key),
                _ => Cursor::Task(id),
            },
            group @ Cursor::Group(_) => group,
        });
        let stops = self.stops();
        self.selected = cursor
            .filter(|c| stops.contains(c))
            .or_else(|| stops.first().cloned());
    }

    /// Selects the visible task `id`, unfolding its group; a task hidden
    /// by the filter stays unselected.
    pub fn select_task(&mut self, id: TaskId) {
        if let Some(key) = self.group_of(&id).map(|g| g.key) {
            self.collapsed.remove(&key);
            self.selected = Some(Cursor::Task(id));
        }
    }

    /// Folds the selected task's group to its header, or unfolds the
    /// folded group under the cursor and selects its first task.
    pub fn toggle_group(&mut self) {
        match self.selected.take() {
            Some(Cursor::Task(id)) => {
                if let Some(key) = self.group_of(&id).map(|g| g.key) {
                    self.collapsed.insert(key.clone());
                    self.selected = Some(Cursor::Group(key));
                }
            }
            Some(Cursor::Group(key)) => {
                self.collapsed.remove(&key);
                self.selected = self
                    .groups()
                    .into_iter()
                    .find(|g| g.key == key)
                    .and_then(|g| g.tasks.first().map(|t| Cursor::Task(t.id.clone())));
            }
            None => {}
        }
    }

    /// Moves the cursor by `delta` stops, clamped to the list.
    pub fn select_offset(&mut self, delta: isize) {
        let stops = self.stops();
        if stops.is_empty() {
            self.selected = None;
            return;
        }
        let last = stops.len() - 1;
        let current = self.selected_index().unwrap_or(0);
        let target = current.saturating_add_signed(delta).min(last);
        self.selected = Some(stops[target].clone());
    }

    /// Puts the cursor on the first stop.
    pub fn select_first(&mut self) {
        self.selected = self.stops().first().cloned();
    }

    /// Puts the cursor on the last stop.
    pub fn select_last(&mut self) {
        self.selected = self.stops().last().cloned();
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
        // At the bottom, the height decides and the width does not.
        let mut m = m.with_detail_position(DetailPosition::Bottom);
        m.height = 19;
        assert_eq!(m.layout(), LayoutKind::OnePane);
        m.height = 20;
        assert_eq!(m.layout(), LayoutKind::Stacked);
        m.width = 40;
        assert_eq!(m.layout(), LayoutKind::Stacked);
        // And on the right, the height does not.
        let mut m = m.with_detail_position(DetailPosition::Right);
        m.width = 100;
        m.height = 5;
        assert_eq!(m.layout(), LayoutKind::TwoPane);
    }

    #[test]
    fn rows_are_headers_and_tasks_in_cli_order() {
        let m = model();
        let rows = m.rows();
        assert_eq!(rows.len(), 7);
        assert_eq!(
            rows[0],
            Row::Header(GroupKey::Status(Some(Status::IN_PROGRESS)), 1)
        );
        assert!(matches!(rows[1], Row::Task(t) if t.id == TaskId::from(1)));
        assert_eq!(
            rows[2],
            Row::Header(GroupKey::Status(Some(Status::READY)), 2)
        );
        assert!(matches!(rows[3], Row::Task(t) if t.id == TaskId::from(2)));
        assert!(matches!(rows[4], Row::Task(t) if t.id == TaskId::from(3)));
        assert_eq!(rows[5], Row::Header(GroupKey::Status(None), 1));
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
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        m.selected = Some(Cursor::Task(TaskId::from(3)));
        m.set_tasks(vec![
            task(3, "Tagged thing", Some(Status::READY)),
            task(9, "New", None),
        ]);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(3))));
        assert_eq!(m.selected_index(), Some(0));
        assert_eq!(m.selected_row(), Some(1));
        m.set_filter("New".into());
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(9))));
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
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        m.select_offset(-5);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        m.select_offset(50);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        m.select_first();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        m.select_last();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        m.selected = None;
        m.select_offset(1);
        assert_eq!(
            m.selected,
            Some(Cursor::Task(TaskId::from(2))),
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
    fn folded_groups_are_one_stop() {
        let ready = Cursor::Group(GroupKey::Status(Some(Status::READY)));
        let mut m = model();
        m.selected = Some(Cursor::Task(TaskId::from(3)));
        m.toggle_group();
        assert_eq!(m.selected, Some(ready.clone()));
        assert_eq!(
            m.collapsed,
            BTreeSet::from([GroupKey::Status(Some(Status::READY))])
        );
        assert_eq!(m.selected_task(), None, "a header is not a task");
        let rows = m.rows();
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[2],
            Row::Header(GroupKey::Status(Some(Status::READY)), 2)
        );
        assert_eq!(rows[3], Row::Header(GroupKey::Status(None), 1));
        assert_eq!(
            m.stops(),
            [
                Cursor::Task(TaskId::from(1)),
                ready.clone(),
                Cursor::Task(TaskId::from(4)),
            ]
        );
        assert_eq!(m.selected_index(), Some(1));
        assert_eq!(m.selected_row(), Some(2));
        m.select_offset(1);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        m.select_offset(-1);
        assert_eq!(m.selected, Some(ready.clone()));
        m.collapsed.insert(GroupKey::Status(None));
        m.select_last();
        assert_eq!(m.selected, Some(Cursor::Group(GroupKey::Status(None))));
        assert_eq!(m.selected_row(), Some(3));
        m.toggle_group();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        m.selected = Some(ready);
        m.toggle_group();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        assert_eq!(m.collapsed, BTreeSet::new());
        m.selected = None;
        m.toggle_group();
        assert_eq!(m.selected, None);
        assert_eq!(m.collapsed, BTreeSet::new());
    }

    #[test]
    fn the_cursor_follows_tasks_into_folded_groups() {
        let mut m = model();
        m.collapsed.insert(GroupKey::Status(Some(Status::READY)));
        // A reload that moves the selected task into a folded group.
        m.selected = Some(Cursor::Task(TaskId::from(1)));
        m.set_tasks(vec![
            task(5, "Started", Some(Status::IN_PROGRESS)),
            task(1, "First", Some(Status::READY)),
            task(2, "Second", Some(Status::READY)),
            task(4, "Loose", None),
        ]);
        assert_eq!(
            m.selected,
            Some(Cursor::Group(GroupKey::Status(Some(Status::READY))))
        );
        // A filter that empties the folded group sends the cursor to the top.
        m.set_filter("Loose".into());
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        m.set_filter(String::new());
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        // Selecting a task by id unfolds its group; a hidden one is ignored.
        m.select_task(TaskId::from(2));
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
        assert_eq!(m.collapsed, BTreeSet::new());
        m.select_task(TaskId::from(99));
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(2))));
    }

    #[test]
    fn the_done_group_is_on_demand_folded_filtered_and_newest_first() {
        use tasq_core::clock::FixedClock;
        let done_at = |id: u64, title: &str, at: &str| {
            let mut t = task(id, title, None);
            t.close(&FixedClock::at(at));
            t
        };
        let mut m = model();
        let mut tasks = m.tasks.clone();
        tasks.push(done_at(6, "Older second", "2026-10-01 09:00"));
        tasks.push(done_at(7, "Newer", "2026-10-05 17:10"));
        m.set_tasks(tasks);
        let done = GroupKey::Done;

        // Hidden by default: the list and the total are the open tasks.
        assert_eq!(m.rows().len(), 7);
        assert_eq!(m.total(), 4);
        assert_eq!(m.visible().len(), 4);

        m.toggle_done();
        assert!(m.toggles.done);
        assert_eq!(m.total(), 6);
        assert_eq!(m.collapsed, BTreeSet::new(), "shown unfolded");
        assert_eq!(m.rows()[7], Row::Header(done.clone(), 2));
        assert_eq!(m.rows().len(), 10);
        let ids: Vec<&str> = m.visible().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3", "4", "7", "6"]);
        m.select_last();
        assert_eq!(m.selected_task().map(|t| t.id.as_str()), Some("6"));
        m.select_offset(-1);
        assert_eq!(m.selected_task().map(|t| t.id.as_str()), Some("7"));

        // The filter applies to the done tasks too; `#status` never matches one.
        m.set_filter("second".into());
        let ids: Vec<&str> = m.visible().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["2", "6"]);
        m.set_filter("#ready".into());
        let ids: Vec<&str> = m.visible().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["2", "3"]);
        m.set_filter("Newer".into());
        assert_eq!(m.selected_task().map(|t| t.id.as_str()), Some("7"));

        // Hiding it moves a cursor that was on a done task back to the top.
        m.set_filter(String::new());
        m.toggle_done();
        assert!(!m.toggles.done);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
        assert_eq!(m.total(), 4);
        // Folded with `z`, hidden and shown again: unfolded.
        m.toggle_done();
        m.collapsed.insert(done.clone());
        m.toggle_done();
        m.toggle_done();
        assert_eq!(m.collapsed, BTreeSet::new());
        // Other folds are left alone.
        m.collapsed.insert(GroupKey::Status(None));
        m.toggle_done();
        assert_eq!(m.collapsed, BTreeSet::from([GroupKey::Status(None)]));
        m.collapsed.clear();
        // No done task: no DONE header.
        m.set_filter("First".into());
        assert_eq!(m.rows().len(), 2);
    }

    #[test]
    fn the_today_view_keeps_doing_due_and_overdue_and_today_s_closes() {
        use tasq_core::clock::FixedClock;
        let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let mut m = model().with_today(day("2026-10-07"));
        let mut tasks = m.tasks.clone();
        tasks[1].due = Some(day("2026-10-07")); // 2, ready, due today
        tasks[2].due = Some(day("2026-10-08")); // 3, ready, due tomorrow
        tasks[3].due = Some(day("2026-10-01")); // 4, no status, overdue
        let mut today = task(6, "Closed today", None);
        today.close(&FixedClock::at("2026-10-07 09:00"));
        let mut earlier = task(7, "Closed earlier", None);
        earlier.close(&FixedClock::at("2026-10-06 09:00"));
        tasks.extend([today, earlier]);
        m.set_tasks(tasks);
        let ids =
            |m: &Model| -> Vec<String> { m.visible().iter().map(|t| t.id.to_string()).collect() };

        m.toggle_today();
        assert!(m.toggles.today);
        assert_eq!(ids(&m), ["1", "2", "4"]);
        assert_eq!(m.total(), 3);
        // With the DONE group: only what was closed today.
        m.toggle_done();
        assert_eq!(ids(&m), ["1", "2", "4", "6"]);
        assert_eq!(m.total(), 4);
        // The filter still applies on top.
        m.set_filter("first".into());
        assert_eq!(ids(&m), ["1"]);
        m.set_filter(String::new());
        // Off again: everything, the selection kept when still listed.
        m.select_task(TaskId::from(4));
        m.toggle_today();
        assert!(!m.toggles.today);
        assert_eq!(ids(&m), ["1", "2", "3", "4", "6", "7"]);
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(4))));
        // A selected task that the view hides: the cursor goes to the top.
        m.select_task(TaskId::from(3));
        m.toggle_today();
        assert_eq!(m.selected, Some(Cursor::Task(TaskId::from(1))));
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
