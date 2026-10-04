//! `tasq store info` and `tasq store sync`.

use serde_json::Value;
use tasq_core::store::{IdScheme, Store, StoreInfo};

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
                return app.out.json(&json::document([
                    ("store", json::to_value(&info)),
                    ("bookkeeper", Value::from(store.bookkeeper().name())),
                ]));
            }
            app.out
                .print(&render_info(&info, store.bookkeeper().name()))
        }
        StoreCommand::Sync => {
            let outcome = store.bookkeeper().sync()?;
            if let Some(raw) = &outcome.raw_output {
                app.out.verbose(raw);
            }
            if app.out.json_mode() {
                return app.out.json(&json::document([
                    ("synced", Value::from(outcome.synced)),
                    ("detail", Value::from(outcome.detail.as_str())),
                ]));
            }
            app.out.print(&format!("{}\n", outcome.detail))
        }
    }
}

/// The `store info` text.
pub fn render_info(info: &StoreInfo, bookkeeper: &str) -> String {
    let ids = match info.id_scheme {
        IdScheme::Stable => "stable".to_owned(),
        IdScheme::Positional => format!(
            "positional (line numbers in .index){}",
            if info.ids_may_change_on_reconcile {
                "; they can change after 'nb index reconcile' or deletions"
            } else {
                ""
            }
        ),
    };
    format!(
        "store:       {}\nlocation:    {}\ntasks:       {} (open and done)\nids:         {ids}\nbookkeeper:  {bookkeeper}\n",
        info.name,
        info.location.display(),
        info.task_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_text() {
        let info = StoreInfo {
            name: "nb".into(),
            location: "/nb/home".into(),
            task_count: 6,
            id_scheme: IdScheme::Positional,
            ids_may_change_on_reconcile: true,
        };
        assert_eq!(
            render_info(&info, "native"),
            "store:       nb\nlocation:    /nb/home\ntasks:       6 (open and done)\nids:         positional (line numbers in .index); they can change after 'nb index reconcile' or deletions\nbookkeeper:  native\n"
        );
        let stable = StoreInfo {
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            ..info
        };
        assert!(render_info(&stable, "none").contains("ids:         stable\n"));
    }
}
