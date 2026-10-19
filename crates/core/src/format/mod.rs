//! The markdown task format: lossless parser, writer and edit operations.
//!
//! A task file (ADR 0026) opens with a frontmatter block (`---`, `key: value`
//! lines, `---`) holding the id and the scalar fields ([`key`]), with
//! `status: done` as the done marker, followed by the body: `# Title` and
//! the list sections. [`parse_v2`] reads one, [`Document::from_task_v2`]
//! writes a task as a canonical file, and the [`ops`] edit one: a scalar op
//! writes one frontmatter line, a list op one line of the body.
//!
//! Two layers (plan section 4.4, `docs/file-format.md`):
//!
//! - [`Document`] keeps the file byte for byte: lines with their endings, the
//!   frontmatter block with its unknown keys and blank lines, unknown
//!   sections, trailing newline. [`render`] gives the parsed text back.
//! - [`Task`] is the typed projection of a document, computed by [`parse_v2`].
//!   It is read-only with respect to the file: edits are expressed as
//!   operations on the document (the [`ops`] module) that keep the
//!   insertion rules the original script had for the list sections and edit
//!   one frontmatter line for a scalar, so the store layer applies
//!   operations rather than diffing tasks. [`Document::from_task_v2`] writes
//!   a whole task as a new file for creation.
//!
//! ```
//! use tasq_core::format::{self, ops};
//! use tasq_core::model::{Status, Workflow};
//!
//! let text = "---\nid: 1\nstatus: ready\ntags: [gitlab]\n---\n# Title\n";
//! let workflow = Workflow::default();
//! let mut parsed = format::parse_v2(text, None, &workflow).unwrap();
//! assert_eq!(parsed.task.status, Some(Status::READY));
//! assert_eq!(format::render(&parsed.document), text);
//!
//! ops::set_status(&mut parsed.document, &Status::BLOCKED);
//! assert_eq!(
//!     format::render(&parsed.document),
//!     "---\nid: 1\nstatus: blocked\ntags: [gitlab]\n---\n# Title\n"
//! );
//! ```

mod document;
mod entry;
pub mod ops;
mod read;
mod write;

pub use self::document::{
    Document, FENCE, FormatError, Frontmatter, Newline, Section, TITLE_PREFIX, key,
};
pub use self::entry::{SESSION_SEPARATOR, format_session, format_worktree};
pub use self::read::section;

use chrono::NaiveDateTime;

use crate::model::{Task, Workflow};

/// A parsed task file: the lossless document and its typed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The file, byte for byte.
    pub document: Document,
    /// What the file says about the task.
    pub task: Task,
}

/// Parses a task file: the id comes from the frontmatter and `created_at`
/// is what the caller knows (a tasq store reads it from the ULID file
/// name). `workflow` says which status names are statuses. Any file whose
/// first line is not `---` is [`FormatError::NotFrontmatter`].
pub fn parse_v2(
    text: &str,
    created_at: Option<NaiveDateTime>,
    workflow: &Workflow,
) -> Result<Parsed, FormatError> {
    let document = Document::parse(text)?;
    let mut task = read::project(&document, workflow);
    task.created_at = created_at;
    Ok(Parsed { document, task })
}

/// Projects a document onto a task; what [`parse_v2`] does after splitting
/// lines. The task's `created_at` is `None`.
pub fn project(document: &Document, workflow: &Workflow) -> Task {
    read::project(document, workflow)
}

/// Writes a document back. Same as [`Document::render`].
pub fn render(document: &Document) -> String {
    document.render()
}
