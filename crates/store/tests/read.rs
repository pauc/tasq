//! SV-005: `list`, `get`, `file_of` and `describe` over a directory of
//! `<ULID>.md` files, through the `Store` trait.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use chrono::{DateTime, Local};
use tasq_core::doctor::{Check, CheckStatus};
use tasq_core::model::{Status, Task, TaskDraft, TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::{IdScheme, Store, StoreError, StoreInfo};
use tasq_store::TasqStore;
use tasq_store::ulid::Ulid;

use support::{TempStore, entries};

#[test]
fn an_absent_directory_is_an_empty_store_and_is_not_created() {
    let temp = TempStore::absent();
    let store = temp.open();
    assert_eq!(store.list(&Filter::default()).unwrap(), Vec::new());
    assert_eq!(
        store.list(&Filter::default().any_done()).unwrap(),
        Vec::new()
    );
    assert_eq!(
        store.describe(),
        StoreInfo {
            name: "tasq".into(),
            location: temp.dir(),
            task_count: 0,
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            bookkeeper: None,
        }
    );
    assert!(!temp.dir().exists(), "a read never creates the directory");
    assert_eq!(entries(temp.root_path()), Vec::<String>::new());
}

/// The ULID spec's example name: 2016-07-30 23:54:10.259 UTC.
const SPEC_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// A ULID name minted at `millis` with a fixed random part.
fn ulid_name(millis: u64) -> String {
    format!(
        "{}.md",
        Ulid::from_parts(millis, 0xD676_4C61_EFB9_9302_BD5B)
    )
}

/// A minimal v2 task file.
fn task_file(id: u64, title: &str, status: &str) -> String {
    format!(
        "---\nid: {id}\nstatus: {status}\n---\n# {title}\n\n## Progress\n\n- 2026-10-04 10:15: created\n"
    )
}

fn ids(tasks: &[Task]) -> Vec<u64> {
    tasks
        .iter()
        .map(|t| t.id.as_str().parse().unwrap())
        .collect()
}

#[test]
fn get_reads_the_task_whose_frontmatter_id_matches_and_dates_it_from_the_ulid() {
    let temp = TempStore::empty();
    let path = temp.write(
        &format!("{SPEC_ULID}.md"),
        &task_file(47, "Spec example", "ready"),
    );
    temp.write(
        &ulid_name(1_700_000_000_000),
        &task_file(48, "Other", "ready"),
    );
    let store = temp.open();
    let task = store.get(&TaskId::from(47)).unwrap();
    assert_eq!(task.title, "Spec example");
    assert_eq!(task.status, Some(Status::READY));
    let expected = DateTime::from_timestamp_millis(1_469_922_850_259)
        .unwrap()
        .with_timezone(&Local)
        .naive_local();
    assert_eq!(
        task.created_at,
        Some(expected),
        "the ULID's time, in local time"
    );
    assert_eq!(
        store.get(&TaskId::from(48)).unwrap().created_at,
        Some(
            DateTime::from_timestamp_millis(1_700_000_000_000)
                .unwrap()
                .with_timezone(&Local)
                .naive_local()
        )
    );
    assert_eq!(store.file_of(&TaskId::from(47)).unwrap(), Some(path));
    let err = store.get(&TaskId::from(49)).unwrap_err();
    assert!(
        matches!(&err, StoreError::NotFound(id) if id.as_str() == "49"),
        "{err:?}"
    );
    assert!(matches!(
        store.file_of(&TaskId::from(49)),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn list_is_in_file_name_order_and_applies_the_filter() {
    let temp = TempStore::empty();
    temp.write(&ulid_name(3_000), &task_file(1, "Third by name", "blocked"));
    temp.write(&ulid_name(1_000), &task_file(9, "First by name", "ready"));
    temp.write(&ulid_name(2_000), &task_file(5, "Second by name", "done"));
    let store = temp.open();
    assert_eq!(
        ids(&store.list(&Filter::default()).unwrap()),
        vec![9, 1],
        "open only"
    );
    assert_eq!(
        ids(&store.list(&Filter::default().any_done()).unwrap()),
        vec![9, 5, 1]
    );
    assert_eq!(
        ids(&store.list(&Filter::default().done(true)).unwrap()),
        vec![5]
    );
    assert_eq!(
        ids(&store
            .list(&Filter::default().status(Status::BLOCKED))
            .unwrap()),
        vec![1]
    );
    let titles: Vec<String> = store
        .list(&Filter::default().any_done())
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        vec!["First by name", "Second by name", "Third by name"]
    );
    assert_eq!(store.describe().task_count, 3, "open and done count");
}

#[test]
fn an_id_claimed_by_two_files_is_ambiguous() {
    let temp = TempStore::empty();
    let first = temp.write(&ulid_name(1_000), &task_file(7, "Original", "ready"));
    let second = temp.write(&ulid_name(2_000), &task_file(7, "A bad copy", "ready"));
    temp.write(&ulid_name(3_000), &task_file(8, "Fine", "ready"));
    let mut store = temp.open();
    let err = store.get(&TaskId::from(7)).unwrap_err();
    assert!(
        matches!(&err, StoreError::AmbiguousId { id, paths } if id.as_str() == "7" && *paths == vec![first.clone(), second.clone()]),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "id 7 is claimed by more than one file: {}, {}",
            first.display(),
            second.display()
        )
    );
    assert!(matches!(
        store.file_of(&TaskId::from(7)),
        Err(StoreError::AmbiguousId { .. })
    ));
    assert_eq!(store.get(&TaskId::from(8)).unwrap().title, "Fine");
    assert_eq!(
        store.take_warnings(),
        Vec::<String>::new(),
        "get does not warn"
    );
    let listed = store.list(&Filter::default()).unwrap();
    assert_eq!(ids(&listed), vec![7, 7, 8], "list returns both");
    assert_eq!(listed[1].title, "A bad copy");
    assert_eq!(
        store.take_warnings(),
        vec![format!(
            "id 7 is claimed by more than one file: {}, {}; see 'tasq doctor'",
            first.display(),
            second.display()
        )]
    );
    assert_eq!(store.take_warnings(), Vec::<String>::new(), "taken once");
}

#[test]
fn reads_ignore_what_is_not_a_task_file() {
    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(1, "Task", "ready"));
    temp.write("README.md", "# Notes about this store\n");
    temp.write(&ulid_name(2_000), "---\nstatus: ready\n---\n# No id\n");
    temp.write(&ulid_name(3_000), "---\nid: 2\nstatus: ready\n");
    temp.write("notes.txt", &task_file(3, "Wrong extension", "ready"));
    temp.write(
        &format!("{SPEC_ULID}.md.bak"),
        &task_file(4, "Backup", "ready"),
    );
    temp.write(".tasq/lock", "12345\n");
    temp.write(".tasq/stray.md", &task_file(5, "Inside .tasq", "ready"));
    temp.write(".#editor-lock.md", &task_file(6, "Editor lock", "ready"));
    std::fs::create_dir(temp.file("subdir.md")).unwrap();
    let store = temp.open();
    assert_eq!(ids(&store.list(&Filter::default()).unwrap()), vec![1]);
    for id in [2, 3, 4, 5, 6] {
        assert!(
            matches!(store.get(&TaskId::from(id)), Err(StoreError::NotFound(_))),
            "id {id}"
        );
    }
    assert_eq!(store.describe().task_count, 1);
}

#[test]
#[cfg(unix)]
fn a_dangling_symlink_is_dropped_and_a_live_one_is_read() {
    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(1, "Task", "ready"));
    std::os::unix::fs::symlink(temp.file("gone.md"), temp.file(&ulid_name(2_000))).unwrap();
    let target = temp.root_path().join("elsewhere.md");
    std::fs::write(&target, task_file(2, "Linked", "ready")).unwrap();
    std::os::unix::fs::symlink(&target, temp.file(&ulid_name(3_000))).unwrap();
    let store = temp.open();
    assert_eq!(ids(&store.list(&Filter::default()).unwrap()), vec![1, 2]);
    assert_eq!(
        store.file_of(&TaskId::from(2)).unwrap(),
        Some(temp.file(&ulid_name(3_000))),
        "the link, not its target"
    );
    assert_eq!(
        store.doctor()[0],
        Check::ok(
            "store",
            format!("store at {}, 2 task file(s)", temp.dir().display())
        )
    );
}

