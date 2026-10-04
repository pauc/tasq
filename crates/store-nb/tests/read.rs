//! T-201/T-202: index reading, `list` and `get` on a temp copy of the fixture.

mod support;

use std::path::PathBuf;

use tasq_core::clock::{Clock, FixedClock};
use tasq_core::model::{Link, Priority, ProgressEntry, Session, Status, Tag, TaskId, Worktree};
use tasq_core::query::Filter;
use tasq_store_nb::{Store, StoreError};

use support::{NbEnv, id};

fn ids(tasks: &[tasq_core::model::Task]) -> Vec<u64> {
    tasks
        .iter()
        .map(|t| t.id.as_str().parse().unwrap())
        .collect()
}

fn tag(s: &str) -> Tag {
    Tag::new(s).unwrap()
}

#[test]
fn list_skips_notes_missing_files_and_done_tasks_by_default() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    let open = store.list(&Filter::default()).unwrap();
    assert_eq!(
        ids(&open),
        vec![id::FULL, id::SUPPORT, id::WAITING, id::NO_TAGS]
    );
    let all = store.list(&Filter::default().any_done()).unwrap();
    assert_eq!(
        ids(&all),
        vec![id::FULL, id::SUPPORT, id::DONE, id::WAITING, id::NO_TAGS]
    );
    let done = store.list(&Filter::default().done(true)).unwrap();
    assert_eq!(ids(&done), vec![id::DONE]);
}

#[test]
fn list_applies_the_filter() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    assert_eq!(
        ids(&store
            .list(&Filter::default().status(Status::READY))
            .unwrap()),
        vec![id::SUPPORT]
    );
    assert_eq!(
        ids(&store.list(&Filter::default().tag(tag("gitlab"))).unwrap()),
        vec![id::FULL]
    );
    assert_eq!(
        ids(&store
            .list(&Filter::default().priority(Priority::B))
            .unwrap()),
        vec![id::SUPPORT, id::WAITING, id::NO_TAGS],
        "no priority tag means B"
    );
    assert_eq!(
        ids(&store.list(&Filter::default().no_status()).unwrap()),
        vec![id::NO_TAGS]
    );
    assert_eq!(
        ids(&store.list(&Filter::default().text("SECURITY")).unwrap()),
        vec![id::WAITING]
    );
}

#[test]
fn list_skips_a_todo_file_that_is_not_a_task() {
    let nb = NbEnv::fixture();
    nb.write(support::file_name(id::SUPPORT), "# Not a todo any more\n");
    let store = nb.open();
    let open = store.list(&Filter::default()).unwrap();
    assert_eq!(ids(&open), vec![id::FULL, id::WAITING, id::NO_TAGS]);
    // `get` is explicit about it.
    let err = store.get(&TaskId::from(id::SUPPORT)).unwrap_err();
    assert!(
        matches!(&err, StoreError::Format { path, .. } if *path == nb.file(id::SUPPORT)),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .contains("not a task: first line \"# Not a todo any more\"")
    );
}

#[test]
#[cfg(unix)]
fn list_propagates_read_errors_other_than_a_missing_file() {
    use std::os::unix::fs::PermissionsExt;
    if nix_is_root() {
        eprintln!("skipped: running as root, permissions do not apply");
        return;
    }
    let nb = NbEnv::fixture();
    let path = nb.file(id::SUPPORT);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let store = nb.open();
    let err = store.list(&Filter::default()).unwrap_err();
    assert!(
        matches!(&err, StoreError::Io { path: p, .. } if *p == path),
        "{err:?}"
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(store.list(&Filter::default()).unwrap().len(), 4);
}

#[cfg(unix)]
fn nix_is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

#[test]
fn list_sees_index_changes_made_after_open() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    assert_eq!(store.list(&Filter::default()).unwrap().len(), 4);
    // Somebody (nb) appends a todo.
    nb.write(
        "20260907150000.todo.md",
        "# [ ] Added later\n\n## Tags\n\n#B #ready\n",
    );
    let mut index = nb.read(".index");
    index.push_str("20260907150000.todo.md\n");
    nb.write(".index", &index);
    let open = store.list(&Filter::default()).unwrap();
    assert_eq!(
        ids(&open),
        vec![id::FULL, id::SUPPORT, id::WAITING, id::NO_TAGS, 8]
    );
    assert_eq!(open[4].title, "Added later");
    assert_eq!(store.get(&TaskId::from(8)).unwrap().title, "Added later");
}

