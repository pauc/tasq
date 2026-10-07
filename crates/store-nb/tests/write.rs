//! T-202: `update` and `set_done` on a temp copy of the fixture.

mod support;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tasq_core::clock::{Clock, FixedClock};
use tasq_core::model::{
    Link, Priority, ProgressEntry, Session, Status, TaskId, Workflow, Worktree,
};
use tasq_core::query::Filter;
use tasq_store_nb::{
    Bookkeeper, NbStore, NbStoreOptions, Store, StoreError, StoreWarning, SyncOutcome, Verification,
};

use support::{NbEnv, file_name, id};

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

/// Everything in the notebook except `changed` keeps its modification time.
fn assert_only_changed(before: &[(String, std::time::SystemTime)], nb: &NbEnv, changed: &str) {
    let after = nb.mtimes();
    assert_eq!(before.len(), after.len(), "no file appeared or vanished");
    for ((name, was), (_, now)) in before.iter().zip(&after) {
        if name != changed {
            assert_eq!(was, now, "{name} was rewritten");
        }
    }
    let leftovers: Vec<&String> = after
        .iter()
        .map(|(n, _)| n)
        .filter(|n| n.starts_with(".tasq-"))
        .collect();
    assert_eq!(leftovers, Vec::<&String>::new(), "temp files left behind");
}

#[test]
fn update_sets_status_and_priority_in_place() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let before = nb.mtimes();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.set_status(Status::BLOCKED);
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    assert_eq!(
        nb.read(file_name(id::SUPPORT)),
        "# [ ] Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n## Tags\n\n#support #A #blocked\n\n## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
    );
    assert_only_changed(&before, &nb, file_name(id::SUPPORT));
    assert_eq!(store.get(&TaskId::from(id::SUPPORT)).unwrap(), task);
    assert_eq!(store.warnings(), []);
}

#[test]
fn update_appends_progress_lists_and_project() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.progress
        .push(ProgressEntry::new(at("2026-10-05 09:00"), "writer done"));
    task.add_worktree(Worktree::new("/home/pau/code/tasks-wt/detached"));
    task.add_session(Session {
        at: at("2026-10-05 09:05"),
        id: "def-456".into(),
        launcher: None,
        description: None,
    });
    task.add_merge_request(Link::labelled(
        "https://gitlab.example.invalid/group/project/-/merge_requests/124",
        "Add writer",
    ));
    task.add_related(Link::new("https://example.invalid/docs/spec"));
    task.project = Some(PathBuf::from("/home/pau/code/tasks-wt/feature-a"));
    store.update(&task).unwrap();
    assert_eq!(store.get(&task.id).unwrap(), task);
    let text = nb.read(file_name(id::FULL));
    assert!(
        text.contains("## Project\n\n/home/pau/code/tasks-wt/feature-a\n\n## Due"),
        "{text}"
    );
    assert!(text.contains("- https://gitlab.example.invalid/group/project/-/issues/42\n- https://example.invalid/docs/spec\n\n### Merge requests\n\n- [Add parser](https://gitlab.example.invalid/group/project/-/merge_requests/123)\n- [Add writer](https://gitlab.example.invalid/group/project/-/merge_requests/124)\n"), "{text}");
    assert!(
        text.ends_with("- 2026-10-04 11:40: parser done\n- 2026-10-05 09:00: writer done\n"),
        "{text}"
    );
    assert!(text.contains("- /home/pau/code/tasks-wt/feature-a (`feature-a`)\n- /home/pau/code/tasks-wt/detached\n"), "{text}");
    assert!(
        text.contains(
            "- 2026-10-04 10:20: `abc-123` — first session\n- 2026-10-05 09:05: `def-456`\n"
        ),
        "{text}"
    );
}

#[test]
fn unchanged_update_writes_nothing() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let before = nb.mtimes();
    let task = store.get(&TaskId::from(id::FULL)).unwrap();
    let text = nb.read(file_name(id::FULL));
    store.update(&task).unwrap();
    assert_eq!(nb.mtimes(), before);
    assert_eq!(nb.read(file_name(id::FULL)), text);
}

