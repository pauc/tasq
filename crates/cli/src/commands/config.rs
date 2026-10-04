//! `tasq config show`: the effective configuration with provenance.
//!
//! The config is serialised to a TOML table and printed key by key, each
//! leaf annotated with the layer that set it (`Loaded::explain`), so the
//! output answers "where does this value come from?" at a glance while
//! staying valid TOML.

use std::fmt::Write as _;

use serde_json::Value;
use tasq_core::config::Loaded;
use toml::Table;

use crate::app::App;
use crate::cli::ConfigCommand;
use crate::error::{CliError, Result};
use crate::json;

/// Runs a `config` subcommand.
pub fn run(app: &App, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show => {
            if app.out.json_mode() {
                let layers: Vec<Value> = app
                    .loaded
                    .layers
                    .iter()
                    .map(|l| {
                        serde_json::json!({
                            "origin": l.origin.to_string(),
                            "keys": l.keys,
                        })
                    })
                    .collect();
                return app.out.json(&json::document([
                    ("config", json::to_value(&app.loaded.config)),
                    ("profile", json::to_value(&app.loaded.profile)),
                    ("profiles", json::to_value(&app.loaded.profiles)),
                    ("layers", Value::Array(layers)),
                ]));
            }
            app.out.page(&render(&app.loaded)?)
        }
    }
}

/// Column where the `# origin` comment starts.
const COMMENT_COLUMN: usize = 44;

/// The annotated TOML.
pub fn render(loaded: &Loaded) -> Result<String> {
    let table = toml::Value::try_from(&loaded.config)
        .map_err(|e| CliError::Internal(anyhow::anyhow!("serialising config: {e}")))?;
    let Some(table) = table.as_table() else {
        return Err(CliError::Internal(anyhow::anyhow!(
            "config did not serialise to a table"
        )));
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# profile: {}",
        loaded.profile.as_deref().unwrap_or("none")
    );
    if !loaded.profiles.is_empty() {
        let _ = writeln!(out, "# profiles available: {}", loaded.profiles.join(", "));
    }
    let _ = writeln!(out, "# layers, later ones win:");
    for (i, layer) in loaded.layers.iter().enumerate() {
        let _ = writeln!(out, "#   {}. {}", i + 1, layer.origin);
    }
    write_table(table, "", loaded, &mut out);
    Ok(out)
}

fn is_array_of_tables(value: &toml::Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(toml::Value::is_table))
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

/// Top-level tables in the order of the `Config` struct (the order the
/// documentation uses); anything else, and nested keys, alphabetical.
const TOP_LEVEL_ORDER: &[&str] = &[
    "store", "workflow", "work", "launch", "ui", "forge", "source", "report",
];

fn ordered_keys<'a>(table: &'a Table, prefix: &str) -> Vec<&'a String> {
    let mut keys: Vec<&String> = table.keys().collect();
    if prefix.is_empty() {
        keys.sort_by_key(|k| {
            TOP_LEVEL_ORDER
                .iter()
                .position(|known| known == k)
                .unwrap_or(TOP_LEVEL_ORDER.len())
        });
    }
    keys
}

fn write_table(table: &Table, prefix: &str, loaded: &Loaded, out: &mut String) {
    let keys = ordered_keys(table, prefix);
    for key in &keys {
        let value = &table[*key];
        if value.is_table() || is_array_of_tables(value) {
            continue;
        }
        let path = join(prefix, key);
        let origin = loaded
            .explain(&path)
            .map_or_else(|| "unset".to_owned(), ToString::to_string);
        let line = format!("{key} = {}", format_value(value));
        let _ = writeln!(out, "{line:<COMMENT_COLUMN$} # {origin}");
    }
    for key in &keys {
        let value = &table[*key];
        let path = join(prefix, key);
        if let Some(sub) = value.as_table() {
            // A table with nothing but subtables ([report]) needs no header
            // of its own; an empty one ([ui.colors]) keeps it to show the key.
            let only_subtables = !sub.is_empty() && sub.values().all(toml::Value::is_table);
            if !only_subtables {
                let _ = writeln!(out, "\n[{path}]");
            }
            write_table(sub, &path, loaded, out);
        } else if is_array_of_tables(value) {
            for item in value.as_array().into_iter().flatten() {
                let _ = writeln!(out, "\n[[{path}]]");
                if let Some(sub) = item.as_table() {
                    write_table(sub, &path, loaded, out);
                }
            }
        }
    }
}

/// A TOML value in inline form.
pub fn format_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => quote(s),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(format_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        toml::Value::Table(table) => format!(
            "{{ {} }}",
            table
                .iter()
                .map(|(k, v)| format!("{k} = {}", format_value(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A TOML basic string.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use tasq_core::config::{Config, Layer, Origin};

    use super::*;

    #[test]
    fn values() {
        assert_eq!(
            format_value(&toml::Value::String("a\"b\\c\n".into())),
            "\"a\\\"b\\\\c\\n\""
        );
        assert_eq!(format_value(&toml::Value::Integer(3)), "3");
        assert_eq!(format_value(&toml::Value::Boolean(true)), "true");
        assert_eq!(
            format_value(&toml::Value::Array(vec![
                toml::Value::String("a".into()),
                toml::Value::String("b".into())
            ])),
            "[\"a\", \"b\"]"
        );
        let mut t = Table::new();
        t.insert("x".into(), toml::Value::Integer(1));
        assert_eq!(format_value(&toml::Value::Table(t)), "{ x = 1 }");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn annotated_defaults() {
        let loaded = Loaded {
            config: Config::default(),
            layers: vec![Layer {
                origin: Origin::Defaults,
                keys: vec!["store.notebook".into(), "ui.pager".into()],
            }],
            profile: None,
            profiles: vec!["work".into()],
        };
        let text = render(&loaded).unwrap();
        assert!(text.starts_with("# profile: none\n# profiles available: work\n# layers, later ones win:\n#   1. defaults\n\n[store]\n"), "{text}");
        assert!(!text.contains("\n[report]\n"), "{text}");
        assert!(text.contains("\n[report.summary]\n"), "{text}");
        assert!(text.contains("\n[ui.colors]\n"), "{text}");
        assert!(text.contains("\n[store]\n"), "{text}");
        assert!(
            text.contains(&format!(
                "{:<COMMENT_COLUMN$} # defaults\n",
                "notebook = \"home\""
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!("{:<COMMENT_COLUMN$} # unset\n", "kind = \"nb\"")),
            "{text}"
        );
        assert!(
            text.contains(
                "statuses = [\"in-progress\", \"ready\", \"waiting\", \"blocked\", \"later\"]"
            ),
            "{text}"
        );
        // The output is itself valid TOML that reads back as the same config.
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, Config::default());
    }
}
