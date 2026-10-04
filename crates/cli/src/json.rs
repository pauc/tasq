//! The `--json` document shape.
//!
//! Every JSON document `tasq` prints is an object with a `"schema"` field
//! (currently [`SCHEMA`]) next to the payload, so scripts can detect a
//! future shape change. Tasks are serialised exactly as the core model
//! (`tasq_core::model::Task`), which is also what `tasq apply` reads back.

use serde_json::{Map, Value};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_carries_the_schema() {
        let doc = document([("tasks", Value::Array(Vec::new()))]);
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            "{\"schema\":1,\"tasks\":[]}"
        );
    }
}