#[test]
fn the_workflow_says_which_status_values_are_statuses() {
    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(1, "In review", "review"));
    assert_eq!(
        temp.open().get(&TaskId::from(1)).unwrap().status,
        None,
        "the default workflow has no 'review'"
    );
    let workflow = Workflow::new(vec![Status::new("review").unwrap()]);
    let store = TasqStore::new(temp.dir(), workflow);
    assert_eq!(
        store.get(&TaskId::from(1)).unwrap().status,
        Some(Status::new("review").unwrap())
    );
}

#[test]
fn a_task_file_with_another_name_is_a_task_without_a_creation_time() {
    let temp = TempStore::empty();
    temp.write("imported.md", &task_file(12, "Imported by hand", "ready"));
    temp.write(
        "01arz3ndektsv4rrffq69g5fav.md",
        &task_file(13, "Lowercase name", "ready"),
    );
    let store = temp.open();
    let task = store.get(&TaskId::from(12)).unwrap();
    assert_eq!(task.title, "Imported by hand");
    assert_eq!(task.created_at, None);
    assert_eq!(store.get(&TaskId::from(13)).unwrap().created_at, None);
    assert_eq!(
        ids(&store.list(&Filter::default()).unwrap()),
        vec![13, 12],
        "name order"
    );
}

