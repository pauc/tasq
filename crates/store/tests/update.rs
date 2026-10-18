//! SV-007: `update` and `set_done` through the `Store` trait: the edits,
//! the conflict check, the store lock; and doctor's stale-lock check.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::time::{Duration, SystemTime};

use tasq_core::clock::FixedClock;
use tasq_core::doctor::Check;
use tasq_core::model::{
    Link, Origin, Priority, ProgressEntry, Session, Status, Tag, TaskId, Worktree,
};
use tasq_core::query::Filter;
use tasq_core::store::{Store, StoreError};

use support::{TempStore, task_file, touch, ulid_name};

/// The file every test starts from: id 3, a hand-written section after
/// `## Progress` that no op touches.
const ORIGINAL: &str = "---\nid: 3\nstatus: ready\npriority: B\n---\n# Draft title\n\n## Progress\n\n- 2026-10-04 10:15: created\n\n## Notes\n\nhand-written\n";

const NAME: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV.md";

fn at(s: &str) -> chrono::NaiveDateTime {
    FixedClock::at(s).0
}

/// A store over a directory holding `ORIGINAL` as `NAME`.
fn store_with_original() -> (TempStore, tasq_store::TasqStore) {
    let temp = TempStore::empty();
    temp.write(NAME, ORIGINAL);
    let store = temp.open();
    (temp, store)
}

fn read(temp: &TempStore) -> String {
    std::fs::read_to_string(temp.file(NAME)).unwrap()
}

#[test]
fn update_writes_every_scalar_and_appends_to_every_list_leaving_the_rest_alone() {
    let (temp, mut store) = store_with_original();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.title = "Final title".into();
    task.set_status(Status::BLOCKED);
    task.set_priority(Priority::A);
    task.rank = Some(5);
    task.due = Some(at("2026-10-20 00:00").date());
    task.project = Some("/p".into());
    task.origin = Some(Origin {
        source: "gitlab".into(),
        external_id: "!7".into(),
        url: Some("https://mr/7".into()),
    });
    task.description = Some("Some text".into());
    task.add_tag(Tag::new("gitlab").unwrap());
    task.add_tag(Tag::new("review").unwrap());
    task.add_related(Link::new("https://rel/1"));
    task.add_merge_request(Link::labelled("https://mr/7", "MR"));
    task.add_worktree(Worktree::on_branch("/w1", "b1"));
    task.add_session(Session::new(at("2026-10-05 09:00"), "s1"));
    task.progress
        .push(ProgressEntry::new(at("2026-10-05 09:00"), "unblocked"));
    store.update(&task).unwrap();
    assert_eq!(
        read(&temp),
        "---\nid: 3\nstatus: blocked\npriority: A\nrank: 5\ndue: 2026-10-20\nproject: /p\nsource: gitlab\nsource_id: !7\nsource_url: https://mr/7\ntags: [gitlab, review]\n---\n# Final title\n\n## Description\n\nSome text\n\n## Related\n\n- https://rel/1\n\n### Merge requests\n\n- [MR](https://mr/7)\n\n## Worktrees\n\n- /w1 (`b1`)\n\n## Sessions\n\n- 2026-10-05 09:00: `s1`\n\n## Progress\n\n- 2026-10-04 10:15: created\n- 2026-10-05 09:00: unblocked\n\n## Notes\n\nhand-written\n"
    );
    assert_eq!(
        store.get(&TaskId::from(3)).unwrap(),
        task,
        "visible on re-read"
    );
    assert_eq!(
        store.list(&Filter::default()).unwrap(),
        vec![task.clone()],
        "and through list"
    );
    assert!(!temp.lock_path().exists(), "the lock is released");

    // The same reader can keep writing, and a scalar can be cleared.
    task.rank = None;
    task.due = None;
    task.project = None;
    task.origin = None;
    task.description = None;
    task.tags.clear();
    task.clear_status();
    store.update(&task).unwrap();
    assert_eq!(
        read(&temp),
        "---\nid: 3\npriority: A\n---\n# Final title\n\n## Related\n\n- https://rel/1\n\n### Merge requests\n\n- [MR](https://mr/7)\n\n## Worktrees\n\n- /w1 (`b1`)\n\n## Sessions\n\n- 2026-10-05 09:00: `s1`\n\n## Progress\n\n- 2026-10-04 10:15: created\n- 2026-10-05 09:00: unblocked\n\n## Notes\n\nhand-written\n"
    );
    assert_eq!(store.get(&TaskId::from(3)).unwrap(), task);
    assert_eq!(store.take_warnings(), Vec::<String>::new());
}

