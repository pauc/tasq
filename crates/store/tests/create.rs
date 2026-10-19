//! SV-006: `create` through the `Store` trait: the id, the file, the ULID
//! name and the store lock.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use tasq_core::clock::FixedClock;
use tasq_core::model::{Status, TaskDraft, TaskId};
use tasq_core::query::Filter;
use tasq_core::store::Store;
use tasq_store::ulid::Ulid;

use support::{CLOCK, TempStore, clock_millis, entries, task_file, touch, ulid_name};

const RANDOM: u128 = 0xD676_4C61_EFB9_9302_BD5B;

#[test]
fn creating_in_an_absent_directory_creates_it_and_the_task_gets_id_1() {
    let temp = TempStore::absent();
    let mut store = temp.open_at(&[RANDOM]);
    let task = store.create(TaskDraft::new("First")).unwrap();
    assert_eq!(task.id, TaskId::from(1));
    assert_eq!(task.title, "First");
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.created_at, Some(FixedClock::at(CLOCK).0));
    assert_eq!(task.progress.len(), 1);
    assert_eq!(task.progress[0].note, "created via tasq create");
    let name = format!("{}.md", Ulid::from_parts(clock_millis(), RANDOM));
    assert_eq!(entries(&temp.dir()), vec![".tasq".to_owned(), name.clone()]);
    assert_eq!(
        std::fs::read_to_string(temp.file(&name)).unwrap(),
        "---\nid: 1\nstatus: ready\npriority: B\n---\n# First\n\n## Progress\n\n- 2026-10-04 12:34: created via tasq create\n"
    );
    assert!(!temp.lock_path().exists(), "the lock is released");
    assert_eq!(store.get(&TaskId::from(1)).unwrap(), task, "read back");
    assert_eq!(store.list(&Filter::default()).unwrap(), vec![task]);
    assert_eq!(store.take_warnings(), Vec::<String>::new());
}

#[test]
fn ids_are_the_highest_plus_one_and_a_deleted_id_is_not_reused() {
    let temp = TempStore::empty();
    let three = temp.write(&ulid_name(1_000, 1), &task_file(3, "Three", "ready"));
    temp.write(&ulid_name(2_000, 2), &task_file(7, "Seven, done", "done"));
    temp.write(&ulid_name(3_000, 3), &task_file(5, "Five", "blocked"));
    let mut store = temp.open_at(&[RANDOM, 0xB, 0xC]);
    let task = store.create(TaskDraft::new("Eight")).unwrap();
    assert_eq!(task.id, TaskId::from(8), "done tasks count");
    std::fs::remove_file(&three).unwrap();
    std::fs::remove_file(temp.file(&ulid_name(clock_millis(), RANDOM))).unwrap();
    let task = store.create(TaskDraft::new("Still eight")).unwrap();
    assert_eq!(task.id, TaskId::from(8), "7 is still the highest");

    // A stray copy (two files claiming 7) does not stop a create.
    temp.write("copy.md", &task_file(7, "Copy", "ready"));
    assert_eq!(
        store.create(TaskDraft::new("Nine")).unwrap().id,
        TaskId::from(9)
    );
    assert_eq!(store.take_warnings(), Vec::<String>::new());
}

#[test]
fn ten_parallel_creates_produce_ten_files_and_ten_distinct_ids() {
    let temp = TempStore::absent();
    let dir = temp.dir();
    let workers: Vec<_> = (0..10)
        .map(|n| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut store =
                    tasq_store::TasqStore::new(dir, tasq_core::model::Workflow::default())
                        .with_clock(Box::new(FixedClock::at(CLOCK)));
                store
                    .create(TaskDraft::new(format!("Task {n}")))
                    .map(|t| t.id)
            })
        })
        .collect();
    let mut ids: Vec<u64> = workers
        .into_iter()
        .map(|w| w.join().unwrap().unwrap().as_str().parse().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=10).collect::<Vec<u64>>());
    let names: Vec<String> = entries(&dir)
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert_eq!(names.len(), 10);
    for name in &names {
        let ulid = Ulid::parse(name.strip_suffix(".md").unwrap()).unwrap();
        assert_eq!(ulid.timestamp_millis(), clock_millis(), "{name}");
    }
    let store = temp.open_at(&[RANDOM]);
    assert_eq!(store.list(&Filter::default()).unwrap().len(), 10);
    assert!(!temp.lock_path().exists());
}

#[test]
fn the_file_name_is_a_ulid_minted_from_the_clock_and_get_reports_its_time() {
    let temp = TempStore::empty();
    let mut store = temp.open_at(&[0xAAAA, 0xBBBB]);
    let first = store.create(TaskDraft::new("First")).unwrap();
    let second = store.create(TaskDraft::new("Second")).unwrap();
    let millis = clock_millis();
    assert_eq!(
        entries(&temp.dir()),
        vec![
            ".tasq".to_owned(),
            ulid_name(millis, 0xAAAA),
            ulid_name(millis, 0xBBBB)
        ]
    );
    assert_eq!(
        store.file_of(&TaskId::from(1)).unwrap(),
        Some(temp.file(&ulid_name(millis, 0xAAAA)))
    );
    assert_eq!(first.created_at, Some(FixedClock::at(CLOCK).0));
    assert_eq!(second.created_at, Some(FixedClock::at(CLOCK).0));
    assert_eq!(
        store.get(&TaskId::from(2)).unwrap().created_at,
        Some(FixedClock::at(CLOCK).0)
    );
    assert_eq!(
        Ulid::parse(&ulid_name(millis, 0xAAAA).replace(".md", ""))
            .unwrap()
            .timestamp()
            .with_timezone(&chrono::Local)
            .naive_local(),
        FixedClock::at(CLOCK).0,
        "the name reads back as the clock's time"
    );
}