#[test]
#[cfg(unix)]
fn reads_propagate_errors_other_than_a_missing_file() {
    use std::os::unix::fs::PermissionsExt;
    if is_root() {
        eprintln!("skipped: running as root, permissions do not apply");
        return;
    }
    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(1, "Task", "ready"));
    let unreadable = temp.write(&ulid_name(2_000), &task_file(2, "Unreadable", "ready"));
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut store = temp.open();
    let err = store.list(&Filter::default()).unwrap_err();
    assert!(
        matches!(&err, StoreError::Io { path, .. } if *path == unreadable),
        "{err:?}"
    );
    assert!(matches!(
        store.get(&TaskId::from(1)),
        Err(StoreError::Io { .. })
    ));
    assert_eq!(store.describe().task_count, 0, "describe cannot count");
    let warnings = store.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with(&format!(
            "the store cannot be read: {}: ",
            unreadable.display()
        )),
        "{}",
        warnings[0]
    );
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(ids(&store.list(&Filter::default()).unwrap()), vec![1, 2]);

    // The directory itself unreadable: the error names it.
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let err = store.list(&Filter::default()).unwrap_err();
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(&err, StoreError::Io { path, .. } if *path == temp.dir()),
        "{err:?}"
    );
}

#[cfg(unix)]
fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

#[test]
fn writes_are_not_supported_yet() {
    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(1, "Task", "ready"));
    let mut store = temp.open();
    let draft = TaskDraft::new("New");
    assert_eq!(
        store.create(draft).unwrap_err().to_string(),
        "unsupported: create (the tasq store does not write yet)"
    );
    let task = store.get(&TaskId::from(1)).unwrap();
    assert_eq!(
        store.update(&task).unwrap_err().to_string(),
        "unsupported: update of task 1 (the tasq store does not write yet)"
    );
    assert_eq!(
        store
            .set_done(&TaskId::from(1), true)
            .unwrap_err()
            .to_string(),
        "unsupported: set_done of task 1 (the tasq store does not write yet)"
    );
    assert_eq!(
        store.get(&TaskId::from(1)).unwrap(),
        task,
        "nothing changed"
    );
}

