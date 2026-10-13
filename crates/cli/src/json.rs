//! The `--json` document shape.
//!
//! Every JSON document `tasq` prints is an object with a `"schema"` field
//! (currently [`SCHEMA`]) next to the payload, so scripts can detect a
//! future shape change. Tasks are serialised exactly as the core model
//! (`tasq_core::model::Task`), which is also what `tasq apply` reads back.

use serde_json::{Map, Value};
use tasq_core::model::Task;

/// The current document schema version.
pub const SCHEMA: u64 = 1;

/// `{"schema": SCHEMA, <key>: <value>, ...}` from `(key, value)` pairs
/// (keys are emitted in sorted order).
pub fn document<I, K>(fields: I) -> Value
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<String>,
{
    let mut map = Map::new();
    map.insert("schema".to_owned(), Value::from(SCHEMA));
    for (key, value) in fields {
        map.insert(key.into(), value);
    }
    Value::Object(map)
}

/// Serialises `value` with `serde_json::to_value`, which cannot fail for
/// the plain data types of the model; a failure is a programming error.
pub fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("model types serialise to JSON")
}

/// A task as JSON. With worktrees disabled (`worktree.enabled = false`,
/// ADR 0022) the `worktrees` key is left out.
pub fn task(task: &Task, worktrees: bool) -> Value {
    let mut value = to_value(task);
    if !worktrees && let Value::Object(fields) = &mut value {
        fields.remove("worktrees");
    }
    value
}

/// Tasks as a JSON array, each as [`task`] renders it.
pub fn tasks<'a>(tasks: impl IntoIterator<Item = &'a Task>, worktrees: bool) -> Value {
    Value::Array(tasks.into_iter().map(|t| task(t, worktrees)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktrees_are_left_out_when_disabled() {
        let mut t = Task::new(tasq_core::model::TaskId::from(1), "T");
        t.add_worktree(tasq_core::model::Worktree::new("/w"));
        assert_eq!(task(&t, true)["worktrees"][0]["path"], "/w");
        assert_eq!(task(&t, false).get("worktrees"), None);
        assert_eq!(task(&t, false)["title"], "T");
        let both = tasks([&t, &t], false);
        assert_eq!(both.as_array().map(Vec::len), Some(2));
        assert_eq!(both[1].get("worktrees"), None);
    }

    #[test]
    fn document_carries_the_schema() {
        let doc = document([("tasks", Value::Array(Vec::new()))]);
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            "{\"schema\":1,\"tasks\":[]}"
        );
    }
}
