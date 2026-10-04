//! T-203: `create` on a temp copy of the fixture, with the native, a
//! recording and (nb-gated) the nb bookkeeper.

mod support;

use std::cell::RefCell;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use chrono::NaiveDate;
use tasq_core::clock::FixedClock;
use tasq_core::config::Bookkeeper as Choice;
use tasq_core::model::{Link, Priority, Status, Tag, TaskDraft, TaskId};
use tasq_store_nb::{
    Bookkeeper, NbStore, Store, StoreError, StoreWarning, SyncOutcome, Verification,
};

use support::{NbEnv, id, nb_ids, nb_or_skip};

const CLOCK: &str = "2026-10-04 12:34";
const NEW_FILE: &str = "20261004123400.todo.md";

fn store_at(nb: &NbEnv, choice: Choice, clock: &str) -> NbStore {
    nb.open_with(choice)
        .with_clock(Box::new(FixedClock::at(clock)))
}

fn full_draft() -> TaskDraft {
    TaskDraft::new("Review the new store")
        .with_description("Check create end to end.")
        .with_project("/home/pau/code/tasks")
        .with_due(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap())
        .with_related(Link::new("https://example.invalid/spec"))
        .with_tag(Tag::new("gitlab").unwrap())
        .with_priority(Priority::A)
        .with_merge_request(Link::labelled(
            "https://gitlab.example.invalid/g/p/-/merge_requests/1",
            "Add store",
        ))
        .with_note("first note")
}

#[test]
fn create_writes_the_script_shaped_file_and_registers_it() {
    let nb = NbEnv::fixture();
    let mut store = store_at(&nb, Choice::Native, CLOCK);
    let index_before = nb.read(".index");
    let before = nb.mtimes();
    let task = store.create(full_draft()).unwrap();
    assert_eq!(task.id, TaskId::from(8), "appended as line 8");
    assert_eq!(
        nb.read(NEW_FILE),
        "# [ ] Review the new store\n\
         \n\
         ## Description\n\
         \n\
         Check create end to end.\n\
         \n\
         ## Project\n\
         \n\
         /home/pau/code/tasks\n\
         \n\
         ## Due\n\
         \n\
         2026-10-10\n\
         \n\
         ## Related\n\
         \n\
         - https://example.invalid/spec\n\
         \n\
         ### Merge requests\n\
         \n\
         - [Add store](https://gitlab.example.invalid/g/p/-/merge_requests/1)\n\
         \n\
         ## Tags\n\
         \n\
         #gitlab #A #ready\n\
         \n\
         ## Progress\n\
         \n\
         - 2026-10-04 12:34: first note\n"
    );
    assert_eq!(nb.read(".index"), format!("{index_before}{NEW_FILE}\n"));
    assert_eq!(store.get(&task.id).unwrap(), task);
    assert_eq!(task.title, "Review the new store");
    assert_eq!(task.status, Some(Status::READY));
    assert_eq!(task.priority, Priority::A);
    assert_eq!(task.merge_requests.len(), 1);
    assert_eq!(task.progress[0].note, "first note");
    // Only the new file and the index changed; no temp file left behind.
    let after = nb.mtimes();
    for (name, was) in &before {
        if name != ".index" {
            let now = after.iter().find(|(n, _)| n == name).unwrap().1;
            assert_eq!(*was, now, "{name} was touched");
        }
    }
    assert_eq!(after.len(), before.len() + 1);
    assert!(after.iter().all(|(n, _)| !n.starts_with(".tasq-")));
    assert_eq!(store.warnings(), [], "no git repository: nothing to commit");
    assert_eq!(
        store.describe().task_count,
        6,
        "five existing fixture todos plus ours"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(nb.notebook().join(NEW_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644);
    }
}

#[test]
fn create_defaults_the_note_and_writes_done_drafts_closed() {
    let nb = NbEnv::fixture();
    let mut store = store_at(&nb, Choice::Native, CLOCK);
    let plain = store.create(TaskDraft::new("Plain")).unwrap();
    assert_eq!(
        nb.read(NEW_FILE),
        "# [ ] Plain\n\n## Tags\n\n#B #ready\n\n## Progress\n\n- 2026-10-04 12:34: created via tasq create\n"
    );
    assert_eq!(plain.progress[0].note, "created via tasq create");

    let done = store
        .create(
            TaskDraft::new("Already done")
                .with_done(true)
                .with_status(Some(Status::IN_PROGRESS))
                .with_tag(Tag::new("ops").unwrap()),
        )
        .unwrap();
    assert_eq!(done.id, TaskId::from(9));
    assert!(done.done);
    assert_eq!(done.status, None, "done drafts carry no status");
    assert_eq!(
        nb.read("20261004123401.todo.md"),
        "# [x] Already done\n\n## Tags\n\n#ops #B\n\n## Progress\n\n- 2026-10-04 12:34: created via tasq create\n"
    );
    // A status other than the default and no priority change.
    let waiting = store
        .create(TaskDraft::new("Later").with_status(Some(Status::LATER)))
        .unwrap();
    assert_eq!(waiting.status, Some(Status::LATER));
    assert!(nb.read("20261004123402.todo.md").contains("\n#B #later\n"));
}

#[test]
fn create_bumps_the_filename_by_one_second_when_taken() {
    let nb = NbEnv::fixture();
    nb.write(NEW_FILE, "# [ ] Squatter\n");
    let mut store = store_at(&nb, Choice::Native, CLOCK);
    let task = store.create(TaskDraft::new("Bumped")).unwrap();
    assert_eq!(nb.read(NEW_FILE), "# [ ] Squatter\n", "untouched");
    assert!(
        nb.read("20261004123401.todo.md")
            .starts_with("# [ ] Bumped\n")
    );
    assert_eq!(task.title, "Bumped");
    assert!(nb.read(".index").ends_with("\n20261004123401.todo.md\n"));

    // Every second of the minute taken: an error, nothing written.
    for second in 2..60 {
        nb.write(&format!("202610041234{second:02}.todo.md"), "");
    }
    let index_before = nb.read(".index");
    let err = store.create(TaskDraft::new("No room")).unwrap_err();
    assert!(
        matches!(&err, StoreError::Io { path, source }
            if path == &nb.notebook().join(NEW_FILE)
            && source.kind() == std::io::ErrorKind::AlreadyExists),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "{}: no free todo filename within 60 seconds",
            nb.notebook().join(NEW_FILE).display()
        )
    );
    assert_eq!(nb.read(".index"), index_before);
}