#[test]
fn doctor_reports_the_directory_the_non_task_files_and_the_duplicate_ids() {
    let temp = TempStore::absent();
    let store = temp.open();
    assert_eq!(
        store.doctor(),
        vec![Check::ok(
            "store",
            format!(
                "store at {} does not exist yet (created on the first write)",
                temp.dir().display()
            )
        ),]
    );

    let temp = TempStore::empty();
    temp.write(&ulid_name(1_000), &task_file(7, "Original", "ready"));
    let readme = temp.write("README.md", "# Notes\n");
    let no_id = temp.write(&ulid_name(2_000), "---\nstatus: ready\n---\n# No id\n");
    let copy = temp.write(&ulid_name(3_000), &task_file(7, "Copy", "ready"));
    let bad_id = temp.write(&ulid_name(4_000), "---\nid: seven\n---\n# Bad id\n");
    let open_block = temp.write(&ulid_name(5_000), "---\nid: 9\n# Never closed\n");
    let no_title = temp.write(&ulid_name(6_000), "---\nid: 10\n---\n\nNo heading\n");
    let store = temp.open();
    assert_eq!(
        store.doctor(),
        vec![
            Check::ok(
                "store",
                format!("store at {}, 2 task file(s)", temp.dir().display())
            ),
            Check::fail(
                "task-file",
                format!(
                    "{}: line 3: the frontmatter closes without an 'id' key",
                    no_id.display()
                ),
                "add an 'id: <number>' line to the frontmatter",
            ),
            Check::fail(
                "task-file",
                format!(
                    "{}: line 2: id \"seven\" is not a positive integer",
                    bad_id.display()
                ),
                "make the id a positive integer",
            ),
            Check::fail(
                "task-file",
                format!(
                    "{}: line 1: the frontmatter has no closing '---'",
                    open_block.display()
                ),
                "close the frontmatter block with a '---' line",
            ),
            Check::fail(
                "task-file",
                format!(
                    "{}: line 5: expected the title line '# Title' after the frontmatter, found \"No heading\"",
                    no_title.display()
                ),
                "put a '# Title' line right after the frontmatter",
            ),
            Check::fail(
                "task-file",
                format!(
                    "{}: not a v2 task file: first line \"# Notes\" is not '---'",
                    readme.display()
                ),
                "add a frontmatter block with an id, or move the file out of the store",
            ),
            Check::fail(
                "ids",
                format!(
                    "id 7 is claimed by more than one file: {}, {}",
                    temp.file(&ulid_name(1_000)).display(),
                    copy.display()
                ),
                "give one of the files another id, or remove the copy",
            ),
        ]
    );

    for path in [&readme, &no_id, &copy, &bad_id, &open_block, &no_title] {
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(
        store.doctor(),
        vec![
            Check::ok(
                "store",
                format!("store at {}, 1 task file(s)", temp.dir().display())
            ),
            Check::ok("task-files", "every .md file is a v2 task file"),
            Check::ok("ids", "ids are stable and no id is claimed twice"),
        ]
    );
}

#[test]
#[cfg(unix)]
fn doctor_fails_on_a_directory_it_cannot_read() {
    use std::os::unix::fs::PermissionsExt;
    if is_root() {
        eprintln!("skipped: running as root, permissions do not apply");
        return;
    }
    let temp = TempStore::empty();
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let checks = temp.open().doctor();
    std::fs::set_permissions(temp.dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "store");
    assert_eq!(checks[0].status, CheckStatus::Fail);
    assert!(
        checks[0].detail.starts_with(&format!(
            "store at {} cannot be read: ",
            temp.dir().display()
        )),
        "{}",
        checks[0].detail
    );
    assert_eq!(
        checks[0].fix.as_deref(),
        Some("check the directory's permissions")
    );
}
