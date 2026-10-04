//! T-201: notebook resolution (`$NB_DIR/<name>`, else `nb notebooks show`).

mod support;

use std::path::PathBuf;

use tasq_core::config::StoreConfig;
use tasq_core::model::Workflow;
use tasq_store_nb::{NbStore, NbStoreOptions, Store, StoreError};

use support::{NbEnv, fake_nb, fake_nb_calls};

fn config(notebook: &str) -> StoreConfig {
    StoreConfig {
        notebook: notebook.to_owned(),
        ..StoreConfig::default()
    }
}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn resolves_under_nb_dir_without_running_nb() {
    let nb = NbEnv::fixture();
    // PATH holds only a fake nb that fails loudly if invoked.
    let fake = fake_nb("echo 'nb must not run' >&2; exit 1");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let store = NbStore::open(&config("home"), &options).unwrap();
    assert_eq!(store.dir(), nb.notebook());
    assert_eq!(fake_nb_calls(&fake), Vec::<String>::new());
    assert!(!store.rebuilt_index());
    assert_eq!(store.warnings(), []);
}

#[test]
fn defaults_nb_dir_to_dot_nb_under_home() {
    let nb = NbEnv::fixture();
    // Lay the notebook out as ~/.nb/home.
    let home = nb.root.path().join("user");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::rename(&nb.nb_dir, home.join(".nb")).unwrap();
    let options = NbStoreOptions::new(Workflow::default()).with_home(&home);
    let store = NbStore::open(&config("home"), &options).unwrap();
    assert_eq!(store.dir(), home.join(".nb").join("home"));
}

#[test]
fn falls_back_to_nb_and_sanitises_its_output() {
    let nb = NbEnv::fixture();
    // Not under NB_DIR: the notebook lives elsewhere, as nb's local or
    // relocated notebooks do. The fake nb prints a welcome, colours and CRs.
    let elsewhere = nb.root.path().join("elsewhere");
    std::fs::rename(nb.notebook(), &elsewhere).unwrap();
    let fake = fake_nb(&format!(
        "printf 'Welcome to nb!\\r\\n\\033[1m  {}  \\033[0m\\r\\n'",
        elsewhere.display()
    ));
    let options = NbStoreOptions::new(Workflow::default())
        .with_env(env(&[
            ("NB_DIR", &nb.nb_dir.display().to_string()),
            ("PATH", &fake.path().display().to_string()),
        ]))
        .with_home(&nb.home);
    let store = NbStore::open(&config("work"), &options).unwrap();
    assert_eq!(store.dir(), elsewhere);
    assert_eq!(
        fake_nb_calls(&fake),
        vec!["notebooks show work --path".to_owned()]
    );
    assert_eq!(store.nb().unwrap().program(), fake.path().join("nb"));
}

#[test]
fn missing_notebook_names_the_setting_and_the_config_file() {
    let nb = NbEnv::fixture();
    let fake = fake_nb("printf '! Notebook not found: work\\n' >&2; exit 1");
    let options = NbStoreOptions::new(Workflow::default())
        .with_env(env(&[
            ("NB_DIR", &nb.nb_dir.display().to_string()),
            ("PATH", &fake.path().display().to_string()),
        ]))
        .with_config_file("/etc/tasq/config.toml");
    let err = NbStore::open(&config("work"), &options).unwrap_err();
    let StoreError::Config {
        setting,
        file,
        message,
    } = &err
    else {
        panic!("{err:?}")
    };
    assert_eq!(*setting, "store.notebook");
    assert_eq!(*file, Some(PathBuf::from("/etc/tasq/config.toml")));
    assert!(
        message.contains(&format!("{}/work is not a directory", nb.nb_dir.display())),
        "{message}"
    );
    assert!(
        message.contains(
            "nb notebooks show work --path failed with status 1: ! Notebook not found: work"
        ),
        "{message}"
    );
    assert!(
        err.to_string().starts_with(
            "store.notebook (set in /etc/tasq/config.toml): notebook \"work\" not found"
        )
    );
}

#[test]
fn missing_notebook_without_nb_on_path() {
    let nb = NbEnv::fixture();
    let empty = tempfile::tempdir().unwrap();
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &empty.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("work"), &options).unwrap_err();
    assert!(
        err.to_string().ends_with("and nb is not on PATH to ask"),
        "{err}"
    );
}

