//! SV-008: a notebook migrated with `tasq_store::migrate` reads back through
//! the `Store` trait as the same tasks the nb store reads, plus their
//! creation time and, for a done task nb closed without a time, the time of
//! its last progress entry.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use chrono::NaiveDateTime;
use tasq_core::clock::FixedClock;
use tasq_core::model::Workflow;
use tasq_core::query::Filter;
use tasq_store::TasqStore;
use tasq_store_nb::Store;

use support::{NbEnv, file_name, id};

#[test]
fn the_migrated_store_reads_as_the_nb_store_plus_created_at() {
    let nb = NbEnv::fixture();
    let target = nb.root.path().join("store");
    let mut next = 0;
    tasq_store::migrate::plan(
        &nb.notebook(),
        &target,
        &Workflow::default(),
        &FixedClock::at("2026-10-04 12:34"),
        &mut || {
            next += 1;
            next
        },
    )
    .unwrap()
    .write()
    .unwrap();
    let all = Filter::default().any_done();
    let expected = nb.open().list(&all).unwrap();
    let migrated = TasqStore::new(&target, Workflow::default())
        .list(&all)
        .unwrap();
    assert_eq!(migrated.len(), expected.len());
    for (mut task, nb_task) in migrated.into_iter().zip(expected) {
        let number: u64 = nb_task.id.as_str().parse().unwrap();
        let second = NaiveDateTime::parse_from_str(
            file_name(number).trim_end_matches(".todo.md"),
            "%Y%m%d%H%M%S",
        )
        .unwrap();
        assert_eq!(task.created_at, Some(second), "task {number}");
        task.created_at = None;
        if number == id::DONE {
            assert_eq!(nb_task.closed_at, None, "nb closed it without a time");
            assert_eq!(task.closed_at, Some(second_of("2026-10-03 16:30:00")));
            task.closed_at = None;
        }
        assert_eq!(task, nb_task, "task {number}");
    }
}

/// `YYYY-MM-DD HH:MM:SS`.
fn second_of(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}
