//! `tasq dates [SPEC]...`: resolve a date or range spec to `FROM TO`
//! (plan T-602), for scripts and out-of-process plugins.

use serde_json::Value;
use tasq_core::dates::resolve_range;

use crate::app::App;
use crate::error::Result;
use crate::json;

/// Runs `dates`. The words of `spec` are joined with spaces, so
/// `tasq dates last week` and `tasq dates "last week"` are the same.
pub fn run(app: &App, spec: &[String]) -> Result<()> {
    let spec = spec.join(" ");
    let today = app.clock()?.today();
    let range = resolve_range(&spec, today)?;
    if app.out.json_mode() {
        return app.out.json(&json::document([
            ("spec", Value::from(spec.trim())),
            ("from", json::to_value(&range.from)),
            ("to", json::to_value(&range.to)),
            ("days", json::to_value(&range.days())),
            ("working_days", json::to_value(&range.working_days())),
        ]));
    }
    app.out.print(&format!("{range}\n"))
}
