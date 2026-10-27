//! The `tasq-git` plugin (ADR 0030, `ruli/features/git-sync/PLAN.md`):
//! store sync for a tasq store that is a git repository.
//!
//! tasq runs `tasq-git hook` for every event, with the payload of
//! `docs/plugins.md` on stdin. A store that is not a repository is left
//! alone: its events do nothing and `doctor` answers that sync is off.
//!
//! For a store that is, `post-write`, and `store-open` once the last fetch
//! is older than the interval, detach `tasq-git job`, which commits the
//! task files in the background and, when the branch has an upstream,
//! fetches, rebases (under `tasq store lock`) and pushes (`job`, `state`).
//!
//! `tasq git init` (run by tasq's dispatch as `tasq-git init`) makes the
//! store a repository and, given a URL, pushes it to a branch named after
//! the store; `tasq git clone` fills an empty store from such a branch;
//! `tasq git sync` runs the job's steps in the foreground. `doctor` explains
//! the repository's state (`doctor`).
//!
//! The crate depends on no tasq crate: everything it knows about tasq comes
//! from the payloads, `tasq ... --json` and `tasq store lock`.

#![warn(missing_docs)]

pub mod clone;
pub mod doctor;
pub mod git;
pub mod hook;
pub mod init;
pub mod job;
pub mod payload;
pub mod process;
pub mod state;
pub mod tasq;

pub use self::payload::{Payload, PayloadError};
