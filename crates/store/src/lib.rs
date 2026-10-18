//! The tasq-owned [`Store`] of ADR-0026: [`TasqStore`], a directory of
//! `<ULID>.md` task files with a frontmatter block, under
//! `<data_dir>/<store.name>`, built in slices (`ruli/features/store-v2/PLAN.md`).
//!
//! - [`TasqStore`]: the store. Reads scan the directory on every call (the
//!   id map is the files' frontmatter, there is no index), never lock and
//!   never create it; a directory that does not exist is an empty store.
//!   `create` takes the store lock, allocates the highest id plus one and
//!   writes a new file; `update` takes it, refuses a file that changed
//!   since the task was read, edits the task's document with
//!   [`diff::apply`] and writes it over; `set_done` is the same write with
//!   the done ops and no revision check.
//! - [`lock`]: the store lock, `.tasq/lock`, held across a write; a stale
//!   one is broken, a live one waited on.
//! - [`ulid`]: the file name, whose timestamp is the task's creation time.
//! - [`draft`]: the check and the defaults a draft gets before it is
//!   written, shared with the nb store.
//! - [`Revision`]: change detection for a task file (mtime, length, hash), so
//!   an update refuses to overwrite a file that changed since it was read.
//! - [`diff::apply`]: turns a whole-task write into the edit operations of
//!   [`tasq_core::format::ops`] and reports the fields they could not express.
//!
//! The last two are shared with the nb store until it is deleted.
//!
//! [`Store`]: tasq_core::store::Store

#![warn(missing_docs)]

pub mod diff;
pub mod draft;
pub mod lock;
pub mod revision;
pub mod store;
pub mod ulid;

pub use self::revision::Revision;
pub use self::store::TasqStore;
