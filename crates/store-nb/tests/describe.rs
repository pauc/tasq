//! T-204: `describe()`.

mod support;

use tasq_core::model::TaskId;
use tasq_store_nb::{IdScheme, Store, StoreInfo};

use support::{NbEnv, id};

#[test]
fn describe_reports_location_count_and_positional_ids() {
    let nb = NbEnv::fixture();
    let mut store = nb.open();
    assert_eq!(
        store.describe(),
        StoreInfo {
            name: "nb".into(),
            location: nb.notebook(),
            task_count: 5,
            id_scheme: IdScheme::Positional,
            ids_may_change_on_reconcile: true,
        },
        "open and done todos count; notes.md and the missing file do not"
    );
    // Files are checked live; the index is the one from the last read.
    std::fs::remove_file(nb.file(id::WAITING)).unwrap();
    assert_eq!(
        store.describe().task_count,
        4,
        "a vanished file no longer counts"
    );
    nb.write("20260907150000.todo.md", "# [ ] Added later\n");
    let mut index = nb.read(".index");
    index.push_str("20260907150000.todo.md\n");
    nb.write(".index", &index);
    assert_eq!(store.describe().task_count, 4, "index not re-read yet");
    let _ = store.get(&TaskId::from(id::FULL)).unwrap();
    assert_eq!(store.describe().task_count, 5);
    store.set_done(&TaskId::from(id::SUPPORT), true).unwrap();
    assert_eq!(store.describe().task_count, 5, "done tasks still count");
    assert_eq!(store.describe().location, nb.notebook());
}
