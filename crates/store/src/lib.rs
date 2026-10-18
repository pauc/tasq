//! The crate that becomes the tasq-owned [`Store`] of ADR-0026 (a directory
//! of `<ULID>.md` task files with a frontmatter block, under
//! `<data_dir>/<store.name>`), built in slices (`ruli/features/store-v2/PLAN.md`).
//!
//! So far it holds the two pieces every file store needs, moved out of the nb
//! store so both share them:
//!
//! - [`Revision`]: change detection for a task file (mtime, length, hash), so
//!   an update refuses to overwrite a file that changed since it was read.
//! - [`diff::apply`]: turns a whole-task write into the edit operations of
//!   [`tasq_core::format::ops`] and reports the fields they could not express.
//!
//! [`Store`]: tasq_core::store::Store

#![warn(missing_docs)]

pub mod diff;
pub mod revision;

pub use self::revision::Revision;