#[test]
fn an_unchanged_update_writes_nothing() {
    let (temp, mut store) = store_with_original();
    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    touch(&temp.file(NAME), past);
    let task = store.get(&TaskId::from(3)).unwrap();
    store.update(&task).unwrap();
    assert_eq!(
        std::fs::metadata(temp.file(NAME))
            .unwrap()
            .modified()
            .unwrap(),
        past,
        "not rewritten"
    );
    assert_eq!(read(&temp), ORIGINAL);
}

#[test]
fn update_refuses_changes_it_cannot_express_and_writes_nothing() {
    let (temp, mut store) = store_with_original();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.add_related(Link::new("https://rel/1"));
    store.update(&task).unwrap();
    let written = read(&temp);
    task.related.clear();
    task.progress[0].note = "rewritten".into();
    task.set_priority(Priority::C);
    let err = store.update(&task).unwrap_err();
    assert_eq!(
        err.to_string(),
        "unsupported: changing related, progress of task 3 (the tasq store cannot remove or rewrite related links, merge requests, worktrees, sessions or progress entries)"
    );
    assert_eq!(
        read(&temp),
        written,
        "the priority change was not written either"
    );
}

#[test]
fn update_of_an_unknown_or_ambiguous_id_fails_before_writing() {
    let (temp, mut store) = store_with_original();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.id = TaskId::from(4);
    assert!(
        matches!(store.update(&task).unwrap_err(), StoreError::NotFound(id) if id == TaskId::from(4))
    );
    let copy = temp.write(&ulid_name(5_000, 1), &task_file(3, "Copy", "ready"));
    task.id = TaskId::from(3);
    task.set_priority(Priority::A);
    let err = store.update(&task).unwrap_err();
    assert!(
        matches!(&err, StoreError::AmbiguousId { id, paths } if *id == TaskId::from(3) && *paths == vec![copy.clone(), temp.file(NAME)]),
        "in name order: {err:?}"
    );
    assert_eq!(read(&temp), ORIGINAL, "nothing written");
    assert!(!temp.lock_path().exists());
}

#[test]
fn update_detects_a_change_made_since_the_task_was_read() {
    let (temp, mut store) = store_with_original();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    // Somebody else edits the file in between (same length, so only the
    // hash or mtime can tell).
    let edited = ORIGINAL.replace("hand-written", "HAND-WRITTEN");
    temp.write(NAME, &edited);
    task.set_status(Status::BLOCKED);
    let err = store.update(&task).unwrap_err();
    assert!(
        matches!(&err, StoreError::Conflict { id, path } if *id == task.id && *path == temp.file(NAME)),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "task 3 changed since it was read ({}); re-read it and retry",
            temp.file(NAME).display()
        )
    );
    assert_eq!(read(&temp), edited, "the external edit survives");
    // Still a conflict until the task is re-read.
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::Conflict { .. }
    ));
    let mut fresh = store.get(&task.id).unwrap();
    fresh.set_status(Status::BLOCKED);
    store.update(&fresh).unwrap();
    assert_eq!(store.get(&task.id).unwrap(), fresh);
    assert!(read(&temp).contains("HAND-WRITTEN"));
}

#[test]
fn update_detects_a_change_seen_through_list_and_after_create() {
    let (temp, mut store) = store_with_original();
    let mut task = store
        .list(&Filter::default())
        .unwrap()
        .into_iter()
        .find(|t| t.id == TaskId::from(3))
        .unwrap();
    temp.write(NAME, &ORIGINAL.replace("Draft", "Other"));
    task.set_status(Status::LATER);
    assert!(matches!(
        store.update(&task).unwrap_err(),
        StoreError::Conflict { .. }
    ));

    let mut store = temp.open_at(&[7]);
    let mut created = store
        .create(tasq_core::model::TaskDraft::new("Created"))
        .unwrap();
    let path = temp.file(&ulid_name(support::clock_millis(), 7));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("Created", "Renamed")).unwrap();
    created.set_priority(Priority::A);
    assert!(
        matches!(store.update(&created).unwrap_err(), StoreError::Conflict { path: p, .. } if p == path),
        "create reads the task back, so its revision is known"
    );
}

#[test]
fn update_without_a_prior_read_in_this_store_is_not_checked() {
    let (temp, _) = store_with_original();
    let mut reader = temp.open();
    let mut task = reader.get(&TaskId::from(3)).unwrap();
    temp.write(NAME, &ORIGINAL.replace("hand-written", "edited elsewhere"));
    task.set_status(Status::BLOCKED);
    let mut writer = temp.open();
    writer.update(&task).unwrap();
    let text = read(&temp);
    assert!(text.contains("status: blocked\n"), "{text}");
    assert!(
        text.contains("edited elsewhere"),
        "the ops apply to the file as it is now"
    );
    assert!(
        matches!(
            reader.update(&task).unwrap_err(),
            StoreError::Conflict { .. }
        ),
        "the reader's baseline is the original file"
    );
}