#[test]
fn get_reads_the_whole_task() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    let task = store.get(&TaskId::from(id::FULL)).unwrap();
    let at = |s: &str| FixedClock::at(s).0;
    assert_eq!(task.id, TaskId::from(1));
    assert_eq!(task.title, "Rewrite the tasks script in Rust");
    assert!(!task.done);
    assert_eq!(task.status, Some(Status::IN_PROGRESS));
    assert_eq!(task.priority, Priority::A);
    assert_eq!(task.due, Some(FixedClock::at("2026-10-10 00:00").today()));
    assert_eq!(
        task.description.as_deref(),
        Some("Port the bash script to a Rust workspace.\nKeep the nb files unchanged.")
    );
    assert_eq!(task.project, Some(PathBuf::from("/home/pau/code/tasks")));
    assert_eq!(task.tags, vec![tag("gitlab")]);
    assert_eq!(
        task.related,
        vec![Link::new(
            "https://gitlab.example.invalid/group/project/-/issues/42"
        )]
    );
    assert_eq!(
        task.merge_requests,
        vec![Link::labelled(
            "https://gitlab.example.invalid/group/project/-/merge_requests/123",
            "Add parser"
        )]
    );
    assert_eq!(
        task.worktrees,
        vec![Worktree::on_branch(
            "/home/pau/code/tasks-wt/feature-a",
            "feature-a"
        )]
    );
    assert_eq!(
        task.sessions,
        vec![Session {
            at: at("2026-10-04 10:20"),
            id: "abc-123".into(),
            launcher: None,
            description: Some("first session".into()),
        }]
    );
    assert_eq!(
        task.progress,
        vec![
            ProgressEntry::dated(
                FixedClock::at("2025-03-01 00:00").today(),
                "legacy note without time"
            ),
            ProgressEntry::new(at("2026-10-04 10:15"), "created via tasks create"),
            ProgressEntry::new(at("2026-10-04 11:40"), "parser done"),
        ]
    );
    assert_eq!(task.origin, None);

    let done = store.get(&TaskId::from(id::DONE)).unwrap();
    assert!(done.done);
    assert_eq!(done.status, None);
    assert_eq!(done.priority, Priority::C);

    let no_tags = store.get(&TaskId::from(id::NO_TAGS)).unwrap();
    assert_eq!(no_tags.status, None);
    assert_eq!(no_tags.priority, Priority::B);
    assert_eq!(no_tags.tags, Vec::new());
}

#[test]
fn get_of_missing_or_non_todo_ids_is_not_found() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    for id in [id::NOTE, id::MISSING, 8, 0] {
        let err = store.get(&TaskId::from(id)).unwrap_err();
        assert!(
            matches!(&err, StoreError::NotFound(i) if *i == TaskId::from(id)),
            "{id}: {err:?}"
        );
        assert_eq!(err.to_string(), format!("no task with id {id}"));
    }
    let err = store.get(&TaskId::new("abc").unwrap()).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
    assert!(store.path_of(&TaskId::from(id::MISSING)).is_err());
    assert_eq!(
        store.path_of(&TaskId::from(id::FULL)).unwrap(),
        nb.file(id::FULL)
    );
}

#[test]
fn list_and_get_agree() {
    let nb = NbEnv::fixture();
    let store = nb.open();
    for task in store.list(&Filter::default().any_done()).unwrap() {
        assert_eq!(store.get(&task.id).unwrap(), task);
    }
}
