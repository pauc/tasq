//! T-206: the nb-CLI bookkeeper against a fake `nb` recording its argv (and
//! the real nb when installed), the native bookkeeper on a temporary git
//! repository, and `select_bookkeeper`.

mod support;

use std::path::Path;

use tasq_core::config::Bookkeeper as Choice;
use tasq_core::model::{Priority, TaskId, Workflow};
use tasq_store_nb::nb::Nb;
use tasq_store_nb::{
    Bookkeeper, NativeBookkeeper, NbCliBookkeeper, NbStore, NbStoreOptions, Store, StoreError,
    select_bookkeeper,
};

use support::{NbEnv, fake_nb, fake_nb_calls, file_name, id, nb_or_skip};

fn fake(body: &str) -> (tempfile::TempDir, Nb) {
    let dir = fake_nb(body);
    let nb = Nb::at(
        dir.path().join("nb"),
        vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())],
    );
    (dir, nb)
}

#[test]
fn nb_cli_register_passes_the_basename_and_the_folder() {
    let nb = NbEnv::fixture();
    let (dir, fake_nb) = fake("exit 0");
    let b = NbCliBookkeeper::new(fake_nb, nb.notebook());
    b.register(&nb.notebook().join("x.todo.md")).unwrap();
    assert_eq!(
        fake_nb_calls(&dir),
        vec![format!("index add x.todo.md {}", nb.notebook().display())]
    );
    let (dir, failing) = fake("echo 'File not found' >&2; exit 1");
    let b = NbCliBookkeeper::new(failing, nb.notebook());
    let err = b.register(&nb.notebook().join("x.todo.md")).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "bookkeeping failed after the write: registering x.todo.md: nb index add x.todo.md {} failed with status 1: File not found; run 'nb index reconcile' in the notebook",
            nb.notebook().display()
        )
    );
    assert_eq!(fake_nb_calls(&dir).len(), 1);
}

#[test]
fn nb_cli_checkpoint_commits_without_asking_dirty_and_waits() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let folder = nb.notebook().display().to_string();
    // One spawn: no `nb git dirty` before the checkpoint.
    let (dir, fake_nb) = fake("exit 0");
    let b = NbCliBookkeeper::new(fake_nb, nb.notebook());
    assert!(b.checkpoint("[tasq] Update: a.todo.md").unwrap());
    assert_eq!(
        fake_nb_calls(&dir),
        vec![format!(
            "git checkpoint {folder} [tasq] Update: a.todo.md --wait"
        )]
    );
    // checkpoint itself fails.
    let (_dir, no_commit) = fake("echo 'denied' >&2; exit 1");
    let b = NbCliBookkeeper::new(no_commit, nb.notebook());
    let err = b.checkpoint("[tasq] Update: a.todo.md").unwrap_err();
    assert!(
        err.to_string()
            .starts_with("bookkeeping failed after the write: committing: nb git checkpoint"),
        "{err}"
    );
    assert!(
        err.to_string()
            .ends_with("denied; commit by hand with 'nb git checkpoint'"),
        "{err}"
    );
}

#[test]
fn nb_store_write_runs_only_the_checkpoint() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let folder = nb.notebook().display().to_string();
    let fake = fake_nb("exit 0");
    let options = NbStoreOptions::new(Workflow::default())
        .with_env(vec![("PATH".to_owned(), fake.path().display().to_string())])
        .with_bookkeeper(Choice::Nb);
    let mut store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    // Unchanged: nothing written, nb never runs.
    store.update(&task).unwrap();
    assert_eq!(fake_nb_calls(&fake), Vec::<String>::new());
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    assert_eq!(
        fake_nb_calls(&fake),
        vec![format!(
            "git checkpoint {folder} [tasq] Update: {} --wait",
            file_name(id::SUPPORT)
        )]
    );
    assert_eq!(store.warnings(), []);
}

