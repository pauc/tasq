//! SV-008: `migrate::plan` and `Plan::write` convert an nb notebook into a
//! store directory, read back through the `Store` trait.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use support::{CLOCK, TempStore, entries, is_root, local_millis, ulid_name};
use tasq_core::clock::FixedClock;
use tasq_core::model::{TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_core::store::Store;
use tasq_store::migrate::{self, Plan};
use tasq_store::ulid::Ulid;

/// The nb fixture notebook of the store-nb crate, read in place: the plan
/// never writes to the notebook.
fn notebook() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../store-nb/tests/fixtures/nb/home")
}

/// `YYYY-MM-DD HH:MM:SS`.
fn at(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}

/// The plan of `notebook` into `target` with the default workflow, the
/// clock at [`CLOCK`] and a random part counting up from 1.
fn plan_of(notebook: &Path, target: &Path) -> Result<Plan, migrate::MigrateError> {
    let mut next = 0;
    migrate::plan(
        notebook,
        target,
        &Workflow::default(),
        &FixedClock::at(CLOCK),
        &mut || {
            next += 1;
            next
        },
    )
}

/// A notebook in a temp directory: `.index` and the given files.
fn notebook_with(index: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".index"), index).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

/// The plan of `notebook` with the random part of every ULID taken from
/// `random`, in order (the last value repeats once the list is used up).
fn plan_with(
    notebook: &Path,
    target: &Path,
    random: &[u128],
) -> Result<Plan, migrate::MigrateError> {
    let mut queue: std::collections::VecDeque<u128> = random.iter().copied().collect();
    let last = *random.last().unwrap();
    migrate::plan(
        notebook,
        target,
        &Workflow::default(),
        &FixedClock::at(CLOCK),
        &mut || queue.pop_front().unwrap_or(last),
    )
}

/// The text the plan gives the task with nb id `id`.
fn text_of(plan: &Plan, id: u64) -> String {
    plan.entries()
        .iter()
        .find(|e| e.id == TaskId::from(id))
        .unwrap()
        .text()
        .to_owned()
}

/// Every file under `dir` with its bytes, sorted by path.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(snapshot(&path));
        } else {
            out.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    out.sort();
    out
}

fn ids(plan: &Plan) -> Vec<u64> {
    plan.entries()
        .iter()
        .map(|e| e.id.as_str().parse().unwrap())
        .collect()
}

#[test]
fn converting_the_fixture_gives_one_file_per_task_named_after_the_nb_second() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    assert_eq!(ids(&plan), vec![1, 3, 4, 6, 7]);
    let titles: Vec<&str> = plan.entries().iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Rewrite the tasks script in Rust",
            "Answer the support ticket",
            "Ship the release notes",
            "Wait for the security review",
            "Task without a tags section",
        ]
    );
    plan.write().unwrap();
    let store = temp.open();
    let tasks = store.list(&Filter::default().any_done()).unwrap();
    let created: Vec<(String, Option<NaiveDateTime>)> = tasks
        .iter()
        .map(|t| (t.id.to_string(), t.created_at))
        .collect();
    assert_eq!(
        created,
        vec![
            ("1".to_owned(), Some(at("2026-09-01 09:00:00"))),
            ("3".to_owned(), Some(at("2026-09-02 10:00:00"))),
            ("4".to_owned(), Some(at("2026-09-03 11:00:00"))),
            ("6".to_owned(), Some(at("2026-09-04 12:00:00"))),
            ("7".to_owned(), Some(at("2026-09-06 14:00:00"))),
        ]
    );
    let names: Vec<String> = plan.entries().iter().map(|e| e.file.clone()).collect();
    assert_eq!(entries(&temp.dir()), names);
    let expected: Vec<String> = created
        .iter()
        .zip(1u128..)
        .map(|((_, at), random)| ulid_name(local_millis(at.unwrap()), random))
        .collect();
    assert_eq!(names, expected);
    assert_eq!(
        store.get(&TaskId::from(4)).unwrap().title,
        "Ship the release notes"
    );
}

