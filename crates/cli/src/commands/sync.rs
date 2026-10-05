//! `tasq sync`: run the sources, reconcile, apply (plan T-502).

use serde_json::Value;
use tasq_core::model::{Origin, Status, Task, TaskId};
use tasq_core::query::Filter;
use tasq_core::source::{self, Applied, Change, Defaults, SyncContext};
use tasq_core::store::Store;
use tasq_sources::{Built, build_sources, real_transport};

use crate::app::App;
use crate::error::{CliError, Result};
use crate::json;

/// What happened for one source.
#[derive(Debug, Default)]
struct Report {
    name: String,
    changes: Vec<String>,
    applied: Vec<Applied>,
    error: Option<String>,
}

/// Runs `sync`.
pub fn run(app: &App, only: &[String], dry_run: bool, ids: &[String]) -> Result<()> {
    let env = app.env_vec();
    let mut built = build_sources(app.config(), &env, &real_transport);
    if built.is_empty() {
        return Err(CliError::user(
            "no [[source]] is configured (see docs/config.md and docs/sources.md)",
        ));
    }
    let names = built
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if only.is_empty() {
        // A bare `sync` is the `auto` sources; `enabled` was applied above.
        let auto: Vec<&str> = app
            .config()
            .source
            .iter()
            .filter(|s| s.auto)
            .map(|s| s.name.as_str())
            .collect();
        built.retain(|(n, _)| auto.contains(&n.as_str()));
        if built.is_empty() {
            return Err(CliError::user(format!(
                "every enabled source has auto = false; name one with --source (sources: {names})"
            )));
        }
    } else {
        if let Some(name) = only
            .iter()
            .find(|name| !built.iter().any(|(n, _)| n == *name))
        {
            return Err(CliError::user(format!(
                "no enabled source called {name:?} (sources: {names})"
            )));
        }
        built.retain(|(n, _)| only.contains(n));
    }
    let ids = ids
        .iter()
        .map(|id| App::task_id(id))
        .collect::<Result<Vec<TaskId>>>()?;
    let mut store = app.open_store()?;
    let clock = app.clock()?;
    let default_status = app.config().workflow.default_status.clone();

    let mut reports = Vec::new();
    for (name, result) in built {
        let mut report = Report {
            name: name.clone(),
            ..Report::default()
        };
        match result {
            Err(e) => report.error = Some(e.to_string()),
            Ok(source) => {
                let tasks = store.list(&Filter::default().any_done())?;
                match collect(&source, &tasks, &ids, &default_status) {
                    Err(e) => report.error = Some(e),
                    Ok(changes) => {
                        report.changes = changes.iter().map(Change::describe).collect();
                        if !dry_run {
                            let (applied, error) =
                                source::apply(&mut store, &changes, clock.as_ref());
                            report.applied = applied;
                            if let Some(e) = error {
                                report.error = Some(format!("while applying: {e}"));
                            }
                        }
                    }
                }
            }
        }
        reports.push(report);
    }
    render(app, &reports, dry_run)
}

/// The changes one source asks for: a full sweep (fetch, then check the
/// tracked items the sweep no longer lists), or a check of `ids` only.
fn collect(
    source: &Built,
    tasks: &[Task],
    ids: &[TaskId],
    default_status: &Status,
) -> std::result::Result<Vec<Change>, String> {
    let name = source.name.as_str();
    let origin_of = |task: &Task| task.origin.as_ref().filter(|o| o.source == name).cloned();
    let known: Vec<Origin> = tasks.iter().filter_map(origin_of).collect();
    let (items, states) = if ids.is_empty() {
        let items = source
            .source
            .fetch(&SyncContext {
                known: known.clone(),
            })
            .map_err(|e| e.to_string())?;
        let missing: Vec<Origin> = tasks
            .iter()
            .filter(|t| !t.done)
            .filter_map(origin_of)
            .filter(|o| !items.iter().any(|i| i.external_id == o.external_id))
            .collect();
        let states = if missing.is_empty() {
            Vec::new()
        } else {
            source.source.check(&missing).map_err(|e| e.to_string())?
        };
        (items, states)
    } else {
        let selected: Vec<Origin> = tasks
            .iter()
            .filter(|t| ids.contains(&t.id))
            .filter_map(origin_of)
            .collect();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        (
            Vec::new(),
            source.source.check(&selected).map_err(|e| e.to_string())?,
        )
    };
    let defaults = Defaults {
        status: source
            .defaults
            .status
            .clone()
            .or_else(|| Some(default_status.clone())),
        tags: source.defaults.tags.clone(),
    };
    Ok(source::reconcile(
        name,
        tasks,
        &items,
        &states,
        &source.policy,
        &defaults,
    ))
}

fn render(app: &App, reports: &[Report], dry_run: bool) -> Result<()> {
    let failed = reports.iter().any(|r| r.error.is_some());
    if app.out.json_mode() {
        let sources: Vec<Value> = reports
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "changes": r.changes,
                    "applied": r.applied,
                    "error": r.error,
                })
            })
            .collect();
        app.out.json(&json::document([
            ("dry_run", Value::from(dry_run)),
            ("ok", Value::from(!failed)),
            ("sources", Value::Array(sources)),
        ]))?;
    } else {
        let mut text = String::new();
        for r in reports {
            let heading = match (&r.error, r.changes.len()) {
                (Some(e), _) => format!("{}: failed: {e}", r.name),
                (None, 0) => format!("{}: up to date", r.name),
                (None, n) if dry_run => format!("{}: {n} change(s) (dry run)", r.name),
                (None, n) => format!("{}: {n} change(s)", r.name),
            };
            text.push_str(&heading);
            text.push('\n');
            if dry_run || r.applied.is_empty() {
                for change in &r.changes {
                    text.push_str("  ");
                    text.push_str(change);
                    text.push('\n');
                }
            } else {
                for applied in &r.applied {
                    text.push_str("  ");
                    text.push_str(&applied.description);
                    text.push('\n');
                }
            }
        }
        app.out.print(&text)?;
    }
    if failed {
        Err(CliError::Silent(1))
    } else {
        Ok(())
    }
}
