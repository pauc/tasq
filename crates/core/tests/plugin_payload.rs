//! Plugin event payloads (ADR 0030): the exact JSON a plugin reads.

#![allow(clippy::unwrap_used, reason = "tests panic on failure")]

use std::path::{Path, PathBuf};

use tasq_core::model::TaskId;
use tasq_core::plugin::{Event, Payload, SCHEMA, StoreRef, Write, WriteKind};

fn store() -> StoreRef<'static> {
    StoreRef {
        name: "work",
        location: Path::new("/data/stores/work"),
    }
}

#[test]
fn store_open_carries_the_schema_the_store_and_the_plugin_config() {
    assert_eq!(SCHEMA, 1);
    assert_eq!(Event::StoreOpen.name(), "store-open");
    let config: toml::Table = toml::from_str(
        "interval = 60\nratio = 0.5\non = true\nsince = 2026-10-26\nremote = \"origin\"\n\
         list = [1, \"two\"]\n[deep]\nkey = \"v\"\n",
    )
    .unwrap();
    let json =
        serde_json::to_string(&Payload::new(Event::StoreOpen, store(), Some(&config))).unwrap();
    assert_eq!(
        json,
        "{\"schema\":1,\"event\":\"store-open\",\
         \"store\":{\"name\":\"work\",\"location\":\"/data/stores/work\"},\
         \"config\":{\"deep\":{\"key\":\"v\"},\"interval\":60,\"list\":[1,\"two\"],\
         \"on\":true,\"ratio\":0.5,\"remote\":\"origin\",\"since\":\"2026-10-26\"}}"
    );
}

#[test]
fn a_plugin_without_a_table_gets_an_empty_config() {
    let json = serde_json::to_string(&Payload::new(Event::StoreOpen, store(), None)).unwrap();
    assert!(json.ends_with(",\"config\":{}}"), "{json}");
}

#[test]
fn post_write_adds_the_command_and_every_write() {
    let writes = [
        Write {
            id: TaskId::from(8),
            file: PathBuf::from("/data/stores/work/01K8.md"),
            kind: WriteKind::Create,
        },
        Write {
            id: TaskId::from(3),
            file: PathBuf::from("/data/stores/work/01J2.md"),
            kind: WriteKind::Update,
        },
    ];
    let event = Event::PostWrite {
        command: "create",
        writes: &writes,
    };
    assert_eq!(event.name(), "post-write");
    let json = serde_json::to_string(&Payload::new(event, store(), None)).unwrap();
    assert_eq!(
        json,
        "{\"schema\":1,\"event\":\"post-write\",\
         \"store\":{\"name\":\"work\",\"location\":\"/data/stores/work\"},\
         \"config\":{},\"command\":\"create\",\"writes\":[\
         {\"id\":\"8\",\"file\":\"/data/stores/work/01K8.md\",\"kind\":\"create\"},\
         {\"id\":\"3\",\"file\":\"/data/stores/work/01J2.md\",\"kind\":\"update\"}]}"
    );
}
