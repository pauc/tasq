//! `tasq-git hook` driven the way tasq drives it (docs/plugins.md): the
//! event's payload on stdin, then end of file, asserting on stdout, stderr,
//! the exit code and the store directory.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

/// A store directory under a temp root, absent until a test creates it.
struct Store {
    _root: TempDir,
    dir: PathBuf,
}

impl Store {
    /// A plain store: the directory with one task file, no `.git`.
    fn plain() -> Self {
        let store = Self::absent();
        std::fs::create_dir_all(&store.dir).unwrap();
        std::fs::write(
            store.dir.join("01K6Q8ZT3D0000000000000003.md"),
            "---\nid: 3\n---\n# Three\n",
        )
        .unwrap();
        store
    }

    /// No directory at all: an empty store, created by its first write.
    fn absent() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("stores").join("default");
        Self { _root: root, dir }
    }

    /// The payload tasq sends for `event`, schema 1.
    fn payload(&self, event: &str) -> String {
        let mut doc = serde_json::json!({
            "schema": 1,
            "event": event,
            "store": {"name": "default", "location": self.dir},
            "config": {"interval": 60},
        });
        if event == "post-write" {
            doc["command"] = "log".into();
            doc["writes"] = serde_json::json!([{
                "id": "3",
                "file": self.dir.join("01K6Q8ZT3D0000000000000003.md"),
                "kind": "update",
            }]);
        }
        doc.to_string()
    }

    /// Every path under the store directory, relative and sorted; empty
    /// when it does not exist.
    fn tree(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                out.push(path.strip_prefix(base).unwrap().to_owned());
                walk(&path, base, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.dir, &self.dir, &mut out);
        out.sort();
        out
    }
}

/// Runs `tasq-git <args>` with `stdin` and an environment holding nothing.
fn tasq_git(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tasq-git"))
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn hook(payload: &str) -> Output {
    tasq_git(&["hook"], payload)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// Exit 0, nothing printed.
fn assert_silent(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{}", stderr(out));
    assert_eq!(stdout(out), "");
    assert_eq!(stderr(out), "");
}

mod plain_store {
    use super::*;

    #[test]
    fn store_open_and_post_write_do_nothing() {
        let store = Store::plain();
        let before = store.tree();
        for event in ["store-open", "post-write"] {
            assert_silent(&hook(&store.payload(event)));
            assert_eq!(store.tree(), before, "{event} touched the store");
        }
    }

    #[test]
    fn an_absent_store_is_left_absent() {
        let store = Store::absent();
        for event in ["store-open", "post-write", "doctor"] {
            let out = hook(&store.payload(event));
            assert_eq!(out.status.code(), Some(0), "{event}: {}", stderr(&out));
            assert!(!store.dir.exists(), "{event} created the store");
        }
    }

    #[test]
    fn doctor_answers_one_ok_check() {
        let store = Store::plain();
        let out = hook(&store.payload("doctor"));
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        let expected = format!(
            r#"[{{"name":"sync","status":"ok","message":"store sync off: {} is not a git repository"}}]"#,
            store.dir.display()
        );
        assert_eq!(stdout(&out), format!("{expected}\n"));
    }

    #[test]
    fn doctor_on_an_absent_store_answers_the_same_check() {
        let store = Store::absent();
        let out = hook(&store.payload("doctor"));
        let checks: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(checks[0]["status"], "ok");
        assert_eq!(checks.as_array().unwrap().len(), 1);
    }
}

mod repository {
    use super::*;

    /// A store with a `.git` is not answered as "sync off". The checks of a
    /// synced store are GS-007's; until then the answer is no checks.
    #[test]
    fn doctor_does_not_say_sync_is_off() {
        let store = Store::plain();
        std::fs::create_dir(store.dir.join(".git")).unwrap();
        assert_silent(&hook(&store.payload("doctor")));
    }

    #[test]
    fn a_git_file_counts_too() {
        let store = Store::plain();
        std::fs::write(store.dir.join(".git"), "gitdir: /elsewhere\n").unwrap();
        assert_silent(&hook(&store.payload("doctor")));
    }
}

mod payload {
    use super::*;

    #[test]
    fn an_unknown_schema_is_an_error_naming_it() {
        let store = Store::plain();
        for event in ["store-open", "post-write", "doctor"] {
            let payload = store
                .payload(event)
                .replacen("\"schema\":1", "\"schema\":2", 1);
            let out = hook(&payload);
            assert_eq!(out.status.code(), Some(1), "{event}");
            assert_eq!(stdout(&out), "", "{event}");
            assert_eq!(
                stderr(&out),
                "unsupported event payload schema 2 (tasq-git reads schema 1)\n",
                "{event}"
            );
        }
    }

    #[test]
    fn a_payload_without_a_schema_is_an_error() {
        let out = hook(r#"{"event":"doctor","store":{"name":"default","location":"/x"}}"#);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stderr(&out), "the event payload has no schema\n");
    }

    #[test]
    fn text_is_an_error() {
        let out = hook("hello");
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("the event payload is not JSON: "),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn a_schema_1_payload_without_a_store_is_an_error() {
        let out = hook(r#"{"schema":1,"event":"doctor","config":{}}"#);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out)
                .starts_with("the schema 1 event payload is malformed: missing field `store`"),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn an_unknown_event_is_ignored() {
        let store = Store::plain();
        assert_silent(&hook(&store.payload("pre-frobnicate")));
    }
}

mod usage {
    use super::*;

    #[test]
    fn no_subcommand_is_a_usage_error() {
        let out = tasq_git(&[], "");
        assert_eq!(out.status.code(), Some(2));
    }

    #[test]
    fn version() {
        let out = tasq_git(&["--version"], "");
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            stdout(&out),
            format!("tasq-git {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}