#[test]
fn create_refuses_merge_requests_without_a_title() {
    let nb = NbEnv::fixture();
    let mut store = store_at(&nb, Choice::Native, CLOCK);
    let before = nb.mtimes();
    let err = store
        .create(TaskDraft::new("MR").with_merge_request(Link::new("https://x/mr/1")))
        .unwrap_err();
    assert!(matches!(err, StoreError::Unsupported { .. }), "{err:?}");
    assert!(err.to_string().contains("https://x/mr/1 without a title"));
    assert_eq!(nb.mtimes(), before, "nothing written");
}

/// A bookkeeper that writes the index itself, in a configurable way, and
/// records its calls.
struct Scripted {
    calls: Rc<RefCell<Vec<String>>>,
    register: fn(&Path) -> Result<(), StoreError>,
}

impl Bookkeeper for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn register(&self, file: &Path) -> Result<(), StoreError> {
        self.calls.borrow_mut().push(format!(
            "register {}",
            file.file_name().unwrap().to_string_lossy()
        ));
        (self.register)(file)
    }
    fn checkpoint(&self, message: &str) -> Result<bool, StoreError> {
        self.calls
            .borrow_mut()
            .push(format!("checkpoint {message}"));
        Ok(true)
    }
    fn verify(&self) -> Result<Verification, StoreError> {
        Ok(Verification::consistent("scripted"))
    }
    fn sync(&self) -> Result<SyncOutcome, StoreError> {
        Ok(SyncOutcome::skipped("scripted"))
    }
}

fn scripted(
    store: NbStore,
    register: fn(&Path) -> Result<(), StoreError>,
) -> (NbStore, Rc<RefCell<Vec<String>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let store = store.with_bookkeeper(Box::new(Scripted {
        calls: Rc::clone(&calls),
        register,
    }));
    (store, calls)
}

