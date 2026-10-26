//! The tasks a command wrote, for the `post-write` event (ADR 0030).
//!
//! [`App::open_store`](crate::app::App::open_store) wraps the store in a
//! [`Recording`] that forwards every call and notes each task a successful
//! `create`, `update`, `update_many` or `set_done` actually changed on disk:
//! the task's file is read before and after, so a write the store skipped
//! (the task already as asked) is no write. The CLI sends the notes to the
//! plugins once the command is over; the store crate and core never hear
//! of plugins.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use tasq_core::doctor::Check;
use tasq_core::model::{Task, TaskDraft, TaskId};
use tasq_core::plugin::{Write, WriteKind};
use tasq_core::query::Filter;
use tasq_core::store::{BatchError, Store, StoreError, StoreInfo};

/// Every task written so far, once each, in the order first written;
/// shared by the [`Recording`] stores of one command and whoever reports
/// them.
#[derive(Debug, Clone, Default)]
pub struct Written(Rc<RefCell<Vec<Write>>>);

impl Written {
    /// Hands over, and clears, the writes noted so far.
    pub fn take(&self) -> Vec<Write> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    /// Notes `write`, unless its task was written before: a task created
    /// and then updated by the same command stays a create.
    pub fn note(&self, write: Write) {
        let mut writes = self.0.borrow_mut();
        if writes.iter().all(|w| w.id != write.id) {
            writes.push(write);
        }
    }
}

/// A task's file and its bytes, as seen before a write.
#[derive(Debug)]
struct Snapshot {
    file: PathBuf,
    bytes: Option<Vec<u8>>,
}

/// A [`Store`] that notes the tasks written through it in a [`Written`].
pub struct Recording {
    inner: Box<dyn Store>,
    written: Written,
}

impl Recording {
    /// Wraps `inner`, noting its writes in `written`.
    pub fn new(inner: Box<dyn Store>, written: Written) -> Self {
        Self { inner, written }
    }

    /// The file of `id` and what it holds now; `None` when the store has
    /// no such task or no file for it.
    fn snapshot(&self, id: &TaskId) -> Option<Snapshot> {
        let file = self.inner.file_of(id).ok()??;
        let bytes = std::fs::read(&file).ok();
        Some(Snapshot { file, bytes })
    }

    /// Notes `id` as updated when its file no longer holds the bytes of
    /// `before`. A write keeps the file's name, so no scan is needed; a
    /// task that had no file before was not written by a successful call.
    fn note_update(&self, id: &TaskId, before: Option<&Snapshot>) {
        let Some(before) = before else {
            return;
        };
        if std::fs::read(&before.file).ok() != before.bytes {
            self.written.note(Write {
                id: id.clone(),
                file: before.file.clone(),
                kind: WriteKind::Update,
            });
        }
    }
}

impl Store for Recording {
    fn list(&self, filter: &Filter) -> Result<Vec<Task>, StoreError> {
        self.inner.list(filter)
    }

    fn get(&self, id: &TaskId) -> Result<Task, StoreError> {
        self.inner.get(id)
    }

    fn create(&mut self, draft: TaskDraft) -> Result<Task, StoreError> {
        let task = self.inner.create(draft)?;
        if let Ok(Some(file)) = self.inner.file_of(&task.id) {
            self.written.note(Write {
                id: task.id.clone(),
                file,
                kind: WriteKind::Create,
            });
        }
        Ok(task)
    }

    fn update(&mut self, task: &Task) -> Result<(), StoreError> {
        let before = self.snapshot(&task.id);
        self.inner.update(task)?;
        self.note_update(&task.id, before.as_ref());
        Ok(())
    }

    /// Notes the tasks written before a failure too: the batch stops at
    /// the first error, and the ones before it are on disk.
    fn update_many(&mut self, tasks: &[Task]) -> Result<(), BatchError> {
        let before: Vec<Option<Snapshot>> = tasks.iter().map(|t| self.snapshot(&t.id)).collect();
        let result = self.inner.update_many(tasks);
        let done = match &result {
            Ok(()) => tasks.len(),
            Err(e) => e.written.len(),
        };
        for (task, before) in tasks.iter().zip(&before).take(done) {
            self.note_update(&task.id, before.as_ref());
        }
        result
    }

    fn set_done(&mut self, id: &TaskId, done: bool) -> Result<(), StoreError> {
        let before = self.snapshot(id);
        self.inner.set_done(id, done)?;
        self.note_update(id, before.as_ref());
        Ok(())
    }

    fn describe(&self) -> StoreInfo {
        self.inner.describe()
    }

    fn take_warnings(&mut self) -> Vec<String> {
        self.inner.take_warnings()
    }

    fn doctor(&self) -> Vec<Check> {
        self.inner.doctor()
    }

    fn file_of(&self, id: &TaskId) -> Result<Option<PathBuf>, StoreError> {
        self.inner.file_of(id)
    }
}

#[cfg(test)]
mod tests {
    use tasq_core::model::Workflow;
    use tasq_core::store::MemoryStore;
    use tasq_store::TasqStore;

    use super::*;

    /// No command closes through `set_done`, so the decorator is checked on
    /// its own here: a close is noted, a close of a closed task is not.
    #[test]
    fn set_done_notes_the_task_only_when_its_file_changed() {
        let dir = tempfile::tempdir().unwrap();
        let written = Written::default();
        let mut store = Recording::new(
            Box::new(TasqStore::new(dir.path().join("s"), Workflow::default())),
            written.clone(),
        );
        let id = store.create(TaskDraft::new("One")).unwrap().id;
        let file = store.file_of(&id).unwrap().unwrap();
        assert_eq!(
            written.take(),
            vec![Write {
                id: id.clone(),
                file: file.clone(),
                kind: WriteKind::Create,
            }]
        );
        store.set_done(&id, true).unwrap();
        assert_eq!(
            written.take(),
            vec![Write {
                id: id.clone(),
                file,
                kind: WriteKind::Update,
            }]
        );
        store.set_done(&id, true).unwrap();
        assert_eq!(written.take(), Vec::new(), "already done: nothing written");
        assert_eq!(
            store
                .set_done(&TaskId::from(9), true)
                .unwrap_err()
                .to_string(),
            "no task with id 9"
        );
        assert_eq!(written.take(), Vec::new());
    }

    #[test]
    fn warnings_and_doctor_are_the_inner_store_s() {
        let mut inner = MemoryStore::new([Task::new(TaskId::from(1), "One")]);
        inner.warn("careful");
        let mut store = Recording::new(Box::new(inner), Written::default());
        assert_eq!(store.take_warnings(), vec!["careful".to_owned()]);
        assert_eq!(
            store.doctor(),
            vec![Check::ok("memory", "1 task(s) held in memory")]
        );
    }
}
