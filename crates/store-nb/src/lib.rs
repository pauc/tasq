//! nb-compatible markdown [`Store`] for `tasq`: reads and writes todo files in
//! an nb notebook (ADR-0002) without ever running `nb` (ADR-0023).
//!
//! - [`NbStore::open`] resolves the configured notebook (`$NB_DIR/<name>`,
//!   or a path) and reads its `.index`, whose line numbers are the task ids.
//! - Reads parse the files directly; no process is spawned.
//! - Writes rewrite one file atomically with the edit operations of
//!   [`tasq_core::format::ops`], refuse to overwrite a file that changed since
//!   it was read, and then hand over to a [`Bookkeeper`]: index registration,
//!   then a commit (and, every [`DEFAULT_SYNC_INTERVAL`] seconds when the
//!   caller asks for it, a pull and push) in a background job, so no write
//!   waits for git or the network.
//! - [`Store::create`] writes a new `YYYYMMDDHHMMSS.todo.md`, registers it
//!   through the bookkeeper and reads the id back from `.index`.
//! - [`doctor::checks`] produces the store part of `tasq doctor`.
//!
//! `git` is only ever run with the environment the caller injects through
//! [`NbStoreOptions`], so tests can point it at a temporary notebook.
//!
//! [`Store`]: tasq_core::store::Store

#![warn(missing_docs)]

pub mod bookkeeper;
pub mod create;
pub mod diff;
pub mod doctor;
pub mod git;
pub mod index;
pub mod job;
pub mod native;
pub mod resolve;
pub mod revision;
pub mod store;
pub mod which;

pub use self::bookkeeper::{
    Bookkeeper, NativeBookkeeper, NoopBookkeeper, SyncOutcome, Verification, WriteLock,
};
pub use self::doctor::{Check, CheckStatus};
pub use self::index::{Index, TODO_SUFFIX};
pub use self::revision::Revision;
pub use self::store::{DEFAULT_SYNC_INTERVAL, NbStore, NbStoreOptions, StoreWarning};
pub use tasq_core::store::{BatchError, IdScheme, Store, StoreError, StoreInfo};
