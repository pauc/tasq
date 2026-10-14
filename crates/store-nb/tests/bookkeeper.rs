//! ADR-0023: the native bookkeeper on temporary git repositories. Commits
//! run in a background job behind `.git/tasq/lock`, sync every
//! `sync_interval` seconds, and `nb` is never run.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::time::{Duration, Instant};

use tasq_core::clock::FixedClock;
use tasq_core::model::{Priority, TaskDraft, TaskId, Workflow};
use tasq_store_nb::job::{STALE_AFTER, State, instant, unix_time};
use tasq_store_nb::{
    Bookkeeper, NativeBookkeeper, NbStore, NbStoreOptions, Store, StoreError, StoreWarning,
};

use support::{NbEnv, fake_calls, fake_tool, file_name, id, slow_git};

/// The bookkeeper's clock: whatever the wall clock says, these tests run
/// at this time.
fn clock() -> FixedClock {
    FixedClock::at("2026-10-07 09:30")
}

/// Writes `notes.md` under the lock and hands the lock to a checkpoint, as
/// the store does, then waits for the job.
fn write_and_commit(nb: &NbEnv, b: &NativeBookkeeper, text: &str, message: &str) -> bool {
    let lock = b.lock(&clock()).unwrap();
    nb.write("notes.md", text);
    let started = b.checkpoint(lock, message, &clock()).unwrap();
    drop(b.lock(&clock()).unwrap());
    started
}

fn hook(nb: &NbEnv, name: &str, body: &str) {
    let hooks = nb.notebook().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join(name);
    std::fs::write(&hook, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perms = std::fs::metadata(&hook).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&hook, perms).unwrap();
}

#[test]
fn notebooks_without_git_get_no_lock_and_no_job() {
    let nb = NbEnv::fixture();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    assert_eq!(b.name(), "native");
    let lock = b.lock(&clock()).unwrap();
    assert_eq!(lock.path(), None);
    assert!(!(b.checkpoint(lock, "[tasq] Update: x", &clock()).unwrap()));
    assert!(!nb.notebook().join(".git").exists(), "nothing created");
    assert_eq!(b.take_failure(), None);
}

#[test]
fn checkpoint_commits_in_the_background_and_releases_the_lock() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    let lock = b.lock(&clock()).unwrap();
    let lock_file = nb.notebook().join(".git/tasq/lock");
    assert_eq!(lock.path(), Some(lock_file.as_path()));
    assert!(lock_file.is_file());
    nb.write("notes.md", "# Notes\n\nchanged\n");
    assert!(
        b.checkpoint(lock, "[tasq] Update: notes.md", &clock())
            .unwrap()
    );
    drop(b.lock(&clock()).unwrap());
    assert!(!lock_file.exists(), "the job and the wait both release it");
    assert_eq!(
        nb.git_subjects(),
        vec![
            "[tasq] Update: notes.md".to_owned(),
            "[nb] Initialize".to_owned()
        ]
    );
    assert_eq!(nb.git_status(), "");
    // A new untracked file is added too.
    let lock = b.lock(&clock()).unwrap();
    nb.write("fresh.todo.md", "# [ ] Fresh\n");
    assert!(
        b.checkpoint(lock, "[tasq] Add: fresh.todo.md", &clock())
            .unwrap()
    );
    drop(b.lock(&clock()).unwrap());
    assert_eq!(nb.git_status(), "");
    assert_eq!(nb.git_subjects().len(), 3);
    // A change git ignores commits nothing and is not a failure.
    let lock = b.lock(&clock()).unwrap();
    nb.write(".gitignore", "ignored.md\n");
    b.checkpoint(lock, "[tasq] Update: .gitignore", &clock())
        .unwrap();
    drop(b.lock(&clock()).unwrap());
    let lock = b.lock(&clock()).unwrap();
    nb.write("ignored.md", "# Ignored\n");
    b.checkpoint(lock, "[tasq] Update: ignored.md", &clock())
        .unwrap();
    drop(b.lock(&clock()).unwrap());
    assert_eq!(nb.git_subjects().len(), 4);
    assert_eq!(b.take_failure(), None);
}

