//! The small edits of the original script, `set`, `log` and `done`, as
//! operations on a [`Store`].
//!
//! The CLI (`tasq set/log/done`) and the TUI (`s`, `p`, `l`, `d`) both call
//! these, so the two front ends cannot drift (FR-10): a status change is
//! always "read the task, change the field, optionally log the note, write
//! the whole task back", and `done` is always "log the note first, then
//! close with the store's own `done` semantics".

use thiserror::Error;

use crate::clock::Clock;
use crate::model::{Priority, Status, Task, TaskId, Workflow};
use crate::store::{Store, StoreError};

/// What `set` changes: a workflow status or a priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A workflow status.
    Status(Status),
    /// `A`, `B` or `C`.
    Priority(Priority),
}

impl Value {
    /// Interprets the user's word: a priority letter (with or without `#`)
    /// first, then a status of `workflow` (with or without `#`). `None`
    /// when it is neither.
    pub fn parse(text: &str, workflow: &Workflow) -> Option<Self> {
        if let Ok(priority) = text.parse::<Priority>() {
            return Some(Self::Priority(priority));
        }
        workflow.parse_status(text).map(Self::Status)
    }

    /// The label the script printed after `->`: the status name, or
    /// `priority #A`.
    pub fn label(&self) -> String {
        match self {
            Self::Status(status) => status.to_string(),
            Self::Priority(priority) => format!("priority {}", priority.to_hash()),
        }
    }

    /// Changes the field on `task`.
    pub fn apply(&self, task: &mut Task) {
        match self {
            Self::Status(status) => task.set_status(status.clone()),
            Self::Priority(priority) => task.set_priority(*priority),
        }
    }
}