#[test]
fn nb_cli_checkpoint_skips_notebooks_without_git() {
    let nb = NbEnv::fixture();
    let (dir, fake_nb) = fake("exit 0");
    let b = NbCliBookkeeper::new(fake_nb, nb.notebook());
    assert!(!(b.checkpoint("[tasq] Update: a.todo.md").unwrap()));
    assert_eq!(fake_nb_calls(&dir), Vec::<String>::new(), "nb never ran");
}

#[test]
fn nb_cli_verify_reads_status_and_text() {
    let nb = NbEnv::fixture();
    let folder = nb.notebook().display().to_string();
    let (dir, fine) = fake("exit 0");
    let v = NbCliBookkeeper::new(fine, nb.notebook()).verify().unwrap();
    assert!(v.consistent);
    assert_eq!(v.detail, "nb index verify: index matches the files");
    assert_eq!(v.raw_output, None);
    assert_eq!(fake_nb_calls(&dir), vec![format!("index verify {folder}")]);
    // nb 7.25 here: exit 1 with the message on stderr (ANSI noise included).
    let (_dir, loud) = fake(
        "printf '\\033[31m! Index corrupted.\\033[0m To fix, run:\\r\\n  nb index reconcile\\n' >&2; exit 1",
    );
    let v = NbCliBookkeeper::new(loud, nb.notebook()).verify().unwrap();
    assert!(!v.consistent);
    assert_eq!(v.detail, "nb index verify: index corrupted");
    assert_eq!(
        v.raw_output.as_deref(),
        Some("! Index corrupted. To fix, run:\n  nb index reconcile")
    );
    assert_eq!(v.fix(), Some("run 'nb index reconcile' in the notebook"));
    // The variant PROGRESS.md recorded: "Index corrupted" with exit 0.
    let (_dir, quiet) = fake("echo 'Index corrupted'; exit 0");
    let v = NbCliBookkeeper::new(quiet, nb.notebook()).verify().unwrap();
    assert!(!v.consistent);
    assert_eq!(v.detail, "nb index verify: index corrupted");
    assert_eq!(v.raw_output.as_deref(), Some("Index corrupted"));
    // Some other failure.
    let (_dir, other) = fake("echo 'no such notebook' >&2; exit 1");
    let v = NbCliBookkeeper::new(other, nb.notebook()).verify().unwrap();
    assert!(!v.consistent);
    assert_eq!(v.detail, "nb index verify failed");
    assert_eq!(v.raw_output.as_deref(), Some("no such notebook"));
    // nb cannot even start.
    let missing = Nb::at(nb.root.path().join("no-nb"), Vec::new());
    let err = NbCliBookkeeper::new(missing, nb.notebook())
        .verify()
        .unwrap_err();
    assert!(matches!(err, StoreError::Bookkeeping { .. }), "{err:?}");
    assert!(
        err.to_string()
            .starts_with("bookkeeping failed after the write: verifying the index: cannot run"),
        "{err}"
    );
}

#[test]
fn nb_cli_sync_treats_no_remote_as_nothing_to_do() {
    let nb = NbEnv::fixture();
    let (dir, fine) = fake("echo 'Already up to date.'; exit 0");
    let s = NbCliBookkeeper::new(fine, nb.notebook()).sync().unwrap();
    assert!(s.synced);
    assert_eq!(s.detail, "nb sync: synced with the remote");
    assert_eq!(s.raw_output.as_deref(), Some("Already up to date."));
    assert_eq!(fake_nb_calls(&dir), vec!["sync".to_owned()]);
    let (_dir, quiet) = fake("exit 0");
    let s = NbCliBookkeeper::new(quiet, nb.notebook()).sync().unwrap();
    assert_eq!(s.raw_output, None);
    let (_dir, no_remote) =
        fake("printf '! No remote configured.\\n\\nSet the remote\\n' >&2; exit 1");
    let s = NbCliBookkeeper::new(no_remote, nb.notebook())
        .sync()
        .unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "nb sync: no remote configured");
    assert_eq!(
        s.raw_output.as_deref(),
        Some("! No remote configured.\n\nSet the remote")
    );
    let (_dir, broken) = fake("echo 'auth failed' >&2; exit 1");
    let err = NbCliBookkeeper::new(broken, nb.notebook())
        .sync()
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "bookkeeping failed after the write: syncing: nb sync failed with status 1: auth failed; fix the remote and run 'nb sync' by hand"
    );
}