#[test]
fn an_open_task_keeps_its_status_tags_and_body() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    assert_eq!(
        text_of(&plan, 1),
        "---\n\
         id: 1\n\
         status: in-progress\n\
         priority: A\n\
         due: 2026-10-10\n\
         project: /home/pau/code/tasks\n\
         tags: [gitlab]\n\
         ---\n\
         # Rewrite the tasks script in Rust\n\
         \n\
         ## Description\n\
         \n\
         Port the bash script to a Rust workspace.\n\
         Keep the nb files unchanged.\n\
         \n\
         ## Related\n\
         \n\
         - https://gitlab.example.invalid/group/project/-/issues/42\n\
         \n\
         ### Merge requests\n\
         \n\
         - [Add parser](https://gitlab.example.invalid/group/project/-/merge_requests/123)\n\
         \n\
         ## Worktrees\n\
         \n\
         - /home/pau/code/tasks-wt/feature-a (`feature-a`)\n\
         \n\
         ## Sessions\n\
         \n\
         - 2026-10-04 10:20: `abc-123` — first session\n\
         \n\
         ## Progress\n\
         \n\
         - 2025-03-01: legacy note without time\n\
         - 2026-10-04 10:15: created via tasks create\n\
         - 2026-10-04 11:40: parser done\n"
    );
    assert_eq!(
        text_of(&plan, 3),
        "---\nid: 3\nstatus: ready\npriority: B\ntags: [support]\n---\n\
         # Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n\
         ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
    );
}

#[test]
fn a_done_task_becomes_status_done_with_its_closed_time() {
    let nb = notebook_with(
        "20260903110000.todo.md\n",
        &[(
            "20260903110000.todo.md",
            "# [x] Ship it\n\n## Tags\n\n#gitlab #C\n\n## Closed\n\n2026-10-03 16:30\n\n## Progress\n\n- 2026-10-03 11:00: created\n",
        )],
    );
    let temp = TempStore::absent();
    let plan = plan_of(nb.path(), &temp.dir()).unwrap();
    assert_eq!(
        text_of(&plan, 1),
        "---\nid: 1\nstatus: done\npriority: C\nclosed: 2026-10-03 16:30\ntags: [gitlab]\n---\n\
         # Ship it\n\n## Progress\n\n- 2026-10-03 11:00: created\n"
    );
    plan.write().unwrap();
    let task = temp.open().get(&TaskId::from(1)).unwrap();
    assert!(task.done);
    assert_eq!(task.status, None);
    assert_eq!(task.closed_at, Some(at("2026-10-03 16:30:00")));
}

#[test]
fn a_done_task_closed_without_a_time_is_closed_at_its_last_progress_entry() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    assert_eq!(
        text_of(&plan, 4),
        "---\nid: 4\nstatus: done\npriority: C\nclosed: 2026-10-03 16:30\ntags: [gitlab]\n---\n\
         # Ship the release notes\n\n## Progress\n\n\
         - 2026-10-03 11:00: created via tasks create\n- 2026-10-03 16:30: shipped\n"
    );
}

#[test]
fn a_done_task_with_a_dated_last_entry_is_closed_at_midnight_and_without_entries_not_at_all() {
    let nb = notebook_with(
        "dated.todo.md\nbare.todo.md\nopen.todo.md\n",
        &[
            (
                "dated.todo.md",
                "# [x] Dated\n\n## Progress\n\n- 2026-08-01 07:45: first\n- 2026-08-03: legacy\n",
            ),
            ("bare.todo.md", "# [x] Bare\n"),
            (
                "open.todo.md",
                "# [ ] Open\n\n## Progress\n\n- 2026-08-01 07:45: first\n",
            ),
        ],
    );
    let temp = TempStore::absent();
    plan_of(nb.path(), &temp.dir()).unwrap().write().unwrap();
    let closed: Vec<Option<NaiveDateTime>> = temp
        .open()
        .list(&Filter::default().any_done())
        .unwrap()
        .iter()
        .map(|t| t.closed_at)
        .collect();
    assert_eq!(closed, vec![Some(at("2026-08-03 00:00:00")), None, None]);
}