#[test]
fn nb_printing_nothing_or_a_non_directory_is_an_error() {
    let nb = NbEnv::fixture();
    let nb_dir = nb.nb_dir.display().to_string();
    let silent = fake_nb("printf '\\033[0m\\r\\n'");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb_dir),
        ("PATH", &silent.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("work"), &options).unwrap_err();
    assert!(err.to_string().ends_with("and nb printed no path"), "{err}");

    let file = nb.file(support::id::FULL);
    let points_at_file = fake_nb(&format!("printf '{}\\n'", file.display()));
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb_dir),
        ("PATH", &points_at_file.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("work"), &options).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "store.notebook: notebook \"work\" resolved by nb to {}, which is not a directory",
            file.display()
        )
    );
}

#[test]
fn missing_index_without_nb_is_an_index_error() {
    let nb = NbEnv::fixture();
    std::fs::remove_file(nb.notebook().join(".index")).unwrap();
    let empty = tempfile::tempdir().unwrap();
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &empty.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("home"), &options).unwrap_err();
    let StoreError::Index { path, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*path, nb.notebook().join(".index"));
    assert_eq!(
        message,
        "nb index not found, and nb is not on PATH to rebuild it (run 'nb index reconcile' in the notebook)"
    );
}

#[test]
fn missing_index_with_a_failing_nb_is_an_index_error() {
    let nb = NbEnv::fixture();
    std::fs::remove_file(nb.notebook().join(".index")).unwrap();
    let fake = fake_nb("exit 1");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("home"), &options).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "{}: nb index not found, and 'nb index reconcile' could not rebuild it",
            nb.notebook().join(".index").display()
        )
    );
    assert_eq!(
        fake_nb_calls(&fake),
        vec![format!("index reconcile {}", nb.notebook().display())]
    );
}

#[test]
fn missing_index_with_an_nb_that_succeeds_without_writing_one_is_an_index_error() {
    let nb = NbEnv::fixture();
    std::fs::remove_file(nb.notebook().join(".index")).unwrap();
    let fake = fake_nb("exit 0");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let err = NbStore::open(&config("home"), &options).unwrap_err();
    assert!(
        matches!(&err, StoreError::Index { message, .. } if message.contains("could not rebuild it")),
        "{err:?}"
    );
}

#[test]
fn missing_index_rebuilt_by_a_fake_nb_sets_the_warning() {
    let nb = NbEnv::fixture();
    let index = nb.notebook().join(".index");
    std::fs::remove_file(&index).unwrap();
    // The fake "reconciles" by writing a two-line index into the given dir.
    let fake =
        fake_nb("printf '20260901090000.todo.md\\n20260902100000.todo.md\\n' > \"$3/.index\"");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let mut store = NbStore::open(&config("home"), &options).unwrap();
    assert!(store.rebuilt_index());
    assert_eq!(
        store.warnings(),
        &[tasq_store_nb::StoreWarning::RebuiltIndex {
            path: index.clone()
        }]
    );
    assert_eq!(
        store.warnings()[0].to_string(),
        format!(
            "rebuilt missing nb index at {} (todo ids may have changed)",
            index.display()
        )
    );
    assert_eq!(store.describe().task_count, 2);
    let taken = store.take_warnings();
    assert_eq!(taken.len(), 1);
    assert_eq!(store.warnings(), []);
    assert!(!store.rebuilt_index());
}

#[test]
fn open_dir_skips_resolution() {
    let nb = NbEnv::fixture();
    let store =
        NbStore::open_dir(nb.notebook(), &NbStoreOptions::new(Workflow::default())).unwrap();
    assert_eq!(store.dir(), nb.notebook());
    assert_eq!(store.index_path(), nb.notebook().join(".index"));
    assert!(store.nb().is_none(), "no PATH in the options");
    assert_eq!(
        store.bookkeeper().name(),
        "native",
        "auto without nb falls back to the native bookkeeper"
    );
    assert_eq!(store.env(), Vec::<(String, String)>::new());
    assert_eq!(store.workflow(), &Workflow::default());
    let debug = format!("{store:?}");
    assert!(debug.starts_with("NbStore { dir:"), "{debug}");
}

#[test]
fn unreadable_index_is_an_io_error() {
    let nb = NbEnv::fixture();
    let index = nb.notebook().join(".index");
    std::fs::remove_file(&index).unwrap();
    std::fs::create_dir(&index).unwrap();
    let err =
        NbStore::open_dir(nb.notebook(), &NbStoreOptions::new(Workflow::default())).unwrap_err();
    assert!(
        matches!(&err, StoreError::Io { path, .. } if *path == index),
        "{err:?}"
    );
}
