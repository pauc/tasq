//! Parity with the real `nb`: ids, `nb todo do`, index handling. The store
//! never runs nb (ADR-0023); these tests run nb next to it to prove nb still
//! reads what tasq writes. Each test runs only when `nb` is installed (see
//! `support::nb_or_skip`).

mod support;

use tasq_core::format::{self, ops};
use tasq_core::model::{TaskId, Workflow};
use tasq_core::query::Filter;
use tasq_store_nb::{NbStore, Store};

use support::{NbEnv, file_name, id, nb_ids, nb_or_skip};

fn store_ids(store: &NbStore, filter: &Filter) -> Vec<u64> {
    let mut ids: Vec<u64> = store
        .list(filter)
        .unwrap()
        .iter()
        .map(|t| t.id.as_str().parse().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

#[test]
fn nb_lists_the_same_ids_without_a_git_repository() {
    if !nb_or_skip("nb_lists_the_same_ids_without_a_git_repository") {
        return;
    }
    let nb = NbEnv::fixture();
    let index_before = nb.read(".index");
    let open = nb.nb(&["todos", "open", "--no-color"]);
    let all = nb.nb(&["todos", "--no-color"]);
    let store = nb.open();
    assert_eq!(sorted(nb_ids(&open)), store_ids(&store, &Filter::default()));
    assert_eq!(
        sorted(nb_ids(&all)),
        store_ids(&store, &Filter::default().any_done())
    );
    assert_eq!(
        sorted(nb_ids(&all)),
        vec![id::FULL, id::SUPPORT, id::DONE, id::WAITING, id::NO_TAGS]
    );
    assert_eq!(
        nb.read(".index"),
        index_before,
        "nb left the index (and the missing line) alone"
    );
    assert!(
        !nb.notebook().join(".git").exists(),
        "nb did not initialise git"
    );
}

#[test]
fn nb_lists_the_same_ids_with_a_git_repository() {
    if !nb_or_skip("nb_lists_the_same_ids_with_a_git_repository") {
        return;
    }
    let nb = NbEnv::fixture();
    nb.git_init();
    let open = nb.nb(&["todos", "open", "--no-color"]);
    let store = nb.open();
    assert_eq!(sorted(nb_ids(&open)), store_ids(&store, &Filter::default()));
    // With git, nb also resolves the notebook by name.
    let path = support::sanitize::last_line(&nb.nb(&["notebooks", "show", "home", "--path"]));
    assert_eq!(std::path::PathBuf::from(path), nb.notebook());
}

#[test]
fn set_done_matches_nb_todo_do_byte_for_byte() {
    if !nb_or_skip("set_done_matches_nb_todo_do_byte_for_byte") {
        return;
    }
    // nb's copy: `nb todo do` needs a git repository to commit into.
    let theirs = NbEnv::fixture();
    theirs.git_init();
    theirs.nb(&["todo", "do", &id::SUPPORT.to_string()]);
    theirs.nb(&["todo", "do", &id::NO_TAGS.to_string()]);
    assert_eq!(
        theirs.git_subjects()[0],
        format!("[nb] Done: {}", file_name(id::NO_TAGS))
    );

    // Our copy, no git needed.
    let ours = NbEnv::fixture();
    let mut store = ours.open();
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    store.set_done(&TaskId::from(id::NO_TAGS), true).unwrap();

    // Without a status tag the files are identical...
    assert_eq!(
        ours.read(file_name(id::NO_TAGS)),
        theirs.read(file_name(id::NO_TAGS))
    );
    // ...and with one, ours is nb's output plus `strip_status_tag`.
    let nb_text = theirs.read(file_name(id::SUPPORT));
    assert!(
        nb_text.contains("#support #B #ready\n"),
        "nb keeps the status tag: {nb_text}"
    );
    let mut doc = format::Document::parse(&nb_text).unwrap();
    ops::strip_status_tag(&mut doc, &Workflow::default());
    assert_eq!(ours.read(file_name(id::SUPPORT)), format::render(&doc));
    // nb reads our file back as done.
    let listed = ours.nb(&["todos", "open", "--no-color"]);
    assert_eq!(sorted(nb_ids(&listed)), vec![id::FULL, id::WAITING]);
}

#[test]
fn our_writes_keep_nb_index_consistent() {
    if !nb_or_skip("our_writes_keep_nb_index_consistent") {
        return;
    }
    let nb = NbEnv::fixture();
    // Drop the missing-file line so nb's own verify passes to begin with.
    let mut index = String::new();
    for line in nb
        .read(".index")
        .lines()
        .filter(|l| *l != file_name(id::MISSING))
    {
        index.push_str(line);
        index.push('\n');
    }
    nb.write(".index", &index);
    nb.git_init();
    let verify_before = nb.nb(&["index", "verify"]);
    let mut store = nb.open();
    let mut task = store.get(&TaskId::from(id::FULL)).unwrap();
    task.set_status(tasq_core::model::Status::READY);
    store.update(&task).unwrap();
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    store.wait_for_bookkeeping().unwrap();
    assert_eq!(nb.nb(&["index", "verify"]), verify_before);
    assert!(!nb.nb(&["index", "verify"]).contains("corrupted"));
    assert_eq!(nb.read(".index"), index);
}

#[test]
fn a_rebuilt_index_agrees_with_nb() {
    if !nb_or_skip("a_rebuilt_index_agrees_with_nb") {
        return;
    }
    let nb = NbEnv::fixture();
    let index = nb.notebook().join(".index");
    std::fs::remove_file(&index).unwrap();
    let store = nb.open();
    assert!(store.rebuilt_index());
    let rebuilt = nb.read(".index");
    assert!(!rebuilt.contains(file_name(id::MISSING)), "{rebuilt}");
    assert_eq!(rebuilt.lines().count(), 6);
    assert!(!nb.nb(&["index", "verify"]).contains("corrupted"));
    let open = nb.nb(&["todos", "open", "--no-color"]);
    assert_eq!(sorted(nb_ids(&open)), store_ids(&store, &Filter::default()));
}

#[test]
fn a_reconciled_index_keeps_ids_and_satisfies_nb() {
    if !nb_or_skip("a_reconciled_index_keeps_ids_and_satisfies_nb") {
        return;
    }
    let nb = NbEnv::fixture();
    nb.write("20261001000000.todo.md", "# [ ] Unlisted\n");
    let (consistent, before) = nb.nb_status(&["index", "verify"]);
    assert!(!consistent, "the fixture gap: {before}");
    let mut store = nb.open();
    let ids_before = store_ids(&store, &Filter::default());
    let found = store.reconcile().unwrap();
    assert!(!found.consistent);
    assert_eq!(found.missing_files, vec![file_name(id::MISSING)]);
    assert_eq!(found.unindexed_files, vec!["20261001000000.todo.md"]);
    let (consistent, after) = nb.nb_status(&["index", "verify"]);
    assert!(consistent && !after.contains("corrupted"), "{after}");
    let mut ids_after = ids_before.clone();
    ids_after.push(8);
    assert_eq!(store_ids(&store, &Filter::default()), ids_after);
    let open = nb.nb(&["todos", "open", "--no-color"]);
    assert_eq!(sorted(nb_ids(&open)), ids_after, "nb agrees");
}
