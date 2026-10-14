//! T-201: notebook resolution (`$NB_DIR/<name>`, or a path; never nb) and
//! the native rebuild of a missing `.index` (ADR-0023).

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::path::PathBuf;

use tasq_core::config::StoreConfig;
use tasq_core::model::Workflow;
use tasq_store_nb::{NbStore, NbStoreOptions, Store, StoreError, StoreWarning};

use support::{NbEnv, fake_calls, fake_tool};

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
    let fake = fake_tool("nb", "echo 'nb must not run' >&2; exit 1");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let store = NbStore::open(&config("home"), &options).unwrap();
    assert_eq!(store.dir(), nb.notebook());
    assert_eq!(fake_calls(&fake), Vec::<String>::new());
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
fn a_path_names_a_notebook_outside_nb_dir() {
    let nb = NbEnv::fixture();
    // Where nb's local or relocated notebooks live.
    let elsewhere = nb.root.path().join("user/notes");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::rename(nb.notebook(), &elsewhere).unwrap();
    let options = NbStoreOptions::new(Workflow::default())
        .with_env(env(&[("NB_DIR", &nb.nb_dir.display().to_string())]))
        .with_home(nb.root.path().join("user"));
    let store = NbStore::open(&config("~/notes"), &options).unwrap();
    assert_eq!(store.dir(), elsewhere);
    let store = NbStore::open(&config(&elsewhere.display().to_string()), &options).unwrap();
    assert_eq!(store.dir(), elsewhere);
}

#[test]
fn missing_notebook_names_the_setting_and_the_config_file() {
    let nb = NbEnv::fixture();
    let options = NbStoreOptions::new(Workflow::default())
        .with_env(env(&[("NB_DIR", &nb.nb_dir.display().to_string())]))
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
    assert_eq!(
        *message,
        format!(
            "notebook \"work\" not found: {}/work is not a directory",
            nb.nb_dir.display()
        )
    );
    assert!(
        err.to_string().starts_with(
            "store.notebook (set in /etc/tasq/config.toml): notebook \"work\" not found"
        )
    );
}

#[test]
fn missing_index_is_rebuilt_from_the_files_in_name_order() {
    let nb = NbEnv::fixture();
    let index = nb.notebook().join(".index");
    std::fs::remove_file(&index).unwrap();
    let fake = fake_tool("nb", "echo 'nb must not run' >&2; exit 1");
    let options = NbStoreOptions::new(Workflow::default()).with_env(env(&[
        ("NB_DIR", &nb.nb_dir.display().to_string()),
        ("PATH", &fake.path().display().to_string()),
    ]));
    let mut store = NbStore::open(&config("home"), &options).unwrap();
    assert_eq!(fake_calls(&fake), Vec::<String>::new());
    assert!(store.rebuilt_index());
    assert_eq!(
        store.warnings(),
        &[StoreWarning::RebuiltIndex {
            path: index.clone()
        }]
    );
    assert_eq!(
        store.warnings()[0].to_string(),
        format!(
            "rebuilt missing index at {} from the notebook's files (todo ids may have changed)",
            index.display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(&index).unwrap(),
        "20260901090000.todo.md\n20260902100000.todo.md\n20260903110000.todo.md\n\
         20260904120000.todo.md\n20260906140000.todo.md\nnotes.md\n"
    );
    assert_eq!(store.describe().task_count, 5);
    let taken = store.take_warnings();
    assert_eq!(taken.len(), 1);
    assert_eq!(store.warnings(), []);
    assert!(!store.rebuilt_index());
}

#[test]
fn a_missing_index_that_cannot_be_written_is_an_index_error() {
    let nb = NbEnv::fixture();
    let index = nb.notebook().join(".index");
    std::fs::remove_file(&index).unwrap();
    // A dangling symlink: `.index` does not exist, but cannot be created.
    std::os::unix::fs::symlink(nb.root.path().join("no/such/dir"), &index).unwrap();
    let err =
        NbStore::open_dir(nb.notebook(), &NbStoreOptions::new(Workflow::default())).unwrap_err();
    let StoreError::Index { path, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(*path, index);
    assert!(
        message.starts_with("index not found and could not be rebuilt: "),
        "{message}"
    );
}

#[test]
fn open_dir_skips_resolution() {
    let nb = NbEnv::fixture();
    let store =
        NbStore::open_dir(nb.notebook(), &NbStoreOptions::new(Workflow::default())).unwrap();
    assert_eq!(store.dir(), nb.notebook());
    assert_eq!(store.index_path(), nb.notebook().join(".index"));
    assert_eq!(store.bookkeeper().name(), "native");
    assert_eq!(store.sync_interval(), 0);
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
