//! ADR-0023: `NbStore::reconcile` repairs `.index` without renumbering.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use tasq_core::model::TaskId;
use tasq_core::query::Filter;
use tasq_store_nb::Store;

use support::{NbEnv, file_name, id};

#[test]
fn reconcile_blanks_missing_lines_appends_unlisted_files_and_commits() {
    let nb = NbEnv::fixture();
    nb.git_init();
    nb.write("20261001000000.todo.md", "# [ ] Unlisted\n");
    nb.write("zz-notes.md", "# Notes\n");
    let before = nb.read(".index");
    let mut store = nb.open();
    let full = store.get(&TaskId::from(id::FULL)).unwrap();
    let found = store.reconcile().unwrap();
    assert!(!found.consistent);
    assert_eq!(found.missing_files, vec![file_name(id::MISSING)]);
    assert_eq!(
        found.unindexed_files,
        vec!["20261001000000.todo.md", "zz-notes.md"]
    );
    let expected = format!(
        "{}20261001000000.todo.md\nzz-notes.md\n",
        before.replace(&format!("{}\n", file_name(id::MISSING)), "\n")
    );
    assert_eq!(nb.read(".index"), expected);
    assert_eq!(
        store.get(&TaskId::from(id::FULL)).unwrap(),
        full,
        "ids kept"
    );
    assert_eq!(
        store.get(&TaskId::from(8)).unwrap().title,
        "Unlisted",
        "appended after the 7 existing lines"
    );
    assert!(store.list(&Filter::default()).is_ok());
    store.wait_for_bookkeeping().unwrap();
    assert_eq!(nb.git_subjects()[0], "[tasq] Reconcile: .index");
    assert_eq!(nb.git_status(), "");
    assert!(store.bookkeeper().verify().unwrap().consistent);

    // Already consistent: nothing is written or committed.
    let mtime = nb.mtime(".index");
    let again = store.reconcile().unwrap();
    assert!(again.consistent);
    store.wait_for_bookkeeping().unwrap();
    assert_eq!(nb.mtime(".index"), mtime);
    assert_eq!(nb.git_subjects()[0], "[tasq] Reconcile: .index");
    assert_eq!(nb.git_subjects().len(), 2);
    assert_eq!(store.warnings(), []);
}
