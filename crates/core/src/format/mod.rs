//! The nb-compatible markdown task format: lossless parser, writer and edit
//! operations.
//!
//! Two layers (plan section 4.4, `docs/file-format.md`):
//!
//! - [`Document`] keeps the file byte for byte: lines with their endings,
//!   unknown sections, blank lines, trailing newline. [`parse`] accepts any
//!   text whose first line is `# [ ] Title` or `# [x] Title` and
//!   [`render`] gives the same text back.
//! - [`Task`] is the typed projection of a document, computed by [`parse`].
//!   It is read-only with respect to the file: edits are expressed as
//!   operations on the document (the [`ops`] module) that reproduce the
//!   original script's insertion rules exactly, so the store layer applies
//!   operations rather than diffing tasks. [`Document::from_task`] writes a
//!   whole task as a new file for creation.
//!
//! ```
//! use tasq_core::format::{self, ops};
//! use tasq_core::model::{Status, TaskId, Workflow};
//!
//! let text = "# [ ] Title\n\n## Tags\n\n#gitlab #A #ready\n";
//! let workflow = Workflow::default();
//! let mut parsed = format::parse(text, TaskId::from(1), &workflow).unwrap();
//! assert_eq!(parsed.task.status, Some(Status::READY));
//! assert_eq!(format::render(&parsed.document), text);
//!
//! ops::set_status(&mut parsed.document, &Status::BLOCKED, &workflow);
//! assert_eq!(
//!     format::render(&parsed.document),
//!     "# [ ] Title\n\n## Tags\n\n#gitlab #A #blocked\n"
//! );
//! ```

mod document;
mod entry;
pub mod ops;
mod read;
mod write;

pub use self::document::{DONE_PREFIX, Document, FormatError, Newline, OPEN_PREFIX, Section};
pub use self::entry::{SESSION_SEPARATOR, format_session, format_worktree};
pub use self::read::section;

use crate::model::{Task, TaskId, Workflow};

/// A parsed task file: the lossless document and its typed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The file, byte for byte.
    pub document: Document,
    /// What the file says about the task.
    pub task: Task,
}

/// Parses a task file. `id` is the task's position in the notebook index
/// (the file does not know it); `workflow` says which `#tags` are statuses.
pub fn parse(text: &str, id: TaskId, workflow: &Workflow) -> Result<Parsed, FormatError> {
    let document = Document::parse(text)?;
    let task = read::project(&document, id, workflow);
    Ok(Parsed { document, task })
}

/// Projects a document onto a task; what [`parse`] does after splitting lines.
pub fn project(document: &Document, id: TaskId, workflow: &Workflow) -> Task {
    read::project(document, id, workflow)
}

/// Writes a document back. Same as [`Document::render`].
pub fn render(document: &Document) -> String {
    document.render()
}
