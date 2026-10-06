//! The edit view (`e`, ADR-0016): every field of a task on one screen,
//! typed and cycled in place, validated on save into the [`Fields`] that
//! [`tasq_core::edit::revise`] writes.
//!
//! Pure data, like the rest of the model: [`crate::update()`] drives it
//! and [`crate::view()`] draws it. [`Text`] is the small editor behind the
//! text rows and the description, and behind the status-bar prompts
//! (filter, notes, a new title): lines, a cursor, and the usual keys.

use std::path::PathBuf;
use std::str::FromStr;

use chrono::NaiveDate;
use tasq_core::dates::parse_day;
use tasq_core::edit::Fields;
use tasq_core::model::{Priority, Status, Tag, Task, TaskId, Workflow};

/// A row of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The title line (one line of text).
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
    /// `## Description`, as many lines as needed; empty clears (text).
    Description,
}

impl Field {
    /// The rows, top to bottom.
    pub const ALL: [Field; 7] = [
        Field::Title,
        Field::Status,
        Field::Priority,
        Field::Due,
        Field::Project,
        Field::Tags,
        Field::Description,
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
            Self::Description => "Description",
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

/// Editable text: lines and a cursor (`row`, `col` in characters). A
/// single-line row or prompt is a `Text` that never gets a newline (the
/// [`Form`] and [`crate::update()`] see to that); the description takes
/// as many lines as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl Text {
    /// One line, the cursor after its last character.
    pub fn single(text: &str) -> Self {
        let col = text.chars().count();
        Self {
            lines: vec![text.to_owned()],
            row: 0,
            col,
        }
    }

    /// The lines of `text`, the cursor at the start.
    pub fn multi(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(str::to_owned).collect(),
            row: 0,
            col: 0,
        }
    }

    /// The lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The cursor: line index and character offset in that line.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// The text, lines joined by `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    /// Byte offset of character `col` in the current line (its length at
    /// the end).
    fn byte(&self) -> usize {
        self.lines[self.row]
            .char_indices()
            .nth(self.col)
            .map_or(self.lines[self.row].len(), |(i, _)| i)
    }

    /// Inserts `c` at the cursor (`\n` splits the line).
    pub fn insert(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        let at = self.byte();
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    /// Splits the line at the cursor; the cursor starts the new line.
    pub fn newline(&mut self) {
        let at = self.byte();
        let rest = self.lines[self.row].split_off(at);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    /// Inserts `text`, newlines included.
    pub fn paste(&mut self, text: &str) {
        for c in text.chars() {
            self.insert(c);
        }
    }

    /// Deletes the character before the cursor; at the start of a line,
    /// joins it to the previous one.
    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            let at = self.byte();
            self.lines[self.row].remove(at);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len(self.row);
            self.lines[self.row].push_str(&line);
        }
    }

    /// Deletes the character under the cursor; at the end of a line, joins
    /// the next one to it.
    pub fn delete(&mut self) {
        if self.col < self.line_len(self.row) {
            let at = self.byte();
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let line = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&line);
        }
    }

    /// Moves left, wrapping to the end of the previous line. `false` at the
    /// very start.
    pub fn left(&mut self) -> bool {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len(self.row);
        } else {
            return false;
        }
        true
    }

    /// Moves right, wrapping to the start of the next line. `false` at the
    /// very end.
    pub fn right(&mut self) -> bool {
        if self.col < self.line_len(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        } else {
            return false;
        }
        true
    }

    /// Moves up a line, keeping the column where the line allows. `false`
    /// on the first line.
    pub fn up(&mut self) -> bool {
        if self.row == 0 {
            return false;
        }
        self.row -= 1;
        self.col = self.col.min(self.line_len(self.row));
        true
    }

    /// Moves down a line, keeping the column where the line allows.
    /// `false` on the last line.
    pub fn down(&mut self) -> bool {
        if self.row + 1 >= self.lines.len() {
            return false;
        }
        self.row += 1;
        self.col = self.col.min(self.line_len(self.row));
        true
    }

    /// To the start of the line.
    pub fn home(&mut self) {
        self.col = 0;
    }

    /// To the end of the line.
    pub fn end(&mut self) {
        self.col = self.line_len(self.row);
    }
}

