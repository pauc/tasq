//! Core domain of `tasq`: the task model, the `Store`, `Source` and `Launcher`
//! extension traits, configuration, reconciliation and reports.
//!
//! This crate has no I/O of its own; the other workspace crates implement the
//! traits it defines.
//!
//! # Mutation testing
//!
//! The workspace is checked with `cargo-mutants` (`just mutants`, configured
//! in `.cargo/mutants.toml`). Every function in this crate is expected to be
//! killed by at least one test. The only functions allowed to opt out are
//! those that merely wrap I/O or `exec` (process replacement, terminal setup)
//! and therefore cannot be observed from a unit test. Mark them with
//!
//! ```ignore
//! /// Reason: thin wrapper around `std::fs::read_to_string`, nothing to assert.
//! #[mutants::skip]
//! fn read_file(path: &Path) -> io::Result<String> { /* ... */ }
//! ```
//!
//! always with a one-line reason. Use the plain `#[mutants::skip]` form:
//! `#[cfg_attr(test, mutants::skip)]` compiles and is recognised too, but
//! cargo-mutants finds the attribute by scanning the source text and never
//! evaluates the `cfg_attr` condition, so the condition only adds confusion
//! (source: <https://mutants.rs/attrs.html>). The attribute comes from the
//! tiny `mutants` crate, a regular dependency because it sits on non-test code.

#![warn(missing_docs)]

pub mod clock;
pub mod config;
pub mod dates;
pub mod edit;
pub mod format;
pub mod launch;
pub mod model;
pub mod query;
pub mod report;
pub mod source;
pub mod store;
pub mod theme;
pub mod work;