#[test]
fn a_clock_before_the_epoch_mints_a_zero_timestamp() {
    let temp = TempStore::empty();
    let mut store = temp.open_with("1960-01-01 00:00", Box::new(|| RANDOM));
    store.create(TaskDraft::new("Old")).unwrap();
    assert_eq!(
        entries(&temp.dir()),
        vec![".tasq".to_owned(), ulid_name(0, RANDOM)]
    );
}

#[test]
fn a_taken_name_is_retried_with_a_fresh_ulid() {
    let temp = TempStore::empty();
    let millis = clock_millis();
    let squatter = temp.write(
        &ulid_name(millis, 0xAAAA),
        &task_file(1, "Squatter", "ready"),
    );
    let mut store = temp.open_at(&[0xAAAA, 0xAAAA, 0xBBBB]);
    let task = store.create(TaskDraft::new("Retried")).unwrap();
    assert_eq!(task.id, TaskId::from(2));
    assert_eq!(
        std::fs::read_to_string(&squatter).unwrap(),
        task_file(1, "Squatter", "ready"),
        "untouched"
    );
    assert_eq!(
        entries(&temp.dir()),
        vec![
            ".tasq".to_owned(),
            ulid_name(millis, 0xAAAA),
            ulid_name(millis, 0xBBBB)
        ],
        "no temp file left behind"
    );
    assert_eq!(
        store.file_of(&TaskId::from(2)).unwrap(),
        Some(temp.file(&ulid_name(millis, 0xBBBB)))
    );
}

#[test]
fn a_name_that_stays_taken_is_an_error_after_a_bounded_number_of_attempts() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    let temp = TempStore::empty();
    let millis = clock_millis();
    temp.write(
        &ulid_name(millis, 0xAAAA),
        &task_file(1, "Squatter", "ready"),
    );
    let minted = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&minted);
    let mut store = temp.open_with(
        CLOCK,
        Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            0xAAAA
        }),
    );
    let err = store.create(TaskDraft::new("No room")).unwrap_err();
    let taken = temp.file(&ulid_name(millis, 0xAAAA));
    assert!(
        matches!(&err, tasq_core::store::StoreError::Io { path, source }
            if *path == taken && source.kind() == std::io::ErrorKind::AlreadyExists),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "{}: no free file name after {} attempts",
            taken.display(),
            tasq_store::store::MAX_NAME_ATTEMPTS
        )
    );
    assert_eq!(
        minted.load(Ordering::SeqCst),
        tasq_store::store::MAX_NAME_ATTEMPTS
    );
    assert_eq!(
        entries(&temp.dir()),
        vec![".tasq".to_owned(), ulid_name(millis, 0xAAAA)],
        "nothing written, no temp file left"
    );
    assert!(
        !temp.lock_path().exists(),
        "the lock is released on failure"
    );
    assert_eq!(store.list(&Filter::default()).unwrap().len(), 1);
}

/// The instant of the fixed clock, as a `SystemTime`.
fn clock_instant() -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_millis(clock_millis())
}

#[test]
fn a_stale_lock_is_broken_and_the_create_proceeds() {
    let temp = TempStore::empty();
    temp.write(".tasq/lock", "");
    touch(
        &temp.lock_path(),
        clock_instant() - tasq_store::lock::STALE_AFTER,
    );
    let mut store = temp.open_at(&[RANDOM]);
    let task = store.create(TaskDraft::new("After a crash")).unwrap();
    assert_eq!(task.id, TaskId::from(1));
    assert!(
        !temp.lock_path().exists(),
        "the stale lock is gone with ours"
    );
}

#[test]
fn a_live_lock_makes_the_create_wait_until_it_is_released() {
    let temp = TempStore::empty();
    temp.write(".tasq/lock", "");
    touch(
        &temp.lock_path(),
        clock_instant() - tasq_store::lock::STALE_AFTER + std::time::Duration::from_secs(1),
    );
    let dir = temp.dir();
    let lock = temp.lock_path();
    // The holder writes task 5 and only then releases the lock: a create
    // that waited allocates 6, one that did not would allocate 1.
    let holder = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(
            dir.join(ulid_name(1_000, 1)),
            task_file(5, "Written by the holder", "ready"),
        )
        .unwrap();
        std::fs::remove_file(lock).unwrap();
    });
    let mut store = temp.open_at(&[RANDOM]);
    let started = std::time::Instant::now();
    let task = store.create(TaskDraft::new("Waited")).unwrap();
    holder.join().unwrap();
    assert_eq!(task.id, TaskId::from(6));
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(300),
        "waited for the holder"
    );
    assert!(!temp.lock_path().exists());
}

