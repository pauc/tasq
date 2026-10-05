//! The edit form (`e`, ADR-0015): the six fields of a task on one screen,
//! typed and cycled in place, validated on `Enter` into the
//! [`Fields`] that [`tasq_core::edit::revise`] writes.
//!
//! Pure data, like the rest of the model: [`crate::update()`] drives it
//! and [`crate::view()`] draws it.

use std::path::PathBuf;
use std::str::FromStr;

use chrono::NaiveDate;
use tasq_core::dates::parse_day;
use tasq_core::edit::Fields;
use tasq_core::model::{Priority, Status, Tag, Task, TaskId, Workflow};

/// A row of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The title line (text).
    Title,
    /// The workflow status, or none (choice).
    Status,
    /// `A`, `B` or `C` (choice).
    Priority,
    /// `## Due` as typed: ISO or `today`/`tomorrow`/`yesterday`; empty clears (text).
    Due,
    /// `## Project` path; empty clears (text).
    Project,
    /// Topic tags, space-separated, `#` optional (text).
    Tags,
}

impl Field {
    /// The rows, top to bottom.
    pub const ALL: [Field; 6] = [
        Field::Title,
        Field::Status,
        Field::Priority,
        Field::Due,
        Field::Project,
        Field::Tags,
    ];

    /// The row label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Status => "Status",
            Self::Priority => "Priority",
            Self::Due => "Due",
            Self::Project => "Project",
            Self::Tags => "Tags",
        }
    }

    /// Whether the row is typed into (else it is cycled with Left/Right).
    pub fn is_text(self) -> bool {
        !matches!(self, Self::Status | Self::Priority)
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    /// The row below, or this one at the bottom.
    #[must_use]
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1).min(Self::ALL.len() - 1)]
    }

    /// The row above, or this one at the top.
    #[must_use]
    pub fn prev(self) -> Self {
        Self::ALL[self.index().saturating_sub(1)]
    }
}

/// Why the form cannot be saved: which row, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormError {
    /// The row to put the focus on.
    pub field: Field,
    /// The status-bar message.
    pub message: String,
}

/// The form's state: the task it edits, the rows as typed or chosen, and
/// the focused row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// The task being edited.
    pub id: TaskId,
    /// The title as typed.
    pub title: String,
    /// Index into [`Form::status_choices`].
    pub status: usize,
    /// Index into [`crate::model::Model::priority_choices`].
    pub priority: usize,
    /// The due date as typed.
    pub due: String,
    /// The project path as typed.
    pub project: String,
    /// The tags as typed.
    pub tags: String,
    /// The focused row.
    pub focus: Field,
}

