//! Core domain of `tasq`: the task model, the `Store`, `Source` and `Launcher`
//! extension traits, configuration, reconciliation and reports.
//!
//! This crate has no I/O of its own; the other workspace crates implement the
//! traits it defines.

#![warn(missing_docs)]

pub mod clock;
pub mod config;
pub mod format;
pub mod model;
pub mod query;
