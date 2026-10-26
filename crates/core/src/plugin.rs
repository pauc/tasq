//! Plugin events (ADR 0030): what tasq tells an enabled plugin, and the
//! document it sends on the plugin's stdin.
//!
//! For every [`Event`] the CLI runs `tasq-<name> hook` with a [`Payload`]
//! serialised as JSON. Payloads carry [`SCHEMA`]; within a version fields
//! are only ever added. Building them is pure; running the plugin is the
//! CLI's job.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

use crate::model::TaskId;

/// The payload schema version.
pub const SCHEMA: u32 = 1;

/// What a plugin is told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// A command (or the TUI, at start and on `r`) opened the store.
    StoreOpen,
    /// A command (or a TUI action) wrote tasks; sent once, after its last
    /// write, and only when it wrote something.
    PostWrite {
        /// The subcommand name (`ui` in the TUI).
        command: &'a str,
        /// Every task written, once each, in the order first written.
        writes: &'a [Write],
    },
}

impl Event<'_> {
    /// The `event` field of the payload and the value of `TASQ_EVENT`.
    pub fn name(self) -> &'static str {
        match self {
            Self::StoreOpen => "store-open",
            Self::PostWrite { .. } => "post-write",
        }
    }
}

/// A task a command wrote, as `post-write` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Write {
    /// The task's id.
    pub id: TaskId,
    /// The file holding it.
    pub file: PathBuf,
    /// Whether the command created the task or changed it.
    pub kind: WriteKind,
}

/// How a task was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteKind {
    /// A new task file.
    Create,
    /// An existing task file rewritten.
    Update,
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
/// `post-write` adds `"command"` and `"writes": [{"id", "file", "kind"}]`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Payload<'a> {
    schema: u32,
    event: &'static str,
    store: StoreRef<'a>,
    config: Table<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    writes: Option<&'a [Write]>,
}

impl<'a> Payload<'a> {
    /// The payload of `event` for `store`, with the plugin's `config`.
    pub fn new(event: Event<'a>, store: StoreRef<'a>, config: Option<&'a toml::Table>) -> Self {
        let (command, writes) = match event {
            Event::StoreOpen => (None, None),
            Event::PostWrite { command, writes } => (Some(command), Some(writes)),
        };
        Self {
            schema: SCHEMA,
            event: event.name(),
            store,
            config: Table(config),
            command,
            writes,
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