#[test]
fn sequential_updates_by_the_same_reader_succeed() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::WAITING)).unwrap();
    task.set_status(Status::READY);
    store.update(&task).unwrap();
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    task.progress
        .push(ProgressEntry::new(at("2026-10-05 09:00"), "unblocked"));
    store.update(&task).unwrap();
    assert_eq!(store.get(&task.id).unwrap(), task);
    assert!(nb.read(file_name(id::WAITING)).contains("#ready #A\n"));
}

#[test]
fn update_detects_a_concurrent_change() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    // Somebody else edits the file in between (same length, so only the
    // hash or mtime can tell).
    let text = nb.read(file_name(id::SUPPORT));
    nb.write(
        file_name(id::SUPPORT),
        &text.replace("#support", "#SUPPORT"),
    );
    task.set_status(Status::BLOCKED);
    let err = store.update(&task).unwrap_err();
    assert!(
        matches!(&err, StoreError::Conflict { id, path } if *id == task.id && *path == nb.file(id::SUPPORT)),
        "{err:?}"
    );
    assert!(
        nb.read(file_name(id::SUPPORT)).contains("#SUPPORT"),
        "nothing written"
    );
    // Still a conflict until the task is re-read.
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::Conflict { .. }
    ));
    let fresh = store.get(&task.id).unwrap();
    let mut fresh_edit = fresh.clone();
    fresh_edit.set_status(Status::BLOCKED);
    store.update(&fresh_edit).unwrap();
    assert_eq!(store.get(&task.id).unwrap(), fresh_edit);
}

#[test]
fn update_detects_a_change_seen_through_list() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store
        .list(&Filter::default())
        .unwrap()
        .into_iter()
        .find(|t| t.id == TaskId::from(id::NO_TAGS))
        .unwrap();
    nb.write(file_name(id::NO_TAGS), "# [ ] Task without a tags section\n\n## Progress\n\n- 2026-10-06 14:00: created via tasks create\n- 2026-10-06 15:00: edited elsewhere\n");
    task.set_status(Status::LATER);
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::Conflict { .. }
    ));
}

#[test]
fn update_without_a_prior_read_in_this_store_is_not_checked() {
    let nb = NbEnv::fixture();
    let reader = nb.open();
    let mut writer = nb.open();
    let mut task = reader.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.set_status(Status::LATER);
    writer.update(&task).unwrap();
    assert!(
        nb.read(file_name(id::SUPPORT))
            .contains("#support #B #later\n")
    );
}

#[test]
fn update_refuses_changes_it_cannot_express() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let before = nb.mtimes();
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.related.clear();
    task.progress.clear();
    let err = store.update(&task).unwrap_err();
    let StoreError::Unsupported { operation } = &err else {
        panic!("{err:?}")
    };
    assert!(
        operation.starts_with("changing related, progress of task 1 "),
        "{operation}"
    );
    assert_eq!(nb.mtimes(), before, "nothing written");
}

#[test]
fn update_rewrites_the_form_fields() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.title = "Renamed".into();
    task.description = Some("Why.\n\nHow.".into());
    task.tags.clear();
    task.due = None;
    task.project = None;
    store.update(&task).unwrap();
    assert_eq!(store.get(&TaskId::from(id::FULL)).unwrap(), task);
    let text = nb.read(file_name(id::FULL));
    assert!(
        text.starts_with("# [ ] Renamed\n\n## Description\n\nWhy.\n\nHow.\n\n## "),
        "{text}"
    );
    assert!(!text.contains("## Due"), "{text}");
    assert!(!text.contains("## Project"), "{text}");
    assert!(text.contains("\n#A #in-progress\n"), "{text}");
}

#[test]
fn update_of_an_unknown_id_is_not_found() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let task = tasq_core::model::Task::new(TaskId::from(id::MISSING), "ghost");
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::NotFound(_)
    ));
    let task = tasq_core::model::Task::new(TaskId::from(id::NOTE), "note");
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::NotFound(_)
    ));
}