/// Why the view cannot be saved: which row, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormError {
    /// The row to put the focus on.
    pub field: Field,
    /// The status-bar message.
    pub message: String,
}

/// The view's state: the task it edits, the rows as typed or chosen, and
/// the focused row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// The task being edited.
    pub id: TaskId,
    /// The title as typed.
    pub title: Text,
    /// Index into [`Form::status_choices`].
    pub status: usize,
    /// Index into [`crate::model::Model::priority_choices`].
    pub priority: usize,
    /// The due date as typed.
    pub due: Text,
    /// The project path as typed.
    pub project: Text,
    /// The tags as typed.
    pub tags: Text,
    /// The description as typed.
    pub description: Text,
    /// The focused row.
    pub focus: Field,
}

impl Form {
    /// The view for `task`, its fields filled in, the focus on the title.
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
            title: Text::single(&task.title),
            status,
            priority,
            due: Text::single(
                &task
                    .due
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
            ),
            project: Text::single(
                &task
                    .project
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            tags: Text::single(
                &task
                    .tags
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            description: Text::multi(task.description.as_deref().unwrap_or_default()),
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

    /// What a choice row shows: the chosen status (`none` for no status)
    /// or priority; a text row's text (see [`Form::text`] for its cursor).
    pub fn value(&self, field: Field, workflow: &Workflow) -> String {
        match field {
            Field::Status => self
                .chosen_status(workflow)
                .map_or_else(|| "none".to_owned(), |s| s.as_str().to_owned()),
            Field::Priority => self.chosen_priority().as_str().to_owned(),
            text => self.text(text).map(Text::text).unwrap_or_default(),
        }
    }

    /// The text of a text row.
    pub fn text(&self, field: Field) -> Option<&Text> {
        match field {
            Field::Title => Some(&self.title),
            Field::Due => Some(&self.due),
            Field::Project => Some(&self.project),
            Field::Tags => Some(&self.tags),
            Field::Description => Some(&self.description),
            Field::Status | Field::Priority => None,
        }
    }

    fn focused_text(&mut self) -> Option<&mut Text> {
        match self.focus {
            Field::Title => Some(&mut self.title),
            Field::Due => Some(&mut self.due),
            Field::Project => Some(&mut self.project),
            Field::Tags => Some(&mut self.tags),
            Field::Description => Some(&mut self.description),
            Field::Status | Field::Priority => None,
        }
    }

    /// Types `c` into the focused text row; a newline only in the
    /// description.
    pub fn insert(&mut self, c: char) {
        if c == '\n' && self.focus != Field::Description {
            return;
        }
        if let Some(text) = self.focused_text() {
            text.insert(c);
        }
    }

    /// Splits the description at the cursor (`Enter` there).
    pub fn newline(&mut self) {
        self.insert('\n');
    }

    /// Pastes into the focused text row: line breaks stay in the
    /// description and become spaces elsewhere.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = if self.focus == Field::Description {
            text
        } else {
            crate::update::one_line(&text)
        };
        if let Some(field) = self.focused_text() {
            field.paste(&text);
        }
    }

    /// `Backspace` in the focused text row.
    pub fn backspace(&mut self) {
        if let Some(text) = self.focused_text() {
            text.backspace();
        }
    }

    /// `Delete` in the focused text row.
    pub fn delete(&mut self) {
        if let Some(text) = self.focused_text() {
            text.delete();
        }
    }

    /// `Home` in the focused text row.
    pub fn home(&mut self) {
        if let Some(text) = self.focused_text() {
            text.home();
        }
    }

    /// `End` in the focused text row.
    pub fn end(&mut self) {
        if let Some(text) = self.focused_text() {
            text.end();
        }
    }

    /// Moves the focus down (`Tab`).
    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// Moves the focus up (`Shift+Tab`).
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// `Up`: the cursor up a line where there is one, else the focus to
    /// the row above.
    pub fn up(&mut self) {
        if !self.focused_text().is_some_and(Text::up) {
            self.focus_prev();
        }
    }

    /// `Down`: the cursor down a line where there is one, else the focus
    /// to the row below.
    pub fn down(&mut self) {
        if !self.focused_text().is_some_and(Text::down) {
            self.focus_next();
        }
    }