#[test]
fn a_todo_file_that_is_not_utf8_is_refused_naming_it() {
    let nb = notebook_with("a.todo.md\n", &[]);
    let path = nb.path().join("a.todo.md");
    std::fs::write(&path, b"# [ ] caf\xe9\n").unwrap();
    let temp = TempStore::absent();
    let error = plan_of(nb.path(), &temp.dir()).unwrap_err();
    assert!(matches!(error, migrate::MigrateError::Io { .. }), "{error}");
    assert_eq!(
        error.to_string(),
        format!(
            "{}: not valid UTF-8; the migration would replace the invalid bytes",
            path.display()
        )
    );
}

#[test]
fn sections_the_format_does_not_know_are_carried_over_verbatim() {
    let nb = notebook_with(
        "20260901090000.todo.md\n",
        &[(
            "20260901090000.todo.md",
            "# [ ] T\n\n## Notes\n\nkeep me\n\n  indented\n\n## Tags\n\n#ready\n\n## Links\n### Sub\n- x\n\n\n",
        )],
    );
    let temp = TempStore::absent();
    let plan = plan_of(nb.path(), &temp.dir()).unwrap();
    assert_eq!(
        text_of(&plan, 1),
        "---\nid: 1\nstatus: ready\npriority: B\n---\n# T\n\n## Progress\n\
         \n## Notes\n\nkeep me\n\n  indented\n\n## Links\n### Sub\n- x\n"
    );
}

#[test]
fn the_plan_writes_nothing_and_leaves_the_notebook_alone() {
    let temp = TempStore::absent();
    let before = snapshot(&notebook());
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    assert_eq!(plan.target(), temp.dir());
    assert!(!temp.dir().exists());
    assert_eq!(entries(temp.root_path()), Vec::<String>::new());
    plan.write().unwrap();
    assert_eq!(snapshot(&notebook()), before);
}

#[test]
fn an_empty_target_directory_is_accepted() {
    let temp = TempStore::empty();
    plan_of(&notebook(), &temp.dir()).unwrap().write().unwrap();
    assert_eq!(entries(&temp.dir()).len(), 5);
}

#[test]
fn a_target_that_is_not_empty_is_refused() {
    let temp = TempStore::empty();
    temp.write(".keep", "");
    let error = plan_of(&notebook(), &temp.dir()).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "{}: the target exists and is not empty",
            temp.dir().display()
        )
    );
    assert_eq!(entries(&temp.dir()), vec![".keep".to_owned()]);
}

#[test]
fn a_target_filled_after_the_plan_is_refused_by_the_write() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    temp.write("x.md", "mine");
    let error = plan.write().unwrap_err();
    assert!(
        matches!(error, migrate::MigrateError::TargetNotEmpty { .. }),
        "{error}"
    );
    assert_eq!(entries(&temp.dir()), vec!["x.md".to_owned()]);
}

#[test]
fn a_target_that_is_a_file_is_an_error_naming_it() {
    let temp = TempStore::absent();
    std::fs::write(temp.dir(), "a file").unwrap();
    let error = plan_of(&notebook(), &temp.dir()).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}: ", temp.dir().display())),
        "{error}"
    );
    assert!(matches!(error, migrate::MigrateError::Io { .. }), "{error}");
}

#[test]
fn a_directory_without_an_index_is_not_a_notebook() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("20260901090000.todo.md"), "# [ ] T\n").unwrap();
    let temp = TempStore::absent();
    let error = plan_of(dir.path(), &temp.dir()).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{}: not an nb notebook (no .index)", dir.path().display())
    );
}

#[test]
fn an_unreadable_index_is_an_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".index")).unwrap();
    let temp = TempStore::absent();
    let error = plan_of(dir.path(), &temp.dir()).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}: ", dir.path().join(".index").display())),
        "{error}"
    );
}