#[test]
fn update_waits_for_a_live_lock_and_breaks_a_stale_one() {
    let (temp, store) = store_with_original();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.set_priority(Priority::A);
    std::fs::create_dir_all(temp.lock_path().parent().unwrap()).unwrap();
    std::fs::write(temp.lock_path(), "").unwrap();
    touch(&temp.lock_path(), SystemTime::now());
    let (tx, rx) = std::sync::mpsc::channel();
    let dir = temp.dir();
    std::thread::spawn(move || {
        let mut store = tasq_store::TasqStore::new(dir, tasq_core::model::Workflow::default());
        let _ = tx.send(store.update(&task));
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "waiting on the live lock"
    );
    assert!(
        read(&temp).contains("priority: B\n"),
        "not written while locked"
    );
    std::fs::remove_file(temp.lock_path()).unwrap();
    rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
    assert!(read(&temp).contains("priority: A\n"));
    assert!(!temp.lock_path().exists(), "released");

    // A stale lock (two minutes old by the store's clock) is broken.
    let mut store = temp.open_with(support::CLOCK, Box::new(|| 1));
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.set_priority(Priority::C);
    std::fs::write(temp.lock_path(), "").unwrap();
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(support::clock_millis());
    touch(&temp.lock_path(), now - tasq_store::lock::STALE_AFTER);
    store.update(&task).unwrap();
    assert!(read(&temp).contains("priority: C\n"));
    assert!(!temp.lock_path().exists());
}

#[test]
#[cfg(unix)]
fn update_keeps_the_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, mut store) = store_with_original();
    std::fs::set_permissions(temp.file(NAME), std::fs::Permissions::from_mode(0o640)).unwrap();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.set_priority(Priority::C);
    store.update(&task).unwrap();
    assert_eq!(
        std::fs::metadata(temp.file(NAME))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        support::entries(&temp.dir()),
        vec![".tasq".to_owned(), NAME.to_owned()],
        "no temp file left"
    );
}

#[test]
fn a_file_that_is_not_valid_utf8_is_read_but_never_written_back() {
    let temp = TempStore::empty();
    let bytes = b"---\nid: 3\nstatus: ready\n---\n# Caf\xe9\n";
    std::fs::write(temp.file(NAME), bytes).unwrap();
    let mut store = temp.open();
    let mut task = store.get(&TaskId::from(3)).unwrap();
    assert_eq!(
        task.title, "Caf\u{FFFD}",
        "read with a replacement character"
    );
    store.update(&task).unwrap();
    store
        .set_done(&TaskId::from(3), false)
        .unwrap_or_else(|e| panic!("nothing to write, nothing refused: {e}"));
    task.set_priority(Priority::A);
    let err = store.update(&task).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "{}: not valid UTF-8; a write would replace the invalid bytes",
            temp.file(NAME).display()
        )
    );
    assert!(
        matches!(&err, StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::InvalidData)
    );
    assert_eq!(std::fs::read(temp.file(NAME)).unwrap(), bytes, "untouched");
}

#[test]
fn set_done_writes_status_done_and_closed_and_reopening_drops_both() {
    let temp = TempStore::empty();
    temp.write(NAME, ORIGINAL);
    let mut store = temp.open_with("2026-10-06 17:30", Box::new(|| 1));
    store.set_done(&TaskId::from(3), true).unwrap();
    assert_eq!(
        read(&temp),
        ORIGINAL.replace(
            "status: ready\npriority: B\n",
            "status: done\npriority: B\nclosed: 2026-10-06 17:30\n"
        ),
        "the status key is replaced and closed is stamped from the clock, nothing else moves"
    );
    let done = store.get(&TaskId::from(3)).unwrap();
    assert!(done.done);
    assert_eq!(done.status, None);
    assert_eq!(done.closed_at, Some(at("2026-10-06 17:30")));
    assert!(!temp.lock_path().exists());

    // Closing through update records the time; set_done(false) drops it.
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.done = false;
    task.closed_at = None;
    task.set_status(Status::READY);
    store.update(&task).unwrap();
    assert_eq!(
        read(&temp),
        ORIGINAL,
        "reopened: back to the original bytes"
    );
    let mut task = store.get(&TaskId::from(3)).unwrap();
    task.close(&FixedClock::at("2026-10-06 17:30"));
    store.update(&task).unwrap();
    assert_eq!(
        read(&temp),
        ORIGINAL.replace(
            "status: ready\npriority: B\n",
            "status: done\npriority: B\nclosed: 2026-10-06 17:30\n"
        )
    );
    store.set_done(&TaskId::from(3), false).unwrap();
    assert_eq!(
        read(&temp),
        ORIGINAL.replace("status: ready\n", ""),
        "reopened with no status: nothing remembers the old one"
    );
    let reopened = store.get(&TaskId::from(3)).unwrap();
    assert!(!reopened.done);
    assert_eq!(reopened.status, None);
    assert_eq!(reopened.closed_at, None);
}

