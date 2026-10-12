//! Runs `git` for the worktrees tasks declare (ADR 0022,
//! `ruli/features/worktrees/PLAN.md`): where a branch comes from,
//! `git worktree add` at the declared path, the provisioning that
//! follows (copies, links, rendered templates) and the hooks around it.
//! The decisions are [`tasq_core::worktree`]'s; this crate carries them
//! out.
//!
//! Every process is run with an explicitly injected environment, so tests
//! point `PATH` and `HOME` at a temporary tree.

#![warn(missing_docs)]

pub mod create;
pub mod git;
pub mod hooks;
pub mod process;
pub mod provision;
pub mod repo;
pub mod settings;

pub use self::git::{Git, GitError, add_args, describe};
pub use self::repo::Repo;
pub use self::settings::RepoSettings;
