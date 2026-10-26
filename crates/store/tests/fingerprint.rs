//! PE-006: the fingerprint of a store directory, which the TUI compares
//! every two seconds to notice task files changed by anything else.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::time::{Duration, SystemTime};

use tasq_store::fingerprint;

use support::{TempStore, task_file, touch};

#[test]
fn nothing_changed_is_the_same_fingerprint() {
    let temp = TempStore::absent();
    let absent = fingerprint(&temp.dir()).unwrap();
    std::fs::create_dir(temp.dir()).unwrap();
    assert_eq!(fingerprint(&temp.dir()).unwrap(), absent, "both are empty");
    temp.write("01A.md", &task_file(1, "One", "ready"));
    let once = fingerprint(&temp.dir()).unwrap();
    assert_ne!(once, absent);
    assert_eq!(fingerprint(&temp.dir()).unwrap(), once);
}

#[test]
fn a_task_file_added_rewritten_touched_renamed_or_removed_changes_it() {
    let temp = TempStore::empty();
    let one = temp.write("01A.md", &task_file(1, "One", "ready"));
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    touch(&one, at);
    let mut seen = vec![fingerprint(&temp.dir()).unwrap()];
    let mut changed = |what: &str| {
        let now = fingerprint(&temp.dir()).unwrap();
        assert!(!seen.contains(&now), "{what}: {now} seen before");
        seen.push(now);
    };

    temp.write("01B.MD", &task_file(2, "Two", "ready"));
    changed("added, the extension in capitals");
    std::fs::write(&one, task_file(1, "One, longer", "ready")).unwrap();
    touch(&one, at);
    changed("rewritten with another length, same mtime");
    touch(&one, at + Duration::from_secs(1));
    changed("touched, same length");
    std::fs::rename(&one, temp.file("01C.md")).unwrap();
    changed("renamed");
    std::fs::remove_file(temp.file("01C.md")).unwrap();
    changed("removed");
}

#[test]
fn files_the_store_does_not_read_leave_it_alone() {
    let temp = TempStore::empty();
    temp.write("01A.md", &task_file(1, "One", "ready"));
    let before = fingerprint(&temp.dir()).unwrap();
    temp.write(".01A.md.swp.md", "editor lock");
    temp.write("notes.txt", "not a task");
    temp.write(".tasq/lock", "held");
    temp.write(".tasq/plugins/git/warnings", "behind\n");
    temp.write(
        "attachments.md/one.png",
        "a directory named like a task file",
    );
    assert_eq!(fingerprint(&temp.dir()).unwrap(), before);
}

#[test]
fn a_store_path_that_is_not_a_directory_is_an_error() {
    let temp = TempStore::absent();
    std::fs::write(temp.dir(), "a file").unwrap();
    assert!(fingerprint(&temp.dir()).is_err());
}

#[test]
fn a_dangling_symlink_is_left_out_and_a_symlink_loop_is_an_error() {
    let temp = TempStore::empty();
    temp.write("01A.md", &task_file(1, "One", "ready"));
    let before = fingerprint(&temp.dir()).unwrap();
    std::os::unix::fs::symlink(temp.file("gone.md"), temp.file("01B.md")).unwrap();
    assert_eq!(
        fingerprint(&temp.dir()).unwrap(),
        before,
        "as the store skips it"
    );
    std::os::unix::fs::symlink(temp.file("01C.md"), temp.file("01C.md")).unwrap();
    assert!(fingerprint(&temp.dir()).is_err());
}
