//! Plugin event payloads (ADR 0030): the exact JSON a plugin reads.

#![allow(clippy::unwrap_used, reason = "tests panic on failure")]

use std::path::Path;

use tasq_core::plugin::{Event, Payload, SCHEMA, StoreRef};

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