/// Why an edit was refused.
#[derive(Debug, Error)]
pub enum EditError {
    /// A note was given but holds no text.
    #[error("the note must not be empty")]
    EmptyNote,
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `tasq set <id> <value> [note]`: changes the status or priority, logs
/// `note` when given, writes the task and returns it as stored.
pub fn set(
    store: &mut dyn Store,
    id: &TaskId,
    value: &Value,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?;
    let mut task = store.get(id)?;
    value.apply(&mut task);
    if let Some(note) = note {
        task.log(note, clock);
    }
    store.update(&task)?;
    Ok(store.get(id)?)
}

/// `tasq log <id> <note>`: appends a dated progress note and returns the
/// task as stored.
pub fn log(
    store: &mut dyn Store,
    id: &TaskId,
    note: &str,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(Some(note))?.ok_or(EditError::EmptyNote)?;
    let mut task = store.get(id)?;
    task.log(note, clock);
    store.update(&task)?;
    Ok(store.get(id)?)
}

/// `tasq done <id> [note]`: logs `note` when given, then closes the task
/// (`# [x]`, status tag removed) and returns it as stored.
pub fn done(
    store: &mut dyn Store,
    id: &TaskId,
    note: Option<&str>,
    clock: &dyn Clock,
) -> Result<Task, EditError> {
    let note = checked_note(note)?;
    let mut task = store.get(id)?;
    if let Some(note) = note {
        task.log(note, clock);
        store.update(&task)?;
    }
    store.set_done(id, true)?;
    Ok(store.get(id)?)
}

/// A given note must hold text; `None` passes through.
fn checked_note(note: Option<&str>) -> Result<Option<&str>, EditError> {
    match note {
        Some(text) if text.trim().is_empty() => Err(EditError::EmptyNote),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::ProgressEntry;
    use crate::store::MemoryStore;

    fn store() -> MemoryStore {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        let b = Task::new(TaskId::from(2), "B");
        MemoryStore::new([a, b])
    }

    fn clock() -> FixedClock {
        FixedClock::at("2026-10-04 10:15")
    }

    fn entry(note: &str) -> ProgressEntry {
        ProgressEntry::new(clock().0, note)
    }

    #[test]
    fn value_parsing_prefers_priority_then_status() {
        let wf = Workflow::default();
        assert_eq!(Value::parse("A", &wf), Some(Value::Priority(Priority::A)));
        assert_eq!(Value::parse("#C", &wf), Some(Value::Priority(Priority::C)));
        assert_eq!(
            Value::parse("#ready", &wf),
            Some(Value::Status(Status::READY))
        );
        assert_eq!(
            Value::parse("blocked", &wf),
            Some(Value::Status(Status::BLOCKED))
        );
        assert_eq!(Value::parse("nope", &wf), None);
        // Lowercase letters are tags in the file, so they are not priorities.
        assert_eq!(Value::parse("a", &wf), None);
    }

    #[test]
    fn value_labels_and_apply() {
        assert_eq!(Value::Status(Status::BLOCKED).label(), "blocked");
        assert_eq!(Value::Priority(Priority::A).label(), "priority #A");
        let mut task = Task::new(TaskId::from(1), "x");
        Value::Status(Status::WAITING).apply(&mut task);
        assert_eq!(task.status, Some(Status::WAITING));
        assert_eq!(task.priority, Priority::B);
        Value::Priority(Priority::C).apply(&mut task);
        assert_eq!(task.priority, Priority::C);
        assert_eq!(task.status, Some(Status::WAITING));
    }

    #[test]
    fn set_status_with_and_without_note() {
        let mut store = store();
        let id = TaskId::from(1);
        let task = set(
            &mut store,
            &id,
            &Value::Status(Status::IN_PROGRESS),
            None,
            &clock(),
        )
        .unwrap();
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, Vec::new());
        assert_eq!(store.get(&id).unwrap(), task);

        let task = set(
            &mut store,
            &id,
            &Value::Priority(Priority::A),
            Some("urgent"),
            &clock(),
        )
        .unwrap();
        assert_eq!(task.priority, Priority::A);
        assert_eq!(task.status, Some(Status::IN_PROGRESS));
        assert_eq!(task.progress, vec![entry("urgent")]);
        assert_eq!(store.get(&id).unwrap(), task);
    }

    #[test]
    fn set_refuses_an_empty_note_before_writing() {
        let mut store = store();
        let before = store.get(&TaskId::from(1)).unwrap();
        let err = set(
            &mut store,
            &TaskId::from(1),
            &Value::Status(Status::LATER),
            Some("  "),
            &clock(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "the note must not be empty");
        assert_eq!(store.get(&TaskId::from(1)).unwrap(), before);
    }

    #[test]
    fn set_on_a_missing_task_is_not_found() {
        let mut store = store();
        let err = set(
            &mut store,
            &TaskId::from(9),
            &Value::Priority(Priority::A),
            None,
            &clock(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
        assert!(matches!(err, EditError::Store(StoreError::NotFound(_))));
    }

    #[test]
    fn log_appends_a_dated_note() {
        let mut store = store();
        let id = TaskId::from(2);
        let task = log(&mut store, &id, "found the cause", &clock()).unwrap();
        assert_eq!(task.progress, vec![entry("found the cause")]);
        let task = log(&mut store, &id, "fixed", &clock()).unwrap();
        assert_eq!(
            task.progress,
            vec![entry("found the cause"), entry("fixed")]
        );
        assert_eq!(store.get(&id).unwrap(), task);
        assert_eq!(
            log(&mut store, &id, " \n", &clock())
                .unwrap_err()
                .to_string(),
            "the note must not be empty"
        );
        assert_eq!(store.get(&id).unwrap().progress.len(), 2);
        assert_eq!(
            log(&mut store, &TaskId::from(9), "x", &clock())
                .unwrap_err()
                .to_string(),
            "no task with id 9"
        );
    }

    #[test]
    fn done_logs_then_closes() {
        let mut store = store();
        let id = TaskId::from(1);
        let task = done(&mut store, &id, Some("merged"), &clock()).unwrap();
        assert!(task.done);
        assert_eq!(task.status, None);
        assert_eq!(task.progress, vec![entry("merged")]);
        assert_eq!(store.get(&id).unwrap(), task);

        let task = done(&mut store, &TaskId::from(2), None, &clock()).unwrap();
        assert!(task.done);
        assert_eq!(task.progress, Vec::new());
    }

    #[test]
    fn done_refuses_an_empty_note_and_missing_tasks() {
        let mut store = store();
        let err = done(&mut store, &TaskId::from(1), Some(""), &clock()).unwrap_err();
        assert_eq!(err.to_string(), "the note must not be empty");
        assert!(!store.get(&TaskId::from(1)).unwrap().done);
        let err = done(&mut store, &TaskId::from(9), None, &clock()).unwrap_err();
        assert_eq!(err.to_string(), "no task with id 9");
    }

    #[test]
    fn checked_note_passes_text_through() {
        assert_eq!(checked_note(None).unwrap(), None);
        assert_eq!(checked_note(Some("x")).unwrap(), Some("x"));
        assert!(checked_note(Some("\t")).is_err());
    }
}
