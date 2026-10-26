//! Plugin events (ADR 0030): what tasq tells an enabled plugin, and the
//! document it sends on the plugin's stdin.
//!
//! For every [`Event`] the CLI runs `tasq-<name> hook` with a [`Payload`]
//! serialised as JSON. Payloads carry [`SCHEMA`]; within a version fields
//! are only ever added. Building them is pure; running the plugin is the
//! CLI's job.

use std::path::Path;

use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

/// The payload schema version.
pub const SCHEMA: u32 = 1;

/// What a plugin is told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A command (or the TUI, at start) opened the store.
    StoreOpen,
}

impl Event {
    /// The `event` field of the payload and the value of `TASQ_EVENT`.
    pub fn name(self) -> &'static str {
        match self {
            Self::StoreOpen => "store-open",
        }
    }
}

/// The store an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StoreRef<'a> {
    /// `store.name`.
    pub name: &'a str,
    /// The store directory, `<data_dir>/<store.name>`.
    pub location: &'a Path,
}

/// `{"schema": 1, "event": <name>, "store": {"name", "location"},
/// "config": {...}}`, `config` being the plugin's `[plugin.<name>]` table
/// (`{}` when there is none). TOML datetimes become their TOML text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Payload<'a> {
    schema: u32,
    event: &'static str,
    store: StoreRef<'a>,
    config: Table<'a>,
}

impl<'a> Payload<'a> {
    /// The payload of `event` for `store`, with the plugin's `config`.
    pub fn new(event: Event, store: StoreRef<'a>, config: Option<&'a toml::Table>) -> Self {
        Self {
            schema: SCHEMA,
            event: event.name(),
            store,
            config: Table(config),
        }
    }
}

/// A TOML table serialised as plain data (no datetime wrapper object).
#[derive(Debug, Clone, PartialEq)]
struct Table<'a>(Option<&'a toml::Table>);

impl Serialize for Table<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(self.0.map(toml::Table::len))?;
        for (key, value) in self.0.into_iter().flatten() {
            map.serialize_entry(key, &Value(value))?;
        }
        map.end()
    }
}

/// A TOML value serialised as plain data.
struct Value<'a>(&'a toml::Value);

impl Serialize for Value<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            toml::Value::String(s) => serializer.serialize_str(s),
            toml::Value::Integer(i) => serializer.serialize_i64(*i),
            toml::Value::Float(f) => serializer.serialize_f64(*f),
            toml::Value::Boolean(b) => serializer.serialize_bool(*b),
            toml::Value::Datetime(d) => serializer.collect_str(d),
            toml::Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&Value(item))?;
                }
                seq.end()
            }
            toml::Value::Table(table) => Table(Some(table)).serialize(serializer),
        }
    }
}