#[test]
#[cfg(unix)]
fn update_keeps_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let nb = NbEnv::fixture();
    let path = nb.file(id::SUPPORT);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.set_priority(Priority::C);
    store.update(&task).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn set_done_writes_what_nb_todo_do_writes_plus_the_status_strip() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let before = nb.mtimes();
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    assert_eq!(
        nb.read(file_name(id::SUPPORT)),
        "# [x] Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n## Tags\n\n#support #B\n\n## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
    );
    assert_only_changed(&before, &nb, file_name(id::SUPPORT));
    let task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    assert!(task.done);
    assert_eq!(task.status, None);
    assert_eq!(task.priority, Priority::B);

    // A task whose tags line is only the status loses the line, not the heading.
    store.set_done(&TaskId::from(id::WAITING), true).unwrap();
    assert_eq!(
        nb.read(file_name(id::WAITING)),
        "# [x] Wait for the security review\n\n## Tags\n\n\n## Progress\n\n- 2026-10-04 12:00: created via tasks create\n"
    );
    // No tags section: only the title changes.
    store.set_done(&TaskId::from(id::NO_TAGS), true).unwrap();
    assert_eq!(
        nb.read(file_name(id::NO_TAGS)),
        "# [x] Task without a tags section\n\n## Progress\n\n- 2026-10-06 14:00: created via tasks create\n"
    );
    assert_eq!(store.list(&Filter::default()).unwrap().len(), 1);
}

#[test]
fn set_done_is_idempotent_and_reopens() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let before = nb.mtimes();
    store.set_done(&TaskId::from(id::DONE), true).unwrap();
    assert_eq!(nb.mtimes(), before, "already done: nothing written");
    store.set_done(&TaskId::from(id::FULL), false).unwrap();
    assert_eq!(nb.mtimes(), before, "already open: nothing written");

    store.set_done(&TaskId::from(id::DONE), false).unwrap();
    let text = nb.read(file_name(id::DONE));
    assert!(
        text.starts_with("# [ ] Ship the release notes\n\n## Tags\n\n#gitlab #C\n"),
        "{text}"
    );
    let task = store.get(&TaskId::from(id::DONE)).unwrap();
    assert!(!task.done);
    assert_eq!(task.status, None, "no status is restored");
}

#[test]
fn closing_through_update_records_the_time_and_set_done_false_drops_it() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.close(&FixedClock::at("2026-10-07 14:32"));
    store.update(&task).unwrap();
    assert_eq!(
        nb.read(file_name(id::SUPPORT)),
        "# [x] Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n## Closed\n\n2026-10-07 14:32\n\n## Tags\n\n#support #B\n\n## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
    );
    assert_eq!(store.get(&TaskId::from(id::SUPPORT)).unwrap(), task);

    store.set_done(&TaskId::from(id::SUPPORT), false).unwrap();
    assert_eq!(
        nb.read(file_name(id::SUPPORT)),
        "# [ ] Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n## Tags\n\n#support #B\n\n## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
    );
}

#[test]
fn set_done_invalidates_tasks_read_before_it() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    store.set_done(&task.id, true).unwrap();
    task.set_status(Status::BLOCKED);
    // The caller's task predates `set_done`; without a baseline the write is
    // judged on content, and reopening with a status is expressible.
    store.update(&task).unwrap();
    let text = nb.read(file_name(id::SUPPORT));
    assert!(
        text.starts_with("# [ ] Answer the support ticket\n"),
        "{text}"
    );
    assert!(text.contains("#support #B #blocked\n"), "{text}");
}

#[test]
fn set_done_of_unknown_ids_is_not_found() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    for id in [id::NOTE, id::MISSING, 9] {
        assert!(matches!(
            store.set_done(&TaskId::from(id), true).unwrap_err(),
            StoreError::NotFound(_)
        ));
    }
}

/// A bookkeeper that records its calls in a shared log and fails on demand.
struct Recording {
    calls: Rc<RefCell<Vec<String>>>,
    fail: bool,
}

impl Recording {
    fn attach(store: NbStore, fail: bool) -> (NbStore, Rc<RefCell<Vec<String>>>) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let store = store.with_bookkeeper(Box::new(Recording {
            calls: Rc::clone(&calls),
            fail,
        }));
        (store, calls)
    }
}

impl Bookkeeper for Recording {
    fn name(&self) -> &'static str {
        "recording"
    }
    fn register(&self, file: &Path) -> Result<(), StoreError> {
        self.calls
            .borrow_mut()
            .push(format!("register {}", file.display()));
        Ok(())
    }
    fn checkpoint(&self, message: &str) -> Result<bool, StoreError> {
        self.calls
            .borrow_mut()
            .push(format!("checkpoint {message}"));
        if self.fail {
            Err(StoreError::Bookkeeping {
                message: "nb git checkpoint failed; run 'nb git checkpoint' by hand".into(),
            })
        } else {
            Ok(true)
        }
    }
    fn verify(&self) -> Result<Verification, StoreError> {
        Ok(Verification::consistent("recorded"))
    }
    fn sync(&self) -> Result<SyncOutcome, StoreError> {
        Ok(SyncOutcome::skipped("recorded"))
    }
}

