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
            let plugins = &app.config().plugins.enabled;
            if app.out.json_mode() {
                return app.out.json(&info_json(&info, &provenance, plugins));
            }
            app.out.print(&render_info(&info, &provenance, plugins))
        }
    }
}

/// The `store info` document: `"store"` holds the info, `"plugins"` the
/// enabled plugin names, `"data_dir"` and `"store_name"` are
/// `{value, from}`.
fn info_json(info: &StoreInfo, provenance: &Provenance, plugins: &[String]) -> Value {
    let setting = |value: Value, from: &str| serde_json::json!({ "value": value, "from": from });
    json::document([
        ("store", json::to_value(info)),
        ("plugins", Value::from(plugins)),
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

/// The `store info` text; `plugins:` names the enabled plugins, `(none)`
/// when there are none.
pub fn render_info(info: &StoreInfo, provenance: &Provenance, plugins: &[String]) -> String {
    let plugins = if plugins.is_empty() {
        "(none)".to_owned()
    } else {
        plugins.join(", ")
    };
    format!(
        "store:       {}\nlocation:    {}\ndata_dir:    {} (from {})\nstore.name:  {} (from {})\ntasks:       {} (open and done)\nplugins:     {plugins}\n",
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
        let head = "store:       tasq\nlocation:    /data/work\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\n";
        assert_eq!(
            render_info(&info(), &provenance(), &["git".into(), "echo".into()]),
            format!("{head}plugins:     git, echo\n")
        );
        assert_eq!(
            render_info(&info(), &provenance(), &[]),
            format!("{head}plugins:     (none)\n")
        );
    }

    #[test]
    fn info_json_settings_and_plugins() {
        let value = info_json(&info(), &provenance(), &["git".into()]);
        assert_eq!(value["plugins"], serde_json::json!(["git"]));
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
        let value = info_json(&info(), &provenance(), &[]);
        assert_eq!(value["plugins"], serde_json::json!([]));
    }
}