    /// `Left`: the cursor back in a text row, the previous choice in a
    /// choice row.
    pub fn left(&mut self, workflow: &Workflow) {
        match self.focused_text() {
            Some(text) => {
                text.left();
            }
            None => self.cycle(-1, workflow),
        }
    }

    /// `Right`: the cursor forward in a text row, the next choice in a
    /// choice row.
    pub fn right(&mut self, workflow: &Workflow) {
        match self.focused_text() {
            Some(text) => {
                text.right();
            }
            None => self.cycle(1, workflow),
        }
    }

    /// Cycles the focused choice row by `delta`, wrapping around; nothing
    /// on a text row.
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
    /// resolved against `today`, the project as a path, the tags parsed,
    /// the description without blank lines at either end (none when
    /// nothing is left). The first bad row is reported with its message.
    pub fn fields(&self, workflow: &Workflow, today: NaiveDate) -> Result<Fields, FormError> {
        let title = self.title.text();
        let title = title.trim();
        if title.is_empty() {
            return Err(FormError {
                field: Field::Title,
                message: "the title must not be empty".to_owned(),
            });
        }
        let due = self.due.text();
        let due = match due.trim() {
            "" => None,
            text => Some(parse_day(text, today).map_err(|e| FormError {
                field: Field::Due,
                message: format!("due: {e}"),
            })?),
        };
        let project = self.project.text();
        let project = match project.trim() {
            "" => None,
            text => Some(PathBuf::from(text)),
        };
        let mut tags: Vec<Tag> = Vec::new();
        for word in self.tags.text().split_whitespace() {
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
            description: trimmed_description(self.description.lines()),
            status: self.chosen_status(workflow),
            priority: self.chosen_priority(),
            due,
            project,
            tags,
        })
    }
}