#[test]
fn checkpoint_does_not_wait_for_git() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let slow = slow_git(1);
    let b = NativeBookkeeper::new(nb.notebook(), nb.env_with_path_first(slow.path()));
    let lock = b.lock(&clock()).unwrap();
    nb.write("notes.md", "# Notes\n\nslow\n");
    let started = Instant::now();
    assert!(
        b.checkpoint(lock, "[tasq] Update: notes.md", &clock())
            .unwrap()
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "returned after {:?}",
        started.elapsed()
    );
    assert_eq!(nb.git_subjects(), vec!["[nb] Initialize".to_owned()]);
    drop(b.lock(&clock()).unwrap());
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "the next lock waits"
    );
    assert_eq!(nb.git_subjects()[0], "[tasq] Update: notes.md");
    assert_eq!(fake_calls(&slow)[0], "add -A");
}

#[test]
fn a_rejected_commit_is_reported_once() {
    let nb = NbEnv::fixture();
    nb.git_init();
    hook(&nb, "pre-commit", "echo 'hook says no' >&2\nexit 1");
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    assert!(write_and_commit(
        &nb,
        &b,
        "# Notes\n\nchanged\n",
        "[tasq] Update: notes.md"
    ));
    assert_eq!(nb.git_subjects(), vec!["[nb] Initialize".to_owned()]);
    assert_eq!(
        b.take_failure().as_deref(),
        Some("committing: hook says no")
    );
    assert_eq!(b.take_failure(), None);
}

#[test]
fn writes_sync_at_most_once_per_interval() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let remote = nb.add_bare_remote();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone()).with_sync_interval(3600);
    let state = State::new(&nb.notebook());
    assert_eq!(state.last_sync(), None);
    let now = unix_time(&clock());
    write_and_commit(&nb, &b, "# Notes\n\none\n", "[tasq] Update: one");
    assert_eq!(
        nb.subjects_of(&remote)[0],
        "[tasq] Update: one",
        "never synced: due"
    );
    assert_eq!(state.last_sync(), Some(now), "the job records the clock");
    write_and_commit(&nb, &b, "# Notes\n\ntwo\n", "[tasq] Update: two");
    assert_eq!(nb.git_subjects()[0], "[tasq] Update: two");
    assert_eq!(
        nb.subjects_of(&remote)[0],
        "[tasq] Update: one",
        "synced at the same time: not due"
    );
    // Days before the wall clock, but one second short of the interval by
    // the bookkeeper's clock: not due.
    state.record_sync(now - 3599).unwrap();
    write_and_commit(&nb, &b, "# Notes\n\nthree\n", "[tasq] Update: three");
    assert_eq!(nb.subjects_of(&remote)[0], "[tasq] Update: one");
    assert_eq!(state.last_sync(), Some(now - 3599), "no attempt recorded");
    state.record_sync(now - 3600).unwrap();
    write_and_commit(&nb, &b, "# Notes\n\nfour\n", "[tasq] Update: four");
    assert_eq!(
        nb.subjects_of(&remote)[..4],
        [
            "[tasq] Update: four".to_owned(),
            "[tasq] Update: three".to_owned(),
            "[tasq] Update: two".to_owned(),
            "[tasq] Update: one".to_owned()
        ]
    );
    assert_eq!(state.last_sync(), Some(now));
    assert_eq!(b.take_failure(), None);
}

#[test]
fn interval_zero_never_syncs_and_no_remote_records_nothing() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let state = State::new(&nb.notebook());
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone()).with_sync_interval(1);
    write_and_commit(&nb, &b, "# Notes\n\nlocal\n", "[tasq] Update: local");
    assert_eq!(state.last_sync(), None, "no remote, no sync attempt");
    let remote = nb.add_bare_remote();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    write_and_commit(&nb, &b, "# Notes\n\nnever\n", "[tasq] Update: never");
    assert_eq!(nb.subjects_of(&remote)[0], "[tasq] Update: local");
    assert_eq!(state.last_sync(), None);
}

#[test]
fn a_failed_pull_is_reported_and_leaves_no_rebase_behind() {
    let nb = NbEnv::fixture();
    nb.git_init();
    nb.add_bare_remote();
    nb.git(&[
        "remote",
        "set-url",
        "origin",
        &nb.root.path().join("gone.git").display().to_string(),
    ]);
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone()).with_sync_interval(3600);
    write_and_commit(&nb, &b, "# Notes\n\nlocal\n", "[tasq] Update: local");
    assert_eq!(
        nb.git_subjects()[0],
        "[tasq] Update: local",
        "committed anyway"
    );
    let failure = b.take_failure().expect("reported");
    assert!(failure.starts_with("pulling: "), "{failure}");
    assert!(!failure.contains('\n'), "one line: {failure}");
    assert!(!nb.notebook().join(".git/rebase-merge").exists());
    assert!(!nb.notebook().join(".git/rebase-apply").exists());
}