#[test]
fn writes_checkpoint_through_the_bookkeeper() {
    let nb = NbEnv::fixture();
    let (mut store, calls) = Recording::attach(nb.open(), false);
    assert_eq!(store.bookkeeper().name(), "recording");
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    store.update(&task).unwrap();
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    store.set_done(&TaskId::from(id::WAITING), true).unwrap();
    store.set_done(&TaskId::from(id::DONE), false).unwrap();
    store.set_done(&TaskId::from(id::FULL), false).unwrap();
    let debug = format!("{store:?}");
    assert!(debug.contains("bookkeeper: \"recording\""), "{debug}");
    // Only real writes checkpoint: the no-op update and no-op reopen do not.
    assert_eq!(
        *calls.borrow(),
        vec![
            format!("checkpoint [tasq] Update: {}", file_name(id::SUPPORT)),
            format!("checkpoint [tasq] Done: {}", file_name(id::WAITING)),
            format!("checkpoint [tasq] Undone: {}", file_name(id::DONE)),
        ]
    );
    assert_eq!(store.warnings(), []);
}

#[test]
fn bookkeeping_failure_after_a_write_is_a_warning() {
    let nb = NbEnv::fixture();
    let (mut store, calls) = Recording::attach(nb.open(), true);
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    assert!(
        nb.read(file_name(id::SUPPORT))
            .contains("#support #ready #A\n"),
        "the write happened"
    );
    assert_eq!(
        store.warnings(),
        &[StoreWarning::Bookkeeping {
            message: "bookkeeping failed after the write: nb git checkpoint failed; run 'nb git checkpoint' by hand".into()
        }]
    );
    store.set_done(&task.id, true).unwrap();
    assert_eq!(store.warnings().len(), 2);
    assert_eq!(calls.borrow().len(), 2);
    assert_eq!(store.take_warnings().len(), 2);
    assert_eq!(store.warnings(), []);
}

#[test]
fn update_and_set_done_follow_the_configured_workflow() {
    let nb = NbEnv::fixture();
    let custom = Status::new("review").unwrap();
    let workflow = Workflow::new(vec![Status::READY, custom.clone()]);
    let options = NbStoreOptions::new(workflow);
    let mut store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    assert_eq!(store.workflow(), &options.workflow);
    assert_ne!(store.workflow(), &Workflow::default());
    // `#in-progress` is a topic tag in this workflow.
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    assert_eq!(task.status, None);
    assert_eq!(task.tags.len(), 2);
    task.set_status(custom);
    store.update(&task).unwrap();
    assert!(
        nb.read(file_name(id::FULL))
            .contains("#gitlab #A #in-progress #review\n")
    );
    store.set_done(&task.id, true).unwrap();
    assert!(
        nb.read(file_name(id::FULL))
            .contains("#gitlab #A #in-progress\n")
    );
}

#[test]
fn update_with_a_clock_that_has_seconds_still_round_trips() {
    // The file keeps minutes; model constructors truncate, so an entry
    // stamped at 10:15:42 reads back as 10:15 and the write succeeds.
    let nb = NbEnv::fixture();
    let mut store = nb.open_with(tasq_core::config::Bookkeeper::Native);
    let id = TaskId::from(id::SUPPORT);
    let clock = FixedClock(at("2026-10-07 09:30") + chrono::Duration::seconds(42));
    let mut task = store.get(&id).unwrap();
    task.log("with seconds", &clock);
    task.add_session(Session::new(clock.now(), "sid-1"));
    store.update(&task).unwrap();
    let back = store.get(&id).unwrap();
    assert_eq!(back, task);
    let text = nb.read(file_name(id::SUPPORT));
    assert!(
        text.contains("- 2026-10-07 09:30: with seconds\n"),
        "{text}"
    );
    assert!(text.contains("- 2026-10-07 09:30: `sid-1`\n"), "{text}");
}
