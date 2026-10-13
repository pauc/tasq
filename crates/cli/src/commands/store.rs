//! `tasq store info`, `tasq store sync` and `tasq store reconcile`.

use serde_json::Value;
use tasq_core::store::{IdScheme, Store, StoreInfo};

use tasq_store_nb::Verification;

use crate::app::App;
use crate::cli::StoreCommand;
use crate::error::Result;
use crate::json;

/// Runs a `store` subcommand.
pub fn run(app: &App, command: StoreCommand) -> Result<()> {
    let mut store = app.open_store()?;
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
            store.wait_for_bookkeeping()?;
            for warning in store.take_warnings() {
                app.out.warn(&warning.to_string());
            }
            let outcome = store.sync()?;
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
        StoreCommand::Reconcile => {
            let found = store.reconcile()?;
            for warning in store.take_warnings() {
                app.out.warn(&warning.to_string());
            }
            if app.out.json_mode() {
                return app.out.json(&json::document([
                    ("changed", Value::from(!found.consistent)),
                    ("blanked", Value::from(found.missing_files.clone())),
                    ("appended", Value::from(found.unindexed_files.clone())),
                ]));
            }
            app.out.print(&render_reconcile(&found))
        }
    }
}

/// The `store reconcile` text: one line per change, or that there was
/// nothing to do.
pub fn render_reconcile(found: &Verification) -> String {
    if found.consistent {
        return "index already matches the files\n".to_owned();
    }
    let blanked = found
        .missing_files
        .iter()
        .map(|name| format!("blanked  {name} (file is gone; its id stays unused)\n"));
    let appended = found
        .unindexed_files
        .iter()
        .map(|name| format!("appended {name}\n"));
    blanked.chain(appended).collect()
}

/// The `store info` text.
pub fn render_info(info: &StoreInfo, bookkeeper: &str) -> String {
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
            "store:       nb\nlocation:    /nb/home\ntasks:       6 (open and done)\nids:         positional (line numbers in .index); they can change when another tool rewrites .index (nb index reconcile)\nbookkeeper:  native\n"
        );
        let stable = StoreInfo {
            id_scheme: IdScheme::Stable,
            ids_may_change_on_reconcile: false,
            ..info
        };
        assert!(render_info(&stable, "none").contains("ids:         stable\n"));
    }

    #[test]
    fn reconcile_text() {
        let mut found = Verification::consistent("fine");
        assert_eq!(
            render_reconcile(&found),
            "index already matches the files\n"
        );
        found.consistent = false;
        found.missing_files = vec!["gone.todo.md".into()];
        found.unindexed_files = vec!["a.md".into(), "b.todo.md".into()];
        assert_eq!(
            render_reconcile(&found),
            "blanked  gone.todo.md (file is gone; its id stays unused)\nappended a.md\nappended b.todo.md\n"
        );
    }
}