#[test]
fn todo_files_the_index_does_not_list_follow_its_last_line_in_name_order() {
    let nb = notebook_with(
        "a.todo.md\r\nnotes.md\n\n",
        &[
            ("a.todo.md", "# [ ] A\n"),
            ("notes.md", "# Notes\n"),
            ("c.todo.md", "# [ ] C\n"),
            ("b.todo.md", "# [ ] B\n"),
            ("x.md", "# [ ] not a todo\n"),
            (".todo.md", "# [ ] no name\n"),
        ],
    );
    let temp = TempStore::absent();
    let plan = plan_of(nb.path(), &temp.dir()).unwrap();
    let mapping: Vec<(u64, &str)> = plan
        .entries()
        .iter()
        .map(|e| (e.id.as_str().parse().unwrap(), e.title.as_str()))
        .collect();
    assert_eq!(mapping, vec![(1, "A"), (4, "B"), (5, "C")]);
    assert_eq!(plan.entries()[1].from, nb.path().join("b.todo.md"));
}

#[test]
fn missing_files_directories_and_non_tasks_are_skipped() {
    let nb = notebook_with(
        "gone.todo.md\ndir.todo.md\nplain.todo.md\nok.todo.md\n",
        &[
            ("plain.todo.md", "just text\n"),
            ("ok.todo.md", "# [ ] Ok\n"),
        ],
    );
    std::fs::create_dir(nb.path().join("dir.todo.md")).unwrap();
    let temp = TempStore::absent();
    let plan = plan_of(nb.path(), &temp.dir()).unwrap();
    assert_eq!(ids(&plan), vec![4]);
}

#[test]
fn a_todo_file_that_does_not_parse_is_an_error_naming_it() {
    let nb = notebook_with(
        "bad.todo.md\n",
        &[("bad.todo.md", "---\nid: x\n---\n# T\n")],
    );
    let temp = TempStore::absent();
    let error = plan_of(nb.path(), &temp.dir()).unwrap_err();
    assert!(
        matches!(error, migrate::MigrateError::Format { .. }),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}: ", nb.path().join("bad.todo.md").display())),
        "{error}"
    );
}

#[test]
fn an_unreadable_todo_file_is_an_error_naming_it() {
    if is_root() {
        return;
    }
    let nb = notebook_with("a.todo.md\n", &[("a.todo.md", "# [ ] A\n")]);
    let path = nb.path().join("a.todo.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let temp = TempStore::absent();
    let error = plan_of(nb.path(), &temp.dir()).unwrap_err();
    assert!(matches!(error, migrate::MigrateError::Io { .. }), "{error}");
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}: ", path.display())),
        "{error}"
    );
}

#[test]
fn without_a_timestamp_name_the_time_is_the_first_progress_entry_then_the_clock() {
    let nb = notebook_with(
        "timed.todo.md\ndated.todo.md\nbare.todo.md\n2026090109000x.todo.md\n20261301090000.todo.md\n",
        &[
            (
                "timed.todo.md",
                "# [ ] Timed\n\n## Progress\n\n- 2026-08-01 07:45: first\n- 2026-08-02 08:00: second\n",
            ),
            (
                "dated.todo.md",
                "# [ ] Dated\n\n## Progress\n\n- 2026-08-03: legacy\n",
            ),
            ("bare.todo.md", "# [ ] Bare\n"),
            ("2026090109000x.todo.md", "# [ ] Not digits\n"),
            ("20261301090000.todo.md", "# [ ] Not a date\n"),
        ],
    );
    let temp = TempStore::absent();
    let plan = plan_of(nb.path(), &temp.dir()).unwrap();
    let created: Vec<NaiveDateTime> = plan.entries().iter().map(|e| e.created).collect();
    let now = FixedClock::at(CLOCK).0;
    assert_eq!(
        created,
        vec![
            at("2026-08-01 07:45:00"),
            at("2026-08-03 00:00:00"),
            now,
            now,
            now
        ]
    );
    let names: Vec<String> = plan.entries().iter().map(|e| e.file.clone()).collect();
    assert_eq!(
        names,
        vec![
            ulid_name(local_millis(at("2026-08-01 07:45:00")), 1),
            ulid_name(local_millis(at("2026-08-03 00:00:00")), 2),
            ulid_name(local_millis(now), 3),
            ulid_name(local_millis(now), 4),
            ulid_name(local_millis(now), 5),
        ]
    );
}