#[test]
fn a_refused_draft_writes_nothing_and_creates_no_directory() {
    let temp = TempStore::absent();
    let mut store = temp.open_at(&[RANDOM]);
    let err = store
        .create(
            TaskDraft::new("MR").with_merge_request(tasq_core::model::Link::new("https://x/mr/1")),
        )
        .unwrap_err();
    assert!(
        matches!(err, tasq_core::store::StoreError::Unsupported { .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("https://x/mr/1 without a title"));
    assert!(!temp.dir().exists(), "nothing written");
}

#[test]
fn a_full_draft_is_written_as_a_canonical_task_file() {
    use tasq_core::model::{Link, Origin, Priority, Tag};
    let temp = TempStore::empty();
    let mut store = temp.open_at(&[RANDOM, 0xB]);
    let draft = TaskDraft::new("Full draft")
        .with_status(Some(Status::IN_PROGRESS))
        .with_priority(Priority::A)
        .with_due(chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap())
        .with_description("Line one.\nLine two.")
        .with_project("/home/pau/code/tasks")
        .with_tag(Tag::new("gitlab").unwrap())
        .with_tag(Tag::new("review-request").unwrap())
        .with_related(Link::new("https://example.invalid/issues/42"))
        .with_merge_request(Link::labelled(
            "https://example.invalid/mr/123",
            "Add parser",
        ))
        .with_note("created from the issue")
        .with_origin(Origin {
            source: "gitlab".into(),
            external_id: "!123".into(),
            url: Some("https://example.invalid/mr/123".into()),
        });
    let task = store.create(draft).unwrap();
    assert_eq!(
        std::fs::read_to_string(temp.file(&ulid_name(clock_millis(), RANDOM))).unwrap(),
        concat!(
            "---\n",
            "id: 1\n",
            "status: in-progress\n",
            "priority: A\n",
            "due: 2026-10-10\n",
            "project: /home/pau/code/tasks\n",
            "source: gitlab\n",
            "source_id: !123\n",
            "source_url: https://example.invalid/mr/123\n",
            "tags: [gitlab, review-request]\n",
            "---\n",
            "# Full draft\n",
            "\n",
            "## Description\n",
            "\n",
            "Line one.\n",
            "Line two.\n",
            "\n",
            "## Related\n",
            "\n",
            "- https://example.invalid/issues/42\n",
            "\n",
            "### Merge requests\n",
            "\n",
            "- [Add parser](https://example.invalid/mr/123)\n",
            "\n",
            "## Progress\n",
            "\n",
            "- 2026-10-04 12:34: created from the issue\n",
        )
    );
    assert_eq!(store.get(&TaskId::from(1)).unwrap(), task);

    let done = store
        .create(
            TaskDraft::new("Already done")
                .with_done(true)
                .with_status(Some(Status::IN_PROGRESS)),
        )
        .unwrap();
    assert_eq!(done.id, TaskId::from(2));
    assert!(done.done);
    assert_eq!(done.status, None, "done drafts carry no status");
    assert_eq!(done.closed_at, Some(FixedClock::at(CLOCK).0));
    let files: Vec<String> = entries(&temp.dir())
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert_eq!(files.len(), 2);
    assert_eq!(
        std::fs::read_to_string(store.file_of(&TaskId::from(2)).unwrap().unwrap()).unwrap(),
        "---\nid: 2\nstatus: done\npriority: B\nclosed: 2026-10-04 12:34\n---\n# Already done\n\n## Progress\n\n- 2026-10-04 12:34: created via tasq create\n"
    );
}

#[test]
#[cfg(unix)]
fn create_reports_a_directory_it_cannot_write_to() {
    use std::os::unix::fs::PermissionsExt;
    if support::is_root() {
        eprintln!("skipped: running as root, permissions do not apply");
        return;
    }
    let temp = TempStore::empty();
    std::fs::create_dir(temp.dir().join(".tasq")).unwrap();
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut store = temp.open_at(&[RANDOM]);
    let err = store.create(TaskDraft::new("Nope")).unwrap_err();
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(&err, tasq_core::store::StoreError::Io { path, source }
            if *path == temp.dir() && source.kind() == std::io::ErrorKind::PermissionDenied),
        "{err:?}"
    );
    assert!(
        !temp.lock_path().exists(),
        "the lock is released on failure"
    );

    // The lock itself cannot be taken: the error names the lock file.
    std::fs::set_permissions(
        temp.dir().join(".tasq"),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    let err = store.create(TaskDraft::new("Nope")).unwrap_err();
    std::fs::set_permissions(
        temp.dir().join(".tasq"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(
        matches!(&err, tasq_core::store::StoreError::Io { path, source }
            if *path == temp.lock_path() && source.kind() == std::io::ErrorKind::PermissionDenied),
        "{err:?}"
    );
    assert_eq!(
        entries(&temp.dir()),
        vec![".tasq".to_owned()],
        "nothing written"
    );
}
