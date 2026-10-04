//! `tasq apply [file]`: the stdin side of the JSON surface.

use std::io::Read;
use std::path::Path;

use serde_json::Value;
use tasq_core::model::Task;
use tasq_core::store::Store;

use crate::app::App;
use crate::commands::finish;
use crate::error::{CliError, Result};
use crate::json;

/// Reads `{"schema": 1, "task": {...}}` and writes the task.
pub fn run(app: &App, file: Option<&Path>) -> Result<()> {
    let text = if let Some(path) = file {
        std::fs::read_to_string(path)
            .map_err(|e| CliError::user(format!("apply: cannot read {}: {e}", path.display())))?
    } else {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    };
    let task = parse_document(&text)?;
    let mut store = app.open_store()?;
    store.update(&task)?;
    finish(app, &store, &task.id, &format!("[{}] applied\n", task.id))
}

/// The task inside a schema-1 document. Errors name the offending field.
pub fn parse_document(text: &str) -> Result<Task> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| CliError::user(format!("apply: invalid JSON: {e}")))?;
    let Value::Object(fields) = value else {
        return Err(CliError::user(
            "apply: expected a JSON object with \"schema\" and \"task\"",
        ));
    };
    match fields.get("schema") {
        None => return Err(CliError::user("apply: missing field `schema`")),
        Some(Value::Number(n)) if n.as_u64() == Some(json::SCHEMA) => {}
        Some(other) => {
            return Err(CliError::user(format!(
                "apply: unsupported schema {other} (expected {})",
                json::SCHEMA
            )));
        }
    }
    let task = fields
        .get("task")
        .ok_or_else(|| CliError::user("apply: missing field `task`"))?;
    serde_json::from_value::<Task>(task.clone())
        .map_err(|e| CliError::user(format!("apply: invalid task: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_the_problem() {
        let err = |t: &str| parse_document(t).unwrap_err().to_string();
        assert!(err("nope").starts_with("apply: invalid JSON:"));
        assert_eq!(
            err("[]"),
            "apply: expected a JSON object with \"schema\" and \"task\""
        );
        assert_eq!(err("{\"task\":{}}"), "apply: missing field `schema`");
        assert_eq!(
            err("{\"schema\":2,\"task\":{}}"),
            "apply: unsupported schema 2 (expected 1)"
        );
        assert_eq!(
            err("{\"schema\":\"1\",\"task\":{}}"),
            "apply: unsupported schema \"1\" (expected 1)"
        );
        assert_eq!(err("{\"schema\":1}"), "apply: missing field `task`");
        assert_eq!(
            err("{\"schema\":1,\"task\":{}}"),
            "apply: invalid task: missing field `id`"
        );
        assert!(
            err("{\"schema\":1,\"task\":{\"id\":\"3\",\"title\":\"T\",\"done\":false,\"status\":\"#ready\",\"priority\":\"B\",\"due\":null,\"description\":null,\"project\":null,\"tags\":[],\"related\":[],\"merge_requests\":[],\"worktrees\":[],\"sessions\":[],\"progress\":[],\"origin\":null}}")
                .contains("invalid status"),
        );
    }

    #[test]
    fn a_valid_document_parses() {
        let task = parse_document(
            "{\"schema\":1,\"task\":{\"id\":\"3\",\"title\":\"T\",\"done\":false,\"status\":\"ready\",\"priority\":\"A\",\"due\":\"2026-10-10\",\"description\":null,\"project\":null,\"tags\":[\"x\"],\"related\":[],\"merge_requests\":[],\"worktrees\":[],\"sessions\":[],\"progress\":[{\"at\":\"2026-10-04 10:15\",\"note\":\"n\"}],\"origin\":null}}",
        )
        .unwrap();
        assert_eq!(task.title, "T");
        assert_eq!(task.priority, tasq_core::model::Priority::A);
        assert_eq!(task.progress.len(), 1);
    }
}
