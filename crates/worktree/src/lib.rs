//! Runs `git` for the worktrees tasks declare (ADR 0022,
//! `ruli/features/worktrees/PLAN.md`): where a branch comes from, and
//! `git worktree add` at the declared path. The decisions are
//! [`tasq_core::worktree::lifecycle`]'s; this crate carries them out.
//!
//! Every process is run with an explicitly injected environment, so tests
//! point `PATH` and `HOME` at a temporary tree.

#![warn(missing_docs)]

pub mod git;
pub mod process;

pub use self::git::{Git, GitError, add_args, describe};
