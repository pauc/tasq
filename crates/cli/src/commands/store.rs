//! `tasq store info`.

use serde_json::Value;
use tasq_core::store::{IdScheme, StoreInfo};

use crate::app::App;
use crate::cli::StoreCommand;
use crate::error::Result;
use crate::json;

/// Runs a `store` subcommand.
pub fn run(app: &App, command: StoreCommand) -> Result<()> {
    let store = app.open_store()?;
    match command {
        StoreCommand::Info => {
            let info = store.describe();
            if app.out.json_mode() {
                return app.out.json(&info_json(info));
            }
            app.out.print(&render_info(&info))
        }
    }
}

/// The `store info` document: `"store"` holds the info without its
/// bookkeeper, which is the top-level `"bookkeeper"` (`null` for a store
/// that has none).
fn info_json(mut info: StoreInfo) -> Value {
    let bookkeeper = info.bookkeeper.take();
    json::document([
        ("store", json::to_value(&info)),
        ("bookkeeper", Value::from(bookkeeper)),
    ])
}

/// The `store info` text; the `bookkeeper:` line only for a store that has
/// one.
pub fn render_info(info: &StoreInfo) -> String {
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
        "store:       {}\nlocation:    {}\ntasks:       {} (open and done)\nids:         {ids}\n{bookkeeper}",
        info.name,
        info.location.display(),
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

    #[test]
    fn info_text() {
        assert_eq!(
            render_info(&nb_info()),
            "store:       nb\nlocation:    /nb/home\ntasks:       6 (open and done)\nids:         positional (line numbers in .index); they can change when another tool rewrites .index (nb index reconcile)\nbookkeeper:  native\n"
        );
        let stable = StoreInfo {
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            bookkeeper: None,
            ..nb_info()
        };
        assert_eq!(
            render_info(&stable),
            "store:       nb\nlocation:    /nb/home\ntasks:       6 (open and done)\nids:         stable\n"
        );
        let unmoving = StoreInfo {
            ids_may_change_on_reconcile: false,
            ..nb_info()
        };
        assert!(
            render_info(&unmoving).contains("ids:         positional (line numbers in .index)\n"),
            "{}",
            render_info(&unmoving)
        );
    }

    #[test]
    fn info_json_keeps_the_bookkeeper_at_the_top_level() {
        let value = info_json(nb_info());
        assert_eq!(value["bookkeeper"], "native");
        assert_eq!(value["store"]["name"], "nb");
        assert_eq!(value["store"].get("bookkeeper"), None);
        let value = info_json(StoreInfo {
            bookkeeper: None,
            ..nb_info()
        });
        assert_eq!(value["bookkeeper"], Value::Null);
    }
}
