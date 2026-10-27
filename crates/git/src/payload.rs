//! The event payload tasq writes on the plugin's stdin (`docs/plugins.md`,
//! "Payloads"). Only the fields the plugin reads are kept; unknown ones are
//! ignored, as schema 1 only ever adds fields.

use std::path::PathBuf;

use serde::Deserialize;

/// The payload schema this plugin reads.
pub const SCHEMA: u64 = 1;

/// One event, as tasq sends it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Payload {
    /// `store-open`, `post-write`, `doctor`, or one added later.
    pub event: String,
    /// The store the event is about.
    pub store: Store,
    /// The `[plugin.git]` table, `{}` when there is none.
    #[serde(default)]
    pub config: serde_json::Map<String, serde_json::Value>,
    /// `post-write` only: the command that wrote.
    #[serde(default)]
    pub command: Option<String>,
    /// `post-write` only: the tasks it wrote.
    #[serde(default)]
    pub writes: Option<Vec<Write>>,
}

/// One entry of a `post-write` payload's `writes`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Write {
    /// The task id, as a string.
    pub id: String,
}

/// The `store` object of a payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Store {
    /// The store directory, absolute. It may not exist yet.
    pub location: PathBuf,
}

/// Why a payload was refused.
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    /// Not a JSON document.
    #[error("the event payload is not JSON: {0}")]
    NotJson(serde_json::Error),
    /// A JSON document without a `schema` field.
    #[error("the event payload has no schema")]
    NoSchema,
    /// A schema other than [`SCHEMA`], as it appeared in the payload.
    #[error("unsupported event payload schema {0} (tasq-git reads schema {SCHEMA})")]
    Unsupported(serde_json::Value),
    /// A schema 1 payload missing a field the plugin needs.
    #[error("the schema {SCHEMA} event payload is malformed: {0}")]
    Malformed(serde_json::Error),
}

/// Parses `text`, refusing any schema but [`SCHEMA`]. The schema is checked
/// before the fields, so a later schema is named rather than reported as a
/// missing field.
pub fn parse(text: &str) -> Result<Payload, PayloadError> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(PayloadError::NotJson)?;
    match doc.get("schema") {
        None => Err(PayloadError::NoSchema),
        Some(schema) if schema.as_u64() == Some(SCHEMA) => {
            serde_json::from_value(doc).map_err(PayloadError::Malformed)
        }
        Some(schema) => Err(PayloadError::Unsupported(schema.clone())),
    }
}
