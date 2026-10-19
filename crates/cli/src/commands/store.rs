//! `tasq store info`.

use serde_json::Value;
use tasq_core::store::StoreInfo;

use crate::app::{App, Provenance};
use crate::cli::StoreCommand;
use crate::error::Result;
use crate::json;

/// Runs a `store` subcommand.
pub fn run(app: &App, command: StoreCommand) -> Result<()> {
    let store = app.open_store()?;
    match command {
        StoreCommand::Info => {
            let info = store.describe();
            let provenance = app.store_provenance()?;
            if app.out.json_mode() {
                return app.out.json(&info_json(info, &provenance));
            }
            app.out.print(&render_info(&info, &provenance))
        }
    }
}

/// The `store info` document: `"store"` holds the info without its
/// bookkeeper, which is the top-level `"bookkeeper"` (`null` for a store
/// that has none); `"data_dir"` and `"store_name"` are `{value, from}`.
fn info_json(mut info: StoreInfo, provenance: &Provenance) -> Value {
    let bookkeeper = info.bookkeeper.take();
    let setting = |value: Value, from: &str| serde_json::json!({ "value": value, "from": from });
    json::document([
        ("store", json::to_value(&info)),
        ("bookkeeper", Value::from(bookkeeper)),
        (
            "data_dir",
            setting(
                Value::from(provenance.data_dir.display().to_string()),
                &provenance.data_dir_from,
            ),
        ),
        (
            "store_name",
            setting(Value::from(provenance.name.as_str()), &provenance.name_from),
        ),
    ])
}

/// The `store info` text; the `bookkeeper:` line only for a store that has
/// one.
pub fn render_info(info: &StoreInfo, provenance: &Provenance) -> String {
    let bookkeeper = info
        .bookkeeper
        .as_deref()
        .map(|name| format!("bookkeeper:  {name}\n"))
        .unwrap_or_default();
    format!(
        "store:       {}\nlocation:    {}\ndata_dir:    {} (from {})\nstore.name:  {} (from {})\ntasks:       {} (open and done)\n{bookkeeper}",
        info.name,
        info.location.display(),
        provenance.data_dir.display(),
        provenance.data_dir_from,
        provenance.name,
        provenance.name_from,
        info.task_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> StoreInfo {
        StoreInfo {
            name: "tasq".into(),
            location: "/data/work".into(),
            task_count: 6,
            bookkeeper: Some("git".into()),
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            data_dir: "/data".into(),
            data_dir_from: "env".into(),
            name: "work".into(),
            name_from: "--set".into(),
        }
    }

    #[test]
    fn info_text() {
        assert_eq!(
            render_info(&info(), &provenance()),
            "store:       tasq\nlocation:    /data/work\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\nbookkeeper:  git\n"
        );
        let without_bookkeeper = StoreInfo {
            bookkeeper: None,
            ..info()
        };
        assert_eq!(
            render_info(&without_bookkeeper, &provenance()),
            "store:       tasq\nlocation:    /data/work\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\n"
        );
    }

    #[test]
    fn info_json_keeps_the_bookkeeper_at_the_top_level() {
        let value = info_json(info(), &provenance());
        assert_eq!(value["bookkeeper"], "git");
        assert_eq!(
            value["data_dir"],
            serde_json::json!({"value": "/data", "from": "env"})
        );
        assert_eq!(
            value["store_name"],
            serde_json::json!({"value": "work", "from": "--set"})
        );
        assert_eq!(
            value["store"],
            serde_json::json!({"name": "tasq", "location": "/data/work", "task_count": 6})
        );
        let value = info_json(
            StoreInfo {
                bookkeeper: None,
                ..info()
            },
            &provenance(),
        );
        assert_eq!(value["bookkeeper"], Value::Null);
    }
}