#[allow(clippy::unnecessary_wraps)]
fn append_twice(file: &Path) -> Result<(), StoreError> {
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let mut index = std::fs::OpenOptions::new()
        .append(true)
        .open(file.parent().unwrap().join(".index"))
        .unwrap();
    write!(index, "{name}\n{name}\n").unwrap();
    Ok(())
}

fn register_fails(_file: &Path) -> Result<(), StoreError> {
    Err(StoreError::Bookkeeping {
        message: "nb index add exploded".into(),
    })
}

#[allow(clippy::unnecessary_wraps)]
fn register_forgets(_file: &Path) -> Result<(), StoreError> {
    Ok(())
}

#[test]
fn create_takes_the_last_index_line_naming_the_file_and_checkpoints_after() {
    let nb = NbEnv::fixture();
    let (mut store, calls) = scripted(store_at(&nb, Choice::Native, CLOCK), append_twice);
    let task = store.create(TaskDraft::new("Twice")).unwrap();
    assert_eq!(
        task.id,
        TaskId::from(9),
        "lines 8 and 9 both name it; the last wins"
    );
    assert_eq!(
        *calls.borrow(),
        vec![
            format!("register {NEW_FILE}"),
            format!("checkpoint [tasq] Add: {NEW_FILE}"),
        ]
    );
    assert_eq!(store.warnings(), []);
}

#[test]
fn create_reports_a_failed_registration_without_hiding_the_write() {
    let nb = NbEnv::fixture();
    let (mut store, calls) = scripted(store_at(&nb, Choice::Native, CLOCK), register_fails);
    let err = store.create(TaskDraft::new("Kept")).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "bookkeeping failed after the write: {NEW_FILE} was written but not added to .index: bookkeeping failed after the write: nb index add exploded"
        )
    );
    assert!(
        nb.read(NEW_FILE).starts_with("# [ ] Kept\n"),
        "the file stays"
    );
    assert_eq!(calls.borrow().len(), 1, "no checkpoint without an id");

    let (mut store, _) = scripted(store_at(&nb, Choice::Native, CLOCK), register_forgets);
    let err = store.create(TaskDraft::new("Forgotten")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "bookkeeping failed after the write: 20261004123401.todo.md was written but .index does not list it; run 'nb index reconcile' in the notebook"
    );
}

#[test]
fn create_in_a_git_notebook_commits_as_nb_would() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let mut store = store_at(&nb, Choice::Native, CLOCK);
    let task = store.create(TaskDraft::new("Committed")).unwrap();
    assert_eq!(task.id, TaskId::from(8));
    assert_eq!(
        nb.git_subjects(),
        vec![
            format!("[tasq] Add: {NEW_FILE}"),
            "[nb] Initialize".to_owned()
        ]
    );
    assert_eq!(nb.git_status(), "", "index and file both committed");
    assert_eq!(store.warnings(), []);
    // Nothing changed since: the checkpoint is skipped.
    assert!(!(store.bookkeeper().checkpoint("[tasq] noop").unwrap()));
    assert_eq!(nb.git_subjects().len(), 2);
}

#[test]
fn checkpoint_failure_after_create_is_a_warning() {
    let nb = NbEnv::fixture();
    nb.git_init();
    // Break committing: a HOME without an identity.
    let mut options = nb.options().with_bookkeeper(Choice::Native);
    let bare_home = nb.root.path().join("bare-home");
    std::fs::create_dir_all(&bare_home).unwrap();
    for (k, v) in &mut options.env {
        if k == "HOME" {
            *v = bare_home.display().to_string();
        }
    }
    let mut store = NbStore::open_dir(nb.notebook(), &options)
        .unwrap()
        .with_clock(Box::new(FixedClock::at(CLOCK)));
    let task = store.create(TaskDraft::new("Unsigned")).unwrap();
    assert_eq!(task.id, TaskId::from(8), "the task exists");
    let warnings = store.take_warnings();
    assert_eq!(warnings.len(), 1);
    match &warnings[0] {
        StoreWarning::Bookkeeping { message } => {
            assert!(message.starts_with("bookkeeping failed after the write: committing: git commit -q -m [tasq] Add: 20261004123400.todo.md failed"), "{message}");
            assert!(
                message.ends_with("; commit by hand with 'git add -A && git commit'"),
                "{message}"
            );
        }
        other @ StoreWarning::RebuiltIndex { .. } => panic!("{other:?}"),
    }
    assert_eq!(nb.git_subjects(), vec!["[nb] Initialize".to_owned()]);
}

