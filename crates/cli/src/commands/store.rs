//! `tasq store info`.

use serde_json::Value;
use tasq_core::store::{IdScheme, StoreInfo};

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
    let ids = match info.id_scheme {
        IdScheme::Stable => "stable".to_owned(),
        IdScheme::Positional => format!(
            "positional (line numbers in .index){}",
            if info.ids_may_change_on_reconcile {
                "; they can change when another tool rewrites .index (nb index reconcile)"
            } else {
                ""
            }
        ),
    };
    let bookkeeper = info
        .bookkeeper
        .as_deref()
        .map(|name| format!("bookkeeper:  {name}\n"))
        .unwrap_or_default();
    format!(
        "store:       {}\nlocation:    {}\ndata_dir:    {} (from {})\nstore.name:  {} (from {})\ntasks:       {} (open and done)\nids:         {ids}\n{bookkeeper}",
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

    fn nb_info() -> StoreInfo {
        StoreInfo {
            name: "nb".into(),
            location: "/nb/home".into(),
            task_count: 6,
            id_scheme: IdScheme::Positional,
            ids_may_change_on_reconcile: true,
            bookkeeper: Some("native".into()),
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
            render_info(&nb_info(), &provenance()),
            "store:       nb\nlocation:    /nb/home\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\nids:         positional (line numbers in .index); they can change when another tool rewrites .index (nb index reconcile)\nbookkeeper:  native\n"
        );
        let stable = StoreInfo {
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            bookkeeper: None,
            ..nb_info()
        };
        assert_eq!(
            render_info(&stable, &provenance()),
            "store:       nb\nlocation:    /nb/home\ndata_dir:    /data (from env)\nstore.name:  work (from --set)\ntasks:       6 (open and done)\nids:         stable\n"
        );
        let unmoving = StoreInfo {
            ids_may_change_on_reconcile: false,
            ..nb_info()
        };
        assert!(
            render_info(&unmoving, &provenance())
                .contains("ids:         positional (line numbers in .index)\n"),
            "{}",
            render_info(&unmoving, &provenance())
        );
    }

    #[test]
    fn info_json_keeps_the_bookkeeper_at_the_top_level() {
        let value = info_json(nb_info(), &provenance());
        assert_eq!(value["bookkeeper"], "native");
        assert_eq!(
            value["data_dir"],
            serde_json::json!({"value": "/data", "from": "env"})
        );
        assert_eq!(
            value["store_name"],
            serde_json::json!({"value": "work", "from": "--set"})
        );
        assert_eq!(value["store"]["name"], "nb");
        assert_eq!(value["store"].get("bookkeeper"), None);
        let value = info_json(
            StoreInfo {
                bookkeeper: None,
                ..nb_info()
            },
            &provenance(),
        );
        assert_eq!(value["bookkeeper"], Value::Null);
    }
}