#[test]
fn native_checkpoint_commits_only_dirty_repositories() {
    let nb = NbEnv::fixture();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    assert_eq!(b.name(), "native");
    assert!(
        !b.checkpoint("[tasq] Update: x").unwrap(),
        "not a repository"
    );
    nb.git_init();
    assert!(!(b.checkpoint("[tasq] Update: x").unwrap()), "clean");
    assert_eq!(nb.git_subjects(), vec!["[nb] Initialize".to_owned()]);
    nb.write("notes.md", "# Notes\n\nchanged\n");
    assert!(b.checkpoint("[tasq] Update: notes.md").unwrap());
    assert_eq!(
        nb.git_subjects(),
        vec![
            "[tasq] Update: notes.md".to_owned(),
            "[nb] Initialize".to_owned()
        ]
    );
    assert_eq!(nb.git_status(), "");
    // A new untracked file counts as dirty and is added.
    nb.write("fresh.todo.md", "# [ ] Fresh\n");
    assert!(b.checkpoint("[tasq] Add: fresh.todo.md").unwrap());
    assert_eq!(nb.git_status(), "");
    assert_eq!(nb.git_subjects().len(), 3);
    // A change git ignores commits nothing and is not a failure.
    nb.write(".gitignore", "ignored.md\n");
    assert!(b.checkpoint("[tasq] Update: .gitignore").unwrap());
    nb.write("ignored.md", "# Ignored\n");
    assert!(!(b.checkpoint("[tasq] Update: ignored.md").unwrap()));
    assert_eq!(nb.git_subjects().len(), 4);
}

#[test]
fn native_checkpoint_rejected_by_a_hook_is_a_failure() {
    let nb = NbEnv::fixture();
    nb.git_init();
    let hooks = nb.notebook().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'hook says no' >&2\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&hook).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&hook, perms).unwrap();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    nb.write("notes.md", "# Notes\n\nchanged\n");
    let err = b.checkpoint("[tasq] Update: notes.md").unwrap_err();
    assert_eq!(
        err.to_string(),
        "bookkeeping failed after the write: committing: git commit -q -m [tasq] Update: notes.md failed with status 1: hook says no; commit by hand with 'git add -A && git commit'"
    );
    assert_eq!(nb.git_subjects(), vec!["[nb] Initialize".to_owned()]);
}

#[test]
fn native_verify_reports_the_fixture_gap_and_recovers() {
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
    assert_eq!(v.fix(), Some("run 'nb index reconcile' in the notebook"));
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
fn native_sync_pulls_and_pushes_when_there_is_a_remote() {
    let nb = NbEnv::fixture();
    let b = NativeBookkeeper::new(nb.notebook(), nb.env.clone());
    let s = b.sync().unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "not a git repository; nothing to sync");
    nb.git_init();
    let s = b.sync().unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "no remote configured; nothing to sync");
    let remote = nb.add_bare_remote();
    nb.write("notes.md", "# Notes\n\nsynced\n");
    assert!(b.checkpoint("[tasq] Update: notes.md").unwrap());
    let s = b.sync().unwrap();
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
    // A broken remote is an error with the manual fix.
    nb.git(&[
        "remote",
        "set-url",
        "origin",
        &nb.root.path().join("gone.git").display().to_string(),
    ]);
    let err = b.sync().unwrap_err();
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
fn nb_cli_sync_pushes_through_nb() {
    if !nb_or_skip("nb_cli_sync_pushes_through_nb") {
        return;
    }
    let nb = NbEnv::fixture();
    nb.restore_missing_file();
    nb.git_init();
    let remote = nb.add_bare_remote();
    let mut store = nb.open_with(Choice::Nb);
    let mut task = store.get(&TaskId::from(id::SUPPORT)).unwrap();
    task.set_priority(Priority::A);
    store.update(&task).unwrap();
    assert_eq!(store.warnings(), []);
    assert_eq!(
        nb.git_subjects()[0],
        format!("[tasq] Update: {}", file_name(id::SUPPORT))
    );
    let s = store.bookkeeper().sync().unwrap();
    assert!(s.synced, "{s:?}");
    assert_eq!(
        nb.subjects_of(&remote)[0],
        format!("[tasq] Update: {}", file_name(id::SUPPORT))
    );
    // Without a remote nb says so and we report it rather than fail.
    let plain = NbEnv::fixture();
    plain.git_init();
    let s = plain.open_with(Choice::Nb).bookkeeper().sync().unwrap();
    assert!(!s.synced);
    assert_eq!(s.detail, "nb sync: no remote configured");
}