#[test]
fn a_name_the_plan_already_holds_is_minted_again() {
    let nb = notebook_with(
        "a.todo.md\nb.todo.md\n",
        &[("a.todo.md", "# [ ] A\n"), ("b.todo.md", "# [ ] B\n")],
    );
    let temp = TempStore::absent();
    let plan = plan_with(nb.path(), &temp.dir(), &[7, 7, 7, 8]).unwrap();
    let millis = local_millis(FixedClock::at(CLOCK).0);
    let names: Vec<String> = plan.entries().iter().map(|e| e.file.clone()).collect();
    assert_eq!(names, vec![ulid_name(millis, 7), ulid_name(millis, 8)]);
}

#[test]
fn a_name_taken_on_every_attempt_is_an_error_after_the_bound() {
    let nb = notebook_with(
        "a.todo.md\nb.todo.md\n",
        &[("a.todo.md", "# [ ] A\n"), ("b.todo.md", "# [ ] B\n")],
    );
    let temp = TempStore::absent();
    let mut calls = 0;
    let error = migrate::plan(
        nb.path(),
        &temp.dir(),
        &Workflow::default(),
        &FixedClock::at(CLOCK),
        &mut || {
            calls += 1;
            7
        },
    )
    .unwrap_err();
    assert_eq!(calls, 1 + tasq_store::store::MAX_NAME_ATTEMPTS);
    assert_eq!(
        error.to_string(),
        format!(
            "{}: no free file name after 8 attempts",
            nb.path().join("b.todo.md").display()
        )
    );
}

#[test]
fn a_write_that_cannot_create_the_target_is_an_error_naming_it() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.file("deeper")).unwrap();
    std::fs::write(temp.dir(), "a file").unwrap();
    let error = plan.write().unwrap_err();
    assert!(matches!(error, migrate::MigrateError::Io { .. }), "{error}");
}

/// The committed v2 fixture store, `<data_dir>/<store.name>` with the
/// default name, which the CLI tests copy.
fn fixture_store() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stores/default")
}

/// The name the fixture gives `entry`, the `random`-th task: a ULID of the
/// nb second read as UTC (what `migrate` writes with `TZ=UTC`), so the
/// committed names do not depend on the zone of the machine that made them.
fn fixture_name(entry: &migrate::Entry, random: u128) -> String {
    let millis = u64::try_from(entry.created.and_utc().timestamp_millis()).unwrap();
    format!("{}.md", Ulid::from_parts(millis, random))
}

/// The fixture store is the migration of the nb fixture notebook, file for
/// file. `TASQ_UPDATE_FIXTURES=1` rewrites it from the plan instead.
#[test]
fn the_committed_fixture_store_is_the_migrated_nb_fixture() {
    let temp = TempStore::absent();
    let plan = plan_of(&notebook(), &temp.dir()).unwrap();
    let expected: Vec<(String, String)> = plan
        .entries()
        .iter()
        .zip(1u128..)
        .map(|(entry, random)| (fixture_name(entry, random), entry.text().to_owned()))
        .collect();
    if std::env::var_os("TASQ_UPDATE_FIXTURES").is_some() {
        let dir = fixture_store();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in &expected {
            std::fs::write(dir.join(name), text).unwrap();
        }
    }
    let committed: Vec<(String, String)> = entries(&fixture_store())
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(fixture_store().join(&name)).unwrap();
            (name, text)
        })
        .collect();
    assert_eq!(committed, expected);
    let store = tasq_store::TasqStore::new(fixture_store(), Workflow::default());
    let ids: Vec<String> = store
        .list(&Filter::default().any_done())
        .unwrap()
        .iter()
        .map(|t| t.id.to_string())
        .collect();
    assert_eq!(ids, vec!["1", "3", "4", "6", "7"]);
}
