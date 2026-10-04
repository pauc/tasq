//! The bookkeeping that follows a write: registering a new file in `.index`
//! and committing. Plan T-206 provides the `nb` CLI and native git
//! implementations; this module holds the trait they implement and the
//! do-nothing strategy the store uses until one is attached.

use std::path::Path;

use tasq_core::store::StoreError;

/// Keeps the notebook's `.index` and history consistent after the store
/// wrote a file (ADR-0007).
pub trait Bookkeeper {
    /// Strategy name, for diagnostics (`nb`, `native`, `none`).
    fn name(&self) -> &str;

    /// Adds a newly created `file` (a path inside the notebook) to the index.
    fn register(&self, file: &Path) -> Result<(), StoreError>;

    /// Commits pending changes with `message`; a no-op when nothing changed.
    fn checkpoint(&self, message: &str) -> Result<(), StoreError>;

    /// Checks that the index matches the files.
    fn verify(&self) -> Result<(), StoreError>;
}

/// Does no bookkeeping at all: files are written, nothing else happens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopBookkeeper;

impl Bookkeeper for NoopBookkeeper {
    fn name(&self) -> &'static str {
        "none"
    }

    fn register(&self, _file: &Path) -> Result<(), StoreError> {
        Ok(())
    }

    fn checkpoint(&self, _message: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn verify(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_succeeds_at_everything() {
        let b = NoopBookkeeper;
        assert_eq!(b.name(), "none");
        assert!(b.register(Path::new("/x/y.todo.md")).is_ok());
        assert!(b.checkpoint("[tasq] Update: y.todo.md").is_ok());
        assert!(b.verify().is_ok());
    }
}
