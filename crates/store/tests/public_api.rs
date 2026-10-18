//! `Revision` and `diff::apply` through the crate's public paths, the ones
//! `tasq-store-nb` re-exports.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use tasq_core::format;
use tasq_core::model::{Status, TaskId, Workflow};
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
    let parsed = format::parse(
        "# [ ] T\n\n## Tags\n\n#B #ready\n",
        TaskId::from(1),
        &workflow,
    )
    .unwrap();
    let (mut doc, mut task) = (parsed.document, parsed.task);
    task.set_status(Status::BLOCKED);
    diff::apply(&mut doc, &task, &workflow).unwrap();
    assert_eq!(format::render(&doc), "# [ ] T\n\n## Tags\n\n#B #blocked\n");
}
