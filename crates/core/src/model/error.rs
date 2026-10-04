//! Validation errors of the model newtypes.

use thiserror::Error;

/// Why a value was rejected by one of the model constructors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    /// A task id must not be empty.
    #[error("task id must not be empty")]
    EmptyId,
    /// A tag was empty, started with `#`, or contained whitespace.
    #[error("invalid tag {tag:?}: {reason}")]
    InvalidTag {
        /// The rejected text.
        tag: String,
        /// Human-readable reason.
        reason: &'static str,
    },
    /// A status was not lowercase kebab-case (`in-progress`, `ready`, ...).
    #[error("invalid status {0:?}: expected lowercase words joined by '-'")]
    InvalidStatus(String),
    /// A priority was not `A`, `B` or `C` (optionally `#`-prefixed).
    #[error("invalid priority {0:?}: expected A, B or C")]
    InvalidPriority(String),
}
