//! `Revision` and `diff::apply` through the crate's public paths.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use tasq_core::format;
use tasq_core::model::{Status, Workflow};
use tasq_store::{Revision, diff};

#[test]
fn a_rewritten_file_has_a_new_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("task.md");
    std::fs::write(&path, b"one").unwrap();
    let first = Revision::of(&path, b"one").unwrap();
    std::fs::write(&path, b"two").unwrap();
    assert_ne!(first, Revision::of(&path, b"two").unwrap());
}

#[test]
fn apply_edits_the_document_to_match_the_task() {
    let workflow = Workflow::default();
    let parsed =
        format::parse_v2("---\nid: 1\nstatus: ready\n---\n# T\n", None, &workflow).unwrap();
    let (mut doc, mut task) = (parsed.document, parsed.task);
    task.set_status(Status::BLOCKED);
    diff::apply(&mut doc, &task, &workflow).unwrap();
    assert_eq!(
        format::render(&doc),
        "---\nid: 1\nstatus: blocked\n---\n# T\n"
    );
}
