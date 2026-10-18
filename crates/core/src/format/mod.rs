//! The markdown task format: lossless parser, writer and edit operations.
//!
//! Two shapes of file (ADR 0026): **v1**, the nb notebook shape, where line 1
//! is `# [ ] Title` or `# [x] Title` and every field is a `## Section`; and
//! **v2**, where a frontmatter block (`---`, `key: value` lines, `---`) holds
//! the id and the scalar fields ([`key`]), `status: done` is the done marker,
//! and the body is `# Title` plus the list sections. [`Document::parse`] reads
//! both by the first line; [`parse`] projects either, [`parse_v2`] a v2 file
//! only. Writing v2 files is the next slice (SV-004 in
//! `ruli/features/store-v2/PLAN.md`); [`Document::from_task`] and the scalar
//! [`ops`] write the v1 shape.
//!
//! Two layers (plan section 4.4, `docs/file-format.md`):
//!
//! - [`Document`] keeps the file byte for byte: lines with their endings, the
//!   frontmatter block with its unknown keys and blank lines, unknown
//!   sections, trailing newline. [`render`] gives the parsed text back.
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

pub use self::document::{
    DONE_PREFIX, Document, FENCE, FormatError, Frontmatter, Newline, OPEN_PREFIX, Section,
    TITLE_PREFIX,
};
pub use self::entry::{SESSION_SEPARATOR, format_session, format_worktree};
pub use self::read::{key, section};

use chrono::NaiveDateTime;

use crate::model::{Task, TaskId, Workflow};

/// A parsed task file: the lossless document and its typed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The file, byte for byte.
    pub document: Document,
    /// What the file says about the task.
    pub task: Task,
}

/// Parses a task file of either shape. `id` names a v1 task, whose file
/// does not know it (its position in the notebook index); a v2 file carries
/// its own id and `id` is not used. `workflow` says which status names are
/// statuses. The task's `created_at` is `None`; see [`parse_v2`].
pub fn parse(text: &str, id: TaskId, workflow: &Workflow) -> Result<Parsed, FormatError> {
    let document = Document::parse(text)?;
    let task = read::project(&document, id, workflow);
    Ok(Parsed { document, task })
}

/// Parses a v2 task file (ADR 0026): the id comes from the frontmatter and
/// `created_at` is what the caller knows (a tasq store reads it from the
/// ULID file name). A file that does not start with `---` is
/// [`FormatError::NotFrontmatter`].
pub fn parse_v2(
    text: &str,
    created_at: Option<NaiveDateTime>,
    workflow: &Workflow,
) -> Result<Parsed, FormatError> {
    let document = Document::parse(text)?;
    let Some(id) = document.id() else {
        return Err(FormatError::NotFrontmatter {
            first_line: document.title_line().to_owned(),
        });
    };
    let mut task = read::project(&document, id, workflow);
    task.created_at = created_at;
    Ok(Parsed { document, task })
}

/// Projects a document onto a task; what [`parse`] does after splitting
/// lines. `id` is used for a v1 document only, as in [`parse`].
pub fn project(document: &Document, id: TaskId, workflow: &Workflow) -> Task {
    read::project(document, id, workflow)
}

/// Writes a document back. Same as [`Document::render`].
pub fn render(document: &Document) -> String {
    document.render()
}