#[test]
fn nb_cli_verify_against_the_real_nb() {
    if !nb_or_skip("nb_cli_verify_against_the_real_nb") {
        return;
    }
    let nb = NbEnv::fixture();
    let store = nb.open_with(Choice::Nb);
    let v = store.bookkeeper().verify().unwrap();
    assert!(
        !v.consistent,
        "the fixture index names a missing file: {v:?}"
    );
    assert!(
        v.raw_output
            .as_deref()
            .unwrap_or_default()
            .contains("Index corrupted"),
        "{v:?}"
    );
    nb.restore_missing_file();
    let v = store.bookkeeper().verify().unwrap();
    assert!(v.consistent, "{v:?}");
}

#[test]
fn selection_follows_the_config_and_the_path() {
    let nb = NbEnv::fixture();
    let without_nb = NbStoreOptions::new(Workflow::default()).with_env(vec![(
        "PATH".to_owned(),
        nb.root.path().join("empty").display().to_string(),
    )]);
    let store = NbStore::open_dir(nb.notebook(), &without_nb).unwrap();
    assert_eq!(store.bookkeeper().name(), "native", "auto without nb");
    let store = NbStore::open_dir(
        nb.notebook(),
        &without_nb.clone().with_bookkeeper(Choice::Native),
    )
    .unwrap();
    assert_eq!(store.bookkeeper().name(), "native");
    let err = NbStore::open_dir(
        nb.notebook(),
        &without_nb.clone().with_bookkeeper(Choice::Nb),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "store.bookkeeper: set to \"nb\" but nb is not on PATH (install nb, or use \"native\" or \"auto\")"
    );
    // With a (fake) nb on PATH, auto and nb pick it; native still does not.
    let fake = fake_nb("exit 0");
    let with_nb = NbStoreOptions::new(Workflow::default())
        .with_env(vec![("PATH".to_owned(), fake.path().display().to_string())]);
    let store = NbStore::open_dir(nb.notebook(), &with_nb).unwrap();
    assert_eq!(store.bookkeeper().name(), "nb");
    let store =
        NbStore::open_dir(nb.notebook(), &with_nb.clone().with_bookkeeper(Choice::Nb)).unwrap();
    assert_eq!(store.bookkeeper().name(), "nb");
    let store = NbStore::open_dir(
        nb.notebook(),
        &with_nb.clone().with_bookkeeper(Choice::Native),
    )
    .unwrap();
    assert_eq!(store.bookkeeper().name(), "native");
    assert_eq!(
        fake_nb_calls(&fake),
        Vec::<String>::new(),
        "opening never runs nb"
    );
    // The function itself, with an explicit nb.
    let located = Nb::locate(&with_nb.env).unwrap();
    let b = select_bookkeeper(Choice::Auto, Some(&located), Path::new("/x"), &[]).unwrap();
    assert_eq!(b.name(), "nb");
    // `open` honours `store.bookkeeper` from the config.
    let store = nb.open_with(Choice::Native);
    assert_eq!(store.bookkeeper().name(), "native");
}

#[test]
fn auto_picks_nb_when_installed() {
    if !nb_or_skip("auto_picks_nb_when_installed") {
        return;
    }
    let nb = NbEnv::fixture();
    assert_eq!(nb.open().bookkeeper().name(), "nb");
    assert_eq!(nb.open_with(Choice::Nb).bookkeeper().name(), "nb");
}