#[test]
fn a_rejected_push_is_reported() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let remote = nb.add_bare_remote();
    let hooks = remote.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(
        hooks.join("pre-receive"),
        "#!/bin/sh\necho 'read only' >&2\nexit 1\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(hooks.join("pre-receive"))
        .unwrap()
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(hooks.join("pre-receive"), perms).unwrap();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone()).with_sync_interval(3600);
    write_and_commit(&nb, &b, "# Notes\n\nlocal\n", "[tasq] Update: local");
    let failure = b.take_failure().expect("reported");
    assert!(failure.starts_with("pushing: "), "{failure}");
    assert!(failure.contains("read only"), "{failure}");
}

#[test]
fn lock_waits_for_a_running_job_and_breaks_a_stale_one() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    let state = State::new(&nb.notebook());
    let held = state.acquire(&clock()).unwrap();
    let release = std::thread::spawn({
        let held = held.clone();
        move || {
            std::thread::sleep(Duration::from_millis(200));
            std::fs::remove_file(held).unwrap();
        }
    });
    let started = Instant::now();
    drop(b.lock(&clock()).unwrap());
    assert!(started.elapsed() >= Duration::from_millis(200));
    release.join().unwrap();

    let held = state.acquire(&clock()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&held)
        .unwrap()
        .set_modified(instant(&clock()) - STALE_AFTER)
        .unwrap();
    let started = Instant::now();
    drop(b.lock(&clock()).unwrap());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a lock stale by the bookkeeper's clock is broken"
    );
}

#[test]
fn verify_reports_the_fixture_gap_and_recovers() {
    let nb = NbEnv::fixture();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    let v = b.verify().unwrap();
    assert!(!v.consistent);
    assert_eq!(v.missing_files, vec![file_name(id::MISSING)]);
    assert_eq!(v.unindexed_files, Vec::<String>::new());
    assert_eq!(
        v.detail,
        "index out of date: 1 listed file(s) missing, 0 file(s) not listed"
    );
    assert_eq!(v.fix(), Some("run 'tasq store reconcile'"));
    nb.restore_missing_file();
    nb.write("stray.todo.md", "# [ ] Stray\n");
    let v = b.verify().unwrap();
    assert!(!v.consistent);
    assert_eq!(v.missing_files, Vec::<String>::new());
    assert_eq!(v.unindexed_files, vec!["stray.todo.md"]);
    std::fs::remove_file(nb.notebook().join("stray.todo.md")).unwrap();
    let v = b.verify().unwrap();
    assert!(v.consistent);
    assert_eq!(v.detail, "index matches the files");
    std::fs::remove_file(nb.notebook().join(".index")).unwrap();
    assert!(
        matches!(b.verify().unwrap_err(), StoreError::Io { path, .. } if path == nb.notebook().join(".index"))
    );
}

#[test]
fn sync_pulls_and_pushes_when_there_is_a_remote() {
    let nb = NbEnv::fixture();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    let s = b.sync(&clock()).unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "not a git repository; nothing to sync");
    nb.git_init();
    let s = b.sync(&clock()).unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "no remote configured; nothing to sync");
    let remote = nb.add_bare_remote();
    write_and_commit(&nb, &b, "# Notes\n\nsynced\n", "[tasq] Update: notes.md");
    assert_eq!(nb.subjects_of(&remote)[0], "[nb] Initialize", "interval 0");
    let s = b.sync(&clock()).unwrap();
    assert!(s.synced, "{s:?}");
    assert_eq!(s.detail, "pulled and pushed (origin)");
    assert_eq!(s.raw_output, None, "quiet pull and push print nothing");
    assert_eq!(
        nb.subjects_of(&remote),
        vec![
            "[tasq] Update: notes.md".to_owned(),
            "[nb] Initialize".to_owned()
        ]
    );
    assert_eq!(
        State::new(&nb.notebook()).last_sync(),
        Some(unix_time(&clock()))
    );
    assert!(!nb.notebook().join(".git/tasq/lock").exists(), "released");
    // A broken remote is an error with the manual fix.
    nb.git(&[
        "remote",
        "set-url",
        "origin",
        &nb.root.path().join("gone.git").display().to_string(),
    ]);
    let err = b.sync(&clock()).unwrap_err();
    assert!(
        err.to_string().starts_with(
            "bookkeeping failed after the write: pulling: git pull --rebase -q failed"
        ),
        "{err}"
    );
    assert!(
        err.to_string()
            .ends_with("; sync by hand with 'git pull --rebase && git push'"),
        "{err}"
    );
}