/// The description as the file reads it back: blank lines dropped at
/// both ends, `None` when nothing is left.
fn trimmed_description(lines: &[String]) -> Option<String> {
    let blank = |l: &String| l.trim().is_empty();
    let start = lines.iter().position(|l| !blank(l))?;
    let end = lines.iter().rposition(|l| !blank(l)).unwrap_or(start) + 1;
    Some(lines[start..end].join("\n"))
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
        t.description = Some("First line.\n\nThird line.".into());
        t.add_tag(Tag::new("gitlab").unwrap());
        t.add_tag(Tag::new("x").unwrap());
        t
    }

    #[test]
    fn fields_have_labels_kinds_and_an_order() {
        assert_eq!(
            Field::ALL.map(Field::label),
            [
                "Title",
                "Status",
                "Priority",
                "Due",
                "Project",
                "Tags",
                "Description"
            ]
        );
        assert_eq!(
            Field::ALL.map(Field::is_text),
            [true, false, false, true, true, true, true]
        );
        assert_eq!(Field::Title.prev(), Field::Title);
        assert_eq!(Field::Title.next(), Field::Status);
        assert_eq!(Field::Description.next(), Field::Description);
        assert_eq!(Field::Description.prev(), Field::Tags);
        assert_eq!(Field::Due.index(), 3);
    }

    #[test]
    fn text_editing() {
        let mut t = Text::single("héllo");
        assert_eq!(t.cursor(), (0, 5));
        assert_eq!(t.lines(), ["héllo"]);
        t.insert('!');
        assert_eq!(t.text(), "héllo!");
        t.home();
        t.insert('>');
        assert_eq!(t.text(), ">héllo!");
        assert_eq!(t.cursor(), (0, 1));
        assert!(t.right());
        assert!(t.right());
        t.insert('_');
        assert_eq!(t.text(), ">hé_llo!", "inserting after a two-byte char");
        t.backspace();
        t.backspace();
        assert_eq!(t.text(), ">hllo!");
        assert_eq!(t.cursor(), (0, 2));
        t.delete();
        assert_eq!(t.text(), ">hlo!");
        t.end();
        assert_eq!(t.cursor(), (0, 5));
        t.delete();
        assert_eq!(t.text(), ">hlo!", "delete at the very end does nothing");
        assert!(!t.right(), "right at the very end");
        assert!(!t.up());
        assert!(!t.down());
        t.home();
        assert!(!t.left(), "left at the very start");
        t.backspace();
        assert_eq!(
            t.text(),
            ">hlo!",
            "backspace at the very start does nothing"
        );
    }

    #[test]
    fn multiline_editing() {
        let mut t = Text::multi("ab\n\ncd");
        assert_eq!(t.lines(), ["ab", "", "cd"]);
        assert_eq!(t.cursor(), (0, 0));
        t.end();
        t.newline();
        assert_eq!(t.lines(), ["ab", "", "", "cd"]);
        assert_eq!(t.cursor(), (1, 0));
        t.backspace();
        assert_eq!(
            t.lines(),
            ["ab", "", "cd"],
            "backspace joins to the line above"
        );
        assert_eq!(t.cursor(), (0, 2));
        t.delete();
        assert_eq!(
            t.lines(),
            ["ab", "cd"],
            "delete at the end joins the next line"
        );
        assert_eq!(t.cursor(), (0, 2));
        assert!(t.right());
        assert_eq!(t.cursor(), (1, 0), "right wraps to the next line");
        assert!(t.left());
        assert_eq!(t.cursor(), (0, 2), "left wraps to the previous line");
        t.home();
        t.insert('x');
        assert!(t.down());
        assert_eq!(t.cursor(), (1, 1));
        t.end();
        assert_eq!(t.cursor(), (1, 2));
        assert!(t.up());
        assert_eq!(t.cursor(), (0, 2));
        assert!(!t.up());
        t.end();
        assert!(t.down());
        assert_eq!(t.cursor(), (1, 2), "column clamped to the shorter line");
        assert!(!t.down());
        t.paste("1\n2");
        assert_eq!(t.lines(), ["xab", "cd1", "2"]);
        assert_eq!(t.cursor(), (2, 1));
        t.up();
        t.up();
        t.end();
        t.delete();
        assert_eq!(t.lines(), ["xabcd1", "2"], "the next line joins this one");
        assert_eq!(t.cursor(), (0, 3));
        let mut cursor_in_middle = Text::multi("abcd");
        cursor_in_middle.right();
        cursor_in_middle.right();
        cursor_in_middle.insert('\n');
        assert_eq!(cursor_in_middle.lines(), ["ab", "cd"]);
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
        assert_eq!(
            form.value(Field::Description, &wf),
            "First line.\n\nThird line."
        );
        assert_eq!(form.text(Field::Status), None);
        assert_eq!(form.text(Field::Priority), None);
        assert_eq!(form.text(Field::Title).unwrap().cursor(), (0, 5));
        assert_eq!(form.text(Field::Description).unwrap().cursor(), (0, 0));
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
        assert_eq!(blank.value(Field::Description, &wf), "");
        let fields = blank.fields(&wf, today()).unwrap();
        assert_eq!(fields, Fields::of(&Task::new(TaskId::from(1), "B")));
        assert_eq!(fields.status, None);
        assert_eq!(fields.description, None);
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
        form.insert('!');
        assert_eq!(form.title.text(), "Seven!");
        form.backspace();
        assert_eq!(form.title.text(), "Seven");
        form.paste(" more\r\nlines\n");
        assert_eq!(
            form.title.text(),
            "Seven more lines",
            "one line outside the description"
        );
        form.insert('\n');
        form.newline();
        assert_eq!(
            form.title.lines().len(),
            1,
            "no newline outside the description"
        );
        form.home();
        form.delete();
        assert_eq!(form.title.text(), "even more lines");
        form.end();
        form.left(&wf);
        form.insert('_');
        assert_eq!(form.title.text(), "even more line_s");
        form.right(&wf);
        form.insert('_');
        assert_eq!(form.title.text(), "even more line_s_");
        form.focus_next();
        assert_eq!(form.focus, Field::Status);
        let before = form.clone();
        form.insert('x');
        form.backspace();
        form.delete();
        form.home();
        form.end();
        form.paste("y");
        assert_eq!(form, before, "a choice row ignores typing");
        form.focus_next();
        form.focus_next();
        assert_eq!(form.focus, Field::Due);
        form.insert('x');
        assert_eq!(form.due.text(), "2026-10-05x");
        form.focus_next();
        form.insert('x');
        assert_eq!(form.project.text(), "/px");
        form.focus_next();
        form.insert('x');
        assert_eq!(form.tags.text(), "gitlab xx");
        form.focus_next();
        assert_eq!(form.focus, Field::Description);
        form.insert('>');
        form.newline();
        form.paste("a\r\nb");
        assert_eq!(
            form.description.lines(),
            [">", "a", "bFirst line.", "", "Third line."]
        );
        form.focus_next();
        assert_eq!(form.focus, Field::Description, "clamped at the bottom");
        for _ in 0..9 {
            form.focus_prev();
        }
        assert_eq!(form.focus, Field::Title, "clamped at the top");
    }

    #[test]
    fn up_and_down_move_the_cursor_in_the_description_and_the_focus_elsewhere() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        form.down();
        assert_eq!(form.focus, Field::Status);
        form.down();
        form.up();
        assert_eq!(form.focus, Field::Status);
        form.up();
        form.up();
        assert_eq!(form.focus, Field::Title, "clamped at the top");
        form.focus = Field::Tags;
        form.down();
        assert_eq!(form.focus, Field::Description);
        assert_eq!(form.description.cursor(), (0, 0));
        form.down();
        assert_eq!(
            form.description.cursor(),
            (1, 0),
            "moves within the description"
        );
        form.down();
        form.down();
        assert_eq!(form.description.cursor(), (2, 0), "stays on the last line");
        assert_eq!(form.focus, Field::Description);
        form.up();
        form.up();
        assert_eq!(form.description.cursor(), (0, 0));
        form.up();
        assert_eq!(form.focus, Field::Tags, "up from the first line leaves");
    }

    #[test]
    fn left_and_right_cycle_choice_rows_and_move_the_cursor_elsewhere() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        form.left(&wf);
        assert_eq!(form.title.cursor(), (0, 4));
        form.right(&wf);
        form.right(&wf);
        assert_eq!(form.title.cursor(), (0, 5), "stays at the end");
        let before = form.clone();
        form.cycle(1, &wf);
        assert_eq!(form, before, "the title row does not cycle");
        form.focus = Field::Priority;
        form.left(&wf);
        assert_eq!(form.chosen_priority(), Priority::C, "wraps backwards");
        form.right(&wf);
        assert_eq!(form.chosen_priority(), Priority::A);
        form.cycle(2, &wf);
        assert_eq!(form.chosen_priority(), Priority::C);
        form.focus = Field::Status;
        let last = wf.statuses.len();
        form.status = last;
        form.right(&wf);
        assert_eq!(form.status, 0, "wraps forwards");
        form.left(&wf);
        assert_eq!(form.status, last);
        assert_eq!(form.chosen_status(&wf), None);
    }

    #[test]
    fn fields_are_validated_row_by_row() {
        let wf = Workflow::default();
        let mut form = Form::of(&task(), &wf);
        form.title = Text::single(" \t");
        assert_eq!(
            form.fields(&wf, today()),
            Err(FormError {
                field: Field::Title,
                message: "the title must not be empty".into()
            })
        );
        form.title = Text::single("  Trimmed  ");
        form.due = Text::single("soon");
        let err = form.fields(&wf, today()).unwrap_err();
        assert_eq!(err.field, Field::Due);
        assert!(err.message.starts_with("due: "), "{}", err.message);
        form.due = Text::single(" tomorrow ");
        form.tags = Text::single("ok ##bad");
        let err = form.fields(&wf, today()).unwrap_err();
        assert_eq!(err.field, Field::Tags);
        assert!(err.message.starts_with("tags: "), "{}", err.message);
        form.tags = Text::single(" #a b #a ");
        form.project = Text::single("  ");
        form.description = Text::multi("\n \nkept\n\nalso kept\n\t\n");
        let fields = form.fields(&wf, today()).unwrap();
        assert_eq!(fields.title, "Trimmed");
        assert_eq!(fields.due, Some(today() + chrono::Duration::days(1)));
        assert_eq!(fields.project, None);
        assert_eq!(
            fields.tags,
            vec![Tag::new("a").unwrap(), Tag::new("b").unwrap()]
        );
        assert_eq!(fields.description, Some("kept\n\nalso kept".into()));
        form.due = Text::single("");
        form.project = Text::single(" /q ");
        form.description = Text::multi(" \n\n");
        let fields = form.fields(&wf, today()).unwrap();
        assert_eq!(fields.due, None);
        assert_eq!(fields.project, Some(PathBuf::from("/q")));
        assert_eq!(fields.description, None);
        assert_eq!(trimmed_description(&["x".to_owned()]), Some("x".into()));
    }
}
