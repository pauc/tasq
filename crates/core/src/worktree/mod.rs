//! Worktrees as an opt-in feature (ADR 0022, `ruli/features/worktrees/PLAN.md`).
//!
//! A `## Worktrees` entry is a declaration: the path and branch a task works
//! in. It is written when the task is created, the worktree is created when
//! the task is launched and removed when it is closed, and the entry stays so
//! the next launch can create it again. This module holds the pure decisions;
//! running `git` and touching the filesystem is the caller's job.

pub mod declaration;
pub mod git;
pub mod hooks;
pub mod integration;
pub mod lifecycle;
pub mod provision;
pub mod status;
pub mod vars;
