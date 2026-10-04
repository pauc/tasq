//! Domain model: tasks, status, priority, projects and related entities.
//!
//! The model is deliberately plain data plus a few small invariants:
//!
//! - [`Status`] and [`Priority`] are fields of [`Task`], **not** entries in
//!   [`Task::tags`]. The nb file stores all three as `#tags`; the format layer
//!   does that mapping, so the rest of the code never has to ask "is this tag
//!   really a status?".
//! - Statuses are configured through a [`Workflow`], not hard-coded. The five
//!   the original script used are the [`Workflow::default`] and available as
//!   constants on [`Status`].
//! - Newtypes ([`TaskId`], [`Status`], [`Tag`]) validate on construction and
//!   on deserialisation, so a `Tag` is never empty or `#`-prefixed.
//! - Timestamps are local wall-clock time without a zone (see [`crate::clock`]).
//!   Progress entries use [`When`] so that a legacy date-only entry is
//!   written back exactly as it was read.

mod error;
mod id;
mod priority;
mod status;
mod tag;
mod task;

pub use self::error::ModelError;
pub use self::id::TaskId;
pub use self::priority::Priority;
pub use self::status::{Status, Workflow};
pub use self::tag::Tag;
pub use self::task::{Link, Origin, ProgressEntry, Session, Task, TaskDraft, Worktree};
pub use crate::clock::When;
