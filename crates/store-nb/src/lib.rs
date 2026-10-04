//! nb-compatible markdown [`Store`] for `tasq`: reads and writes todo files in
//! an nb notebook (ADR-0002, ADR-0007).
//!
//! - [`NbStore::open`] resolves the configured notebook (`$NB_DIR/<name>`,
//!   else `nb notebooks show <name> --path`) and reads its `.index`, whose
//!   line numbers are the task ids.
//! - Reads parse the files directly; no process is spawned.
//! - Writes rewrite one file atomically with the edit operations of
//!   [`tasq_core::format::ops`], refuse to overwrite a file that changed since
//!   it was read, and then hand over to a [`Bookkeeper`] (index and commits).
//! - [`Store::create`] writes a new `YYYYMMDDHHMMSS.todo.md`, registers it
//!   through the bookkeeper and reads the id back from `.index`.
//!
//! `nb` is only ever run with the environment the caller injects through
//! [`NbStoreOptions`], so tests can point it at a temporary notebook.
//!
//! [`Store`]: tasq_core::store::Store

#![warn(missing_docs)]

pub mod bookkeeper;
pub mod create;
pub mod diff;
pub mod git;
pub mod index;
pub mod native;
pub mod nb;
pub mod nb_cli;
pub mod resolve;
pub mod revision;
pub mod sanitize;
pub mod store;

pub use self::bookkeeper::{
    Bookkeeper, NativeBookkeeper, NbCliBookkeeper, NoopBookkeeper, SyncOutcome, Verification,
    select_bookkeeper,
};
pub use self::index::{Index, TODO_SUFFIX};
pub use self::nb::{Nb, NbError};
pub use self::revision::Revision;
pub use self::store::{NbStore, NbStoreOptions, StoreWarning};
pub use tasq_core::store::{IdScheme, Store, StoreError, StoreInfo};