#[test]
fn store_writes_return_before_the_commit_and_never_run_nb() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let slow = slow_git(1);
    let fake_nb = fake_tool("nb", "echo 'nb must not run' >&2; exit 1");
    let env = nb.env_with_path_first(slow.path());
    let env = env
        .into_iter()
        .map(|(k, v)| {
            if k == "PATH" {
                (k, format!("{}:{v}", fake_nb.path().display()))
            } else {
                (k, v)
            }
        })
        .collect();
    let options = NbStoreOptions::new(Workflow::default()).with_env(env);
    let mut store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    assert_eq!(store.bookkeeper().name(), "native");
    let started = Instant::now();
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "done returned after {:?}",
        started.elapsed()
    );
    store.wait_for_bookkeeping().unwrap();
    assert_eq!(
        nb.git_subjects()[0],
        format!("[tasq] Done: {}", file_name(id::SUPPORT))
    );
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.priority = Priority::C;
    store.update(&task).unwrap();
    let created = store.create(TaskDraft::new("New one")).unwrap();
    store.wait_for_bookkeeping().unwrap();
    let name = store
        .file_of(&created.id)
        .unwrap()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        nb.git_subjects()[..2],
        [
            format!("[tasq] Add: {name}"),
            format!("[tasq] Update: {}", file_name(id::FULL))
        ]
    );
    assert_eq!(nb.git_status(), "");
    assert_eq!(fake_calls(&fake_nb), Vec::<String>::new(), "nb never ran");
    assert_eq!(store.warnings(), &[]);
}

#[test]
fn a_failed_job_is_a_warning_when_the_store_opens_next() {
    let nb = NbEnv::fixture();
    nb.git_init();
    hook(&nb, "pre-commit", "echo 'hook says no' >&2\nexit 1");
    let mut store = nb.open();
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    // The TUI writes through `dyn Store` and cannot show it: the failure
    // waits for the next open, not the next lock.
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.priority = Priority::C;
    store.update(&task).unwrap();
    assert_eq!(store.warnings(), &[]);
    drop(store);
    // Wait for the job without taking its failure.
    drop(
        NativeBookkeeper::new(nb.notebook(), nb.env.clone())
            .lock(&clock())
            .unwrap(),
    );
    let mut next = nb.open();
    let warnings = next.take_warnings();
    let messages: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        messages,
        vec![
            "bookkeeping failed: the background commit failed: committing: hook says no; check 'git status' in the notebook, then run 'tasq store sync'"
                .to_owned()
        ]
    );
    assert!(matches!(warnings[0], StoreWarning::Bookkeeping { .. }));
}

#[test]
fn opening_takes_the_sync_interval_from_the_config() {
    let nb = NbEnv::fixture();
    assert_eq!(nb.open().sync_interval(), 0);
    assert_eq!(nb.open_syncing(3600).sync_interval(), 3600);
}

#[test]
fn the_store_runs_its_bookkeeping_on_its_own_clock() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let remote = nb.add_bare_remote();
    let mut store = nb.open_syncing(3600).with_clock(Box::new(clock()));
    let state = State::new(&nb.notebook());
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    store.wait_for_bookkeeping().unwrap();
    assert_eq!(state.last_sync(), Some(unix_time(&clock())));
    assert_eq!(
        nb.subjects_of(&remote)[0],
        format!("[tasq] Done: {}", file_name(id::SUPPORT))
    );
    state.record_sync(0).unwrap();
    let s = store.sync().unwrap();
    assert!(s.synced, "{s:?}");
    assert_eq!(state.last_sync(), Some(unix_time(&clock())));
}