impl Form {
    /// The form for `task`, its fields filled in, the focus on the title.
    /// A status the workflow does not know shows as `none`.
    pub fn of(task: &Task, workflow: &Workflow) -> Self {
        let none = workflow.statuses.len();
        let status = task
            .status
            .as_ref()
            .map_or(none, |s| workflow.position(s).unwrap_or(none));
        let priority = crate::model::Model::priority_choices()
            .iter()
            .position(|p| *p == task.priority)
            .unwrap_or(1);
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            status,
            priority,
            due: task
                .due
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            project: task
                .project
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            tags: task
                .tags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            focus: Field::Title,
        }
    }

    /// The status row's choices: the workflow's statuses, then `None`.
    pub fn status_choices(workflow: &Workflow) -> Vec<Option<Status>> {
        let mut choices: Vec<Option<Status>> =
            workflow.statuses.iter().cloned().map(Some).collect();
        choices.push(None);
        choices
    }

    /// The chosen status.
    pub fn chosen_status(&self, workflow: &Workflow) -> Option<Status> {
        Self::status_choices(workflow)
            .get(self.status)
            .cloned()
            .flatten()
    }

    /// The chosen priority.
    pub fn chosen_priority(&self) -> Priority {
        crate::model::Model::priority_choices()[self.priority.min(2)]
    }

    /// What a row shows: the typed text, or the chosen status (`none` for
    /// no status) or priority.
    pub fn value(&self, field: Field, workflow: &Workflow) -> String {
        match field {
            Field::Title => self.title.clone(),
            Field::Status => self
                .chosen_status(workflow)
                .map_or_else(|| "none".to_owned(), |s| s.as_str().to_owned()),
            Field::Priority => self.chosen_priority().as_str().to_owned(),
            Field::Due => self.due.clone(),
            Field::Project => self.project.clone(),
            Field::Tags => self.tags.clone(),
        }
    }

    /// The focused row's text, when it is a text row.
    fn text_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            Field::Title => Some(&mut self.title),
            Field::Due => Some(&mut self.due),
            Field::Project => Some(&mut self.project),
            Field::Tags => Some(&mut self.tags),
            Field::Status | Field::Priority => None,
        }
    }

    /// Types `c` into the focused text row.
    pub fn type_char(&mut self, c: char) {
        if let Some(text) = self.text_mut() {
            text.push(c);
        }
    }

    /// Appends `text` (already one line) to the focused text row.
    pub fn paste(&mut self, text: &str) {
        if let Some(field) = self.text_mut() {
            field.push_str(text);
        }
    }

    /// Deletes the last character of the focused text row.
    pub fn backspace(&mut self) {
        if let Some(text) = self.text_mut() {
            text.pop();
        }
    }

    /// Moves the focus down (`Down`, `Tab`).
    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// Moves the focus up (`Up`, `Shift+Tab`).
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// Cycles the focused choice row by `delta` (`Left` is -1, `Right` is
    /// 1), wrapping around; nothing on a text row.
    pub fn cycle(&mut self, delta: isize, workflow: &Workflow) {
        let (index, count) = match self.focus {
            Field::Status => (&mut self.status, Self::status_choices(workflow).len()),
            Field::Priority => (&mut self.priority, 3),
            _ => return,
        };
        let count = count.cast_signed();
        let next = (index.cast_signed() + delta).rem_euclid(count);
        *index = next.cast_unsigned();
    }

    /// The fields to save: the title trimmed and non-empty, the due date
    /// resolved against `today`, the project as a path, the tags parsed.
    /// The first bad row is reported with its message.
    pub fn fields(&self, workflow: &Workflow, today: NaiveDate) -> Result<Fields, FormError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(FormError {
                field: Field::Title,
                message: "the title must not be empty".to_owned(),
            });
        }
        let due = match self.due.trim() {
            "" => None,
            text => Some(parse_day(text, today).map_err(|e| FormError {
                field: Field::Due,
                message: format!("due: {e}"),
            })?),
        };
        let project = match self.project.trim() {
            "" => None,
            text => Some(PathBuf::from(text)),
        };
        let mut tags: Vec<Tag> = Vec::new();
        for word in self.tags.split_whitespace() {
            let tag = Tag::from_str(word).map_err(|e| FormError {
                field: Field::Tags,
                message: format!("tags: {e}"),
            })?;
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        Ok(Fields {
            title: title.to_owned(),
            status: self.chosen_status(workflow),
            priority: self.chosen_priority(),
            due,
            project,
            tags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    }

    fn task() -> Task {
        let mut t = Task::new(TaskId::from(7), "Seven");
        t.set_status(Status::WAITING);
        t.set_priority(Priority::A);
        t.due = Some(today());
        t.project = Some(PathBuf::from("/p"));
        t.add_tag(Tag::new("gitlab").unwrap());
        t.add_tag(Tag::new("x").unwrap());
        t
    }

    #[test]
    fn fields_have_labels_kinds_and_an_order() {
        assert_eq!(
            Field::ALL.map(Field::label),
            ["Title", "Status", "Priority", "Due", "Project", "Tags"]
        );
        assert_eq!(
            Field::ALL.map(Field::is_text),
            [true, false, false, true, true, true]
        );
        assert_eq!(Field::Title.prev(), Field::Title);
        assert_eq!(Field::Title.next(), Field::Status);
        assert_eq!(Field::Tags.next(), Field::Tags);
        assert_eq!(Field::Tags.prev(), Field::Project);
        assert_eq!(Field::Due.index(), 3);
    }

    #[test]
    fn a_form_shows_the_task_and_round_trips_it() {
        let wf = Workflow::default();
        let form = Form::of(&task(), &wf);
        assert_eq!(form.id, TaskId::from(7));
        assert_eq!(form.focus, Field::Title);
        assert_eq!(form.value(Field::Title, &wf), "Seven");
        assert_eq!(form.value(Field::Status, &wf), "waiting");
        assert_eq!(form.value(Field::Priority, &wf), "A");
        assert_eq!(form.value(Field::Due, &wf), "2026-10-05");
        assert_eq!(form.value(Field::Project, &wf), "/p");
        assert_eq!(form.value(Field::Tags, &wf), "gitlab x");
        assert_eq!(form.status, wf.position(&Status::WAITING).unwrap());
        assert_eq!(form.priority, 0);
        assert_eq!(form.fields(&wf, today()).unwrap(), Fields::of(&task()));

        let mut odd = Task::new(TaskId::from(2), "Odd");
        odd.set_status(Status::new("weird").unwrap());
        assert_eq!(
            Form::of(&odd, &wf).status,
            wf.statuses.len(),
            "an unknown status shows as none"
        );

        let blank = Form::of(&Task::new(TaskId::from(1), "B"), &wf);
        assert_eq!(blank.status, wf.statuses.len(), "none is last");
        assert_eq!(blank.priority, 1);
        assert_eq!(blank.value(Field::Status, &wf), "none");
        assert_eq!(blank.value(Field::Due, &wf), "");
        assert_eq!(blank.value(Field::Project, &wf), "");
        assert_eq!(blank.value(Field::Tags, &wf), "");
        let fields = blank.fields(&wf, today()).unwrap();
        assert_eq!(fields, Fields::of(&Task::new(TaskId::from(1), "B")));
        assert_eq!(fields.status, None);
    }

    #[test]
    fn status_choices_end_with_none() {
        let wf = Workflow::default();
        let choices = Form::status_choices(&wf);
        assert_eq!(choices.len(), wf.statuses.len() + 1);
        assert_eq!(choices[0], Some(wf.statuses[0].clone()));
        assert_eq!(choices[choices.len() - 1], None);
        let mut form = Form::of(&task(), &wf);
        form.status = 99;
        assert_eq!(form.chosen_status(&wf), None, "out of range is none");
        form.priority = 99;
        assert_eq!(form.chosen_priority(), Priority::C);
    }

    #[test]
    fn typing_goes_to_the_focused_text_row_only() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        form.type_char('!');
        assert_eq!(form.title, "Seven!");
        form.backspace();
        assert_eq!(form.title, "Seven");
        form.paste(" more");
        assert_eq!(form.title, "Seven more");
        form.focus_next();
        assert_eq!(form.focus, Field::Status);
        let before = form.clone();
        form.type_char('x');
        form.backspace();
        form.paste("y");
        assert_eq!(form, before, "a choice row ignores typing");
        form.focus_next();
        form.focus_next();
        assert_eq!(form.focus, Field::Due);
        form.type_char('x');
        assert_eq!(form.due, "2026-10-05x");
        form.focus_next();
        form.type_char('x');
        assert_eq!(form.project, "/px");
        form.focus_next();
        form.type_char('x');
        assert_eq!(form.tags, "gitlab xx");
        form.focus_next();
        assert_eq!(form.focus, Field::Tags, "clamped at the bottom");
        for _ in 0..9 {
            form.focus_prev();
        }
        assert_eq!(form.focus, Field::Title, "clamped at the top");
    }

    #[test]
    fn cycling_wraps_on_choice_rows_and_does_nothing_elsewhere() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        let before = form.clone();
        form.cycle(1, &wf);
        assert_eq!(form, before, "the title row does not cycle");
        form.focus = Field::Priority;
        form.cycle(-1, &wf);
        assert_eq!(form.chosen_priority(), Priority::C, "wraps backwards");
        form.cycle(1, &wf);
        assert_eq!(form.chosen_priority(), Priority::A);
        form.cycle(2, &wf);
        assert_eq!(form.chosen_priority(), Priority::C);
        form.focus = Field::Status;
        let last = wf.statuses.len();
        form.status = last;
        form.cycle(1, &wf);
        assert_eq!(form.status, 0, "wraps forwards");
        form.cycle(-1, &wf);
        assert_eq!(form.status, last);
        assert_eq!(form.chosen_status(&wf), None);
    }

    #[test]
    fn fields_are_validated_row_by_row() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        form.title = " \t".into();
        assert_eq!(
            form.fields(&wf, today()),
            Err(FormError {
                field: Field::Title,
                message: "the title must not be empty".into()
            })
        );
        form.title = "  Trimmed  ".into();
        form.due = "soon".into();
        let err = form.fields(&wf, today()).unwrap_err();
        assert_eq!(err.field, Field::Due);
        assert!(err.message.starts_with("due: "), "{}", err.message);
        form.due = " tomorrow ".into();
        form.tags = "ok ##bad".into();
        let err = form.fields(&wf, today()).unwrap_err();
        assert_eq!(err.field, Field::Tags);
        assert!(err.message.starts_with("tags: "), "{}", err.message);
        form.tags = " #a b #a ".into();
        form.project = "  ".into();
        let fields = form.fields(&wf, today()).unwrap();
        assert_eq!(fields.title, "Trimmed");
        assert_eq!(fields.due, Some(today() + chrono::Duration::days(1)));
        assert_eq!(fields.project, None);
        assert_eq!(
            fields.tags,
            vec![Tag::new("a").unwrap(), Tag::new("b").unwrap()]
        );
        form.due = String::new();
        form.project = " /q ".into();
        let fields = form.fields(&wf, today()).unwrap();
        assert_eq!(fields.due, None);
        assert_eq!(fields.project, Some(PathBuf::from("/q")));
    }
}
