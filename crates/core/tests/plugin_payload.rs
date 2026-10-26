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

#[test]
fn doctor_carries_no_more_than_store_open() {
    assert_eq!(Event::Doctor.name(), "doctor");
    let json = serde_json::to_string(&Payload::new(Event::Doctor, store(), None)).unwrap();
    assert_eq!(
        json,
        "{\"schema\":1,\"event\":\"doctor\",\
         \"store\":{\"name\":\"work\",\"location\":\"/data/stores/work\"},\"config\":{}}"
    );
}

mod doctor_checks {
    use tasq_core::doctor::Check;
    use tasq_core::plugin::doctor_checks;

    #[test]
    fn each_check_is_named_after_the_plugin() {
        let answer = r#"[
            {"name": "remote", "status": "ok", "message": "origin reachable"},
            {"name": "push", "status": "warn", "message": "3 commits not pushed", "fix": "tasq git push"},
            {"name": "merge", "status": "fail", "message": "conflict in 01K8.md", "fix": "resolve it", "extra": 1}
        ]"#;
        assert_eq!(
            doctor_checks("git", answer),
            vec![
                Check::ok("git/remote", "origin reachable"),
                Check::warn("git/push", "3 commits not pushed", "tasq git push"),
                Check::fail("git/merge", "conflict in 01K8.md", "resolve it"),
            ]
        );
    }

    #[test]
    fn a_check_without_a_fix_has_none() {
        let checks = doctor_checks(
            "git",
            r#"[{"name": "a", "status": "warn", "message": "m"}]"#,
        );
        assert_eq!(checks[0].fix, None);
    }

    #[test]
    fn no_answer_is_no_checks() {
        assert_eq!(doctor_checks("git", ""), Vec::new());
        assert_eq!(doctor_checks("git", " \n\t"), Vec::new());
        assert_eq!(doctor_checks("git", "[]"), Vec::new());
    }

    #[test]
    fn an_answer_that_does_not_parse_is_one_failure() {
        for (answer, error) in [
            ("not json", "expected ident at line 1 column 2"),
            (
                r#"[{"name": "a", "status": "bad", "message": "m"}]"#,
                "unknown variant `bad`, expected one of `ok`, `warn`, `fail` at line 1 column 30",
            ),
            (
                r#"{"name": "a"}"#,
                "invalid type: map, expected a sequence at line 1 column 0",
            ),
        ] {
            assert_eq!(
                doctor_checks("git", answer),
                vec![Check::fail(
                    "git/doctor",
                    format!("the doctor answer is not a list of checks: {error}"),
                    "tasq-git must print a JSON array of {name, status, message, fix} on doctor",
                )],
                "{answer}"
            );
        }
    }
}

mod lasting_warnings {
    use std::path::{Path, PathBuf};

    use tasq_core::plugin::{warnings, warnings_file};

    #[test]
    fn live_under_the_store_in_the_plugin_directory() {
        assert_eq!(
            warnings_file(Path::new("/data/stores/work"), "git"),
            PathBuf::from("/data/stores/work/.tasq/plugins/git/warnings")
        );
    }

    #[test]
    fn are_the_non_empty_lines_trimmed() {
        assert_eq!(
            warnings("push failed: no network\r\n\n   \n  rebase stopped  \nlast"),
            vec!["push failed: no network", "rebase stopped", "last"]
        );
        assert_eq!(warnings(""), Vec::<&str>::new());
    }
}