#[test]
fn nb_lists_the_created_task_with_the_same_id() {
    if !nb_or_skip("nb_lists_the_created_task_with_the_same_id") {
        return;
    }
    let nb = NbEnv::fixture();
    nb.restore_missing_file();
    nb.git_init();
    let mut store = store_at(&nb, Choice::Nb, CLOCK);
    assert_eq!(store.bookkeeper().name(), "nb");
    let task = store.create(full_draft()).unwrap();
    assert_eq!(task.id, TaskId::from(8));
    assert_eq!(store.warnings(), []);
    assert_eq!(
        nb.git_subjects(),
        vec![
            format!("[tasq] Add: {NEW_FILE}"),
            "[nb] Initialize".to_owned()
        ],
        "nb committed with our message, synchronously"
    );
    assert_eq!(nb.git_status(), "");
    let listed = nb_ids(&nb.nb(&["home:todos", "open", "--no-color"]));
    assert!(listed.contains(&8), "{listed:?}");
    assert_eq!(
        listed.len(),
        6,
        "the five open fixture todos (incl. the restored one) plus ours"
    );
    nb.nb(&["index", "verify", &nb.notebook().display().to_string()]);
    let verification = store.bookkeeper().verify().unwrap();
    assert!(verification.consistent, "{verification:?}");
    // `nb todos` lists the same text we read back.
    let shown = nb.nb(&["home:show", "8", "--print", "--no-color"]);
    assert_eq!(shown.trim_end(), nb.read(NEW_FILE).trim_end());
    assert_eq!(store.get(&TaskId::from(8)).unwrap(), task);
    // Create a done one through nb's bookkeeper too: id 9, closed.
    let done = store
        .create(TaskDraft::new("Closed").with_done(true))
        .unwrap();
    assert_eq!(done.id, TaskId::from(id::NO_TAGS + 2));
    let open_ids = nb_ids(&nb.nb(&["home:todos", "open", "--no-color"]));
    assert!(!open_ids.contains(&9));
    let all_ids = nb_ids(&nb.nb(&["home:todos", "--no-color"]));
    assert!(all_ids.contains(&9), "{all_ids:?}");
}

#[test]
fn native_index_line_matches_nb_index_add_byte_for_byte() {
    if !nb_or_skip("native_index_line_matches_nb_index_add_byte_for_byte") {
        return;
    }
    // Two copies: nb registers in one, the native bookkeeper in the other.
    let with_nb = NbEnv::fixture();
    let native = NbEnv::fixture();
    for env in [&with_nb, &native] {
        env.write(NEW_FILE, "# [ ] Same\n");
    }
    with_nb.nb(&[
        "index",
        "add",
        NEW_FILE,
        &with_nb.notebook().display().to_string(),
    ]);
    let store = native.open_with(Choice::Native);
    store
        .bookkeeper()
        .register(&native.notebook().join(NEW_FILE))
        .unwrap();
    assert_eq!(
        std::fs::read(native.notebook().join(".index")).unwrap(),
        std::fs::read(with_nb.notebook().join(".index")).unwrap()
    );
    // Also when the previous line lacks its newline: nb does not repair it.
    for env in [&with_nb, &native] {
        env.write(".index", "first.todo.md");
        env.write("first.todo.md", "");
    }
    with_nb.nb(&[
        "index",
        "add",
        NEW_FILE,
        &with_nb.notebook().display().to_string(),
    ]);
    store
        .bookkeeper()
        .register(&native.notebook().join(NEW_FILE))
        .unwrap();
    assert_eq!(
        native.read(".index"),
        format!("first.todo.md{NEW_FILE}\n"),
        "both append name + newline and nothing else"
    );
    assert_eq!(native.read(".index"), with_nb.read(".index"));
}