#[test]
fn set_done_is_idempotent_and_invalidates_tasks_read_before_it() {
    let (temp, mut store) = store_with_original();
    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    touch(&temp.file(NAME), past);
    store.set_done(&TaskId::from(3), false).unwrap();
    assert_eq!(
        std::fs::metadata(temp.file(NAME))
            .unwrap()
            .modified()
            .unwrap(),
        past,
        "already open: not rewritten"
    );
    let mut stale = store.get(&TaskId::from(3)).unwrap();
    store.set_done(&TaskId::from(3), true).unwrap();
    let closed = read(&temp);
    assert!(closed.contains("\nclosed: "), "{closed}");
    store.set_done(&TaskId::from(3), true).unwrap();
    assert_eq!(read(&temp), closed, "already done: closed is not moved");
    stale.set_priority(Priority::A);
    assert!(
        matches!(
            store.update(&stale).unwrap_err(),
            StoreError::Conflict { .. }
        ),
        "the task read before set_done no longer matches the file"
    );
    let mut fresh = store.get(&TaskId::from(3)).unwrap();
    fresh.set_priority(Priority::A);
    store.update(&fresh).unwrap();
    assert_eq!(store.get(&TaskId::from(3)).unwrap(), fresh);
}

#[test]
fn set_done_of_an_unknown_id_or_an_unwritable_file_fails_before_writing() {
    let (temp, mut store) = store_with_original();
    assert!(matches!(
        store.set_done(&TaskId::from(4), true).unwrap_err(),
        StoreError::NotFound(id) if id == TaskId::from(4)
    ));
    assert_eq!(read(&temp), ORIGINAL);
    let bytes = b"---\nid: 5\nstatus: ready\n---\n# Caf\xe9\n";
    std::fs::write(temp.file(&ulid_name(9_000, 9)), bytes).unwrap();
    let err = store.set_done(&TaskId::from(5), true).unwrap_err();
    assert!(
        matches!(&err, StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::InvalidData),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read(temp.file(&ulid_name(9_000, 9))).unwrap(),
        bytes
    );
    let copy = temp.write(&ulid_name(9_500, 9), &task_file(3, "Copy", "ready"));
    let err = store.set_done(&TaskId::from(3), true).unwrap_err();
    assert!(
        matches!(&err, StoreError::AmbiguousId { id, paths } if *id == TaskId::from(3) && *paths == vec![copy.clone(), temp.file(NAME)]),
        "in name order: {err:?}"
    );
    assert_eq!(read(&temp), ORIGINAL);
    assert!(!temp.lock_path().exists());
}

#[test]
fn doctor_reports_a_stale_lock_and_passes_a_live_one() {
    let (temp, store) = store_with_original();
    let clean = vec![
        Check::ok(
            "store",
            format!("store at {}, 1 task file(s)", temp.dir().display()),
        ),
        Check::ok("task-files", "every .md file is a v2 task file"),
        Check::ok("ids", "ids are stable and no id is claimed twice"),
        Check::ok("lock", "no write in progress"),
    ];
    assert_eq!(store.doctor(), clean);

    let store = temp.open_with(support::CLOCK, Box::new(|| 1));
    let lock = temp.lock_path();
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "").unwrap();
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(support::clock_millis());
    touch(
        &lock,
        now - tasq_store::lock::STALE_AFTER + Duration::from_millis(1),
    );
    let mut live = clean.clone();
    live[3] = Check::ok(
        "lock",
        format!("a write is in progress (lock at {})", lock.display()),
    );
    assert_eq!(store.doctor(), live, "one millisecond short of stale");

    touch(&lock, now - tasq_store::lock::STALE_AFTER);
    let mut stale = clean.clone();
    stale[3] = Check::warn(
        "lock",
        format!(
            "stale lock at {} (untouched for 2 minutes or more: its writer died)",
            lock.display()
        ),
        "remove the file, or ignore it: the next write removes it",
    );
    assert_eq!(store.doctor(), stale);
    assert!(lock.exists(), "doctor does not break it");
}
