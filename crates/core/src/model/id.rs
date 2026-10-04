//! Store-local task identifiers.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// Opaque, store-local identifier of a task.
///
/// With the nb store this is the line number of the todo in the notebook's
/// `.index` (`"23"`), but the model does not assume numbers: another store
/// may use slugs or UUIDs. Ids compare and sort as plain text. The only rule
/// is that an id is never empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TaskId(String);

impl TaskId {
    /// Wraps `id`, rejecting the empty string.
    pub fn new(id: impl Into<String>) -> Result<Self, ModelError> {
        let id = id.into();
        if id.is_empty() {
            Err(ModelError::EmptyId)
        } else {
            Ok(Self(id))
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TaskId {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TaskId {
    type Error = ModelError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<TaskId> for String {
    fn from(id: TaskId) -> Self {
        id.0
    }
}

impl From<u64> for TaskId {
    fn from(n: u64) -> Self {
        Self(n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_empty() {
        assert_eq!(TaskId::new(""), Err(ModelError::EmptyId));
        assert_eq!("".parse::<TaskId>(), Err(ModelError::EmptyId));
        assert_eq!(TaskId::try_from(String::new()), Err(ModelError::EmptyId));
    }

    #[test]
    fn new_keeps_text_verbatim() {
        let id = TaskId::new("23").unwrap();
        assert_eq!(id.as_str(), "23");
        assert_eq!(id.to_string(), "23");
        assert_eq!(String::from(id.clone()), "23");
        assert_eq!("abc-1".parse::<TaskId>().unwrap().as_str(), "abc-1");
    }

    #[test]
    fn from_number() {
        assert_eq!(TaskId::from(7), TaskId::new("7").unwrap());
    }

    #[test]
    fn ids_order_as_text() {
        assert!(TaskId::from(10) < TaskId::from(9));
    }

    #[test]
    fn serde_is_a_plain_string() {
        let id = TaskId::from(23);
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"23\"");
        assert_eq!(serde_json::from_str::<TaskId>("\"23\"").unwrap(), id);
        assert!(serde_json::from_str::<TaskId>("\"\"").is_err());
    }

    #[test]
    fn error_message() {
        assert_eq!(ModelError::EmptyId.to_string(), "task id must not be empty");
    }
}
