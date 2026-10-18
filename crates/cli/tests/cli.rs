//! Integration tests: the `tasq` binary against a temporary fixture
//! notebook. Output is snapshotted with `insta` (`cargo insta review`, or
//! `INSTA_UPDATE=always cargo test -p tasq` to accept everything).

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::path::Path;

use insta::assert_snapshot;
use predicates::prelude::*;
use support::{TestEnv, stderr, stdout};

mod help {
    use super::*;

    #[test]
    fn top_level() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("--help").output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn list_details() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["help", "list"]).output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn version() {
        let env = TestEnv::fixture();
        env.tasq()
            .arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::starts_with("tasq 0.1.0"));
    }
}

mod errors {
    use super::*;

    #[test]
    fn unknown_notebook_is_a_user_error() {
        let env = TestEnv::fixture();
        let out = env.tasq().env("TASQ_NOTEBOOK", "nope").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        let err = stderr(&out);
        assert!(err.starts_with("tasq: store.notebook"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn broken_config_names_file_and_line() {
        let env = TestEnv::fixture();
        let file = env.write_global_config("[store]\nnotbook = \"home\"\n");
        let out = env.tasq().output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let err = stderr(&out);
        assert!(
            err.starts_with(&format!(
                "tasq: {}:2:1: unknown field `notbook`",
                file.display()
            )),
            "{err}"
        );
    }

    #[test]
    fn unknown_profile() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["--profile", "nope"])
            .assert()
            .code(1)
            .stderr("tasq: profile \"nope\" is not defined; available profiles: \n");
    }

    #[test]
    fn usage_errors_exit_2() {
        let env = TestEnv::fixture();
        env.tasq()
            .arg("--bogus")
            .assert()
            .code(2)
            .stderr(predicate::str::contains("unexpected argument '--bogus'"));
        env.tasq()
            .args(["--set", "nonsense"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("expected KEY=VALUE"));
    }

    #[test]
    fn invalid_filter_word() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("##x").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).starts_with("tasq: "), "{}", stderr(&out));
    }

    #[test]
    fn unknown_set_key() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["--set", "store.colour=red"])
            .assert()
            .code(1)
            .stderr("tasq: --set: unknown config key \"store.colour\"\n");
    }
}

mod list {
    use super::*;

    #[test]
    fn grouped_plain() {
        let env = TestEnv::fixture();
        let out = env.tasq().output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn grouped_colored() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["--color", "always"]).output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn no_color_env_is_respected_but_always_wins() {
        let env = TestEnv::fixture();
        let plain = env.tasq().env("NO_COLOR", "1").output().unwrap();
        assert!(!stdout(&plain).contains('\x1b'));
        let forced = env
            .tasq()
            .env("NO_COLOR", "1")
            .args(["--color", "always"])
            .output()
            .unwrap();
        assert!(stdout(&forced).contains('\x1b'));
        let never = env
            .tasq()
            .args(["--color", "always", "--no-color"])
            .output()
            .unwrap();
        assert!(!stdout(&never).contains('\x1b'));
    }

    #[test]
    fn one_status() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("ready").output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
        let explicit = env
            .tasq()
            .args(["list", "--status", "ready"])
            .output()
            .unwrap();
        assert_eq!(stdout(&explicit), stdout(&out));
    }

    /// A hand-written `## Rank` puts a `#C` task above the `#A` one of its
    /// status group, in text and JSON alike; the rank itself is not shown.
    #[test]
    fn ranked_tasks_come_first_in_their_group() {
        let env = TestEnv::fixture();
        env.write_task(
            "20260907150000.todo.md",
            "# [ ] Ranked\n\n## Rank\n\n1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-07 15:00: created\n",
        );
        env.write_task(
            "20260908160000.todo.md",
            "# [ ] Ranked first\n\n## Rank\n\n-1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-08 16:00: created\n",
        );
        env.write_task(
            "20260909170000.todo.md",
            "# [ ] Rank by hand\n\n## Rank\n\nsoon\n\n## Tags\n\n#A #in-progress\n\n## Progress\n\n- 2026-10-09 17:00: created\n",
        );
        for name in [
            "20260907150000.todo.md",
            "20260908160000.todo.md",
            "20260909170000.todo.md",
        ] {
            env.register(name);
        }
        let out = env.tasq().arg("in-progress").output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "in-progress\n  \
             [ 9] #C Ranked first\n  \
             [ 8] #C Ranked\n  \
             [ 1] #A Rewrite the tasks script in Rust (due in 4d)  #gitlab \n  \
             [10] #A Rank by hand\n\n"
        );
        assert!(!stdout(&out).contains("1024"), "{}", stdout(&out));
        let json = env.tasq().args(["in-progress", "--json"]).output().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        let ids: Vec<&str> = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["9", "8", "1", "10"]);
        assert_eq!(value["tasks"][0]["rank"], -1024);
        assert_eq!(value["tasks"][2].get("rank"), None);
        assert_eq!(value["tasks"][3].get("rank"), None);
    }

    #[test]
    fn by_tag() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("gitlab").output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
        let hashed = env.tasq().arg("#gitlab").output().unwrap();
        assert_eq!(stdout(&hashed), stdout(&out));
        let explicit = env
            .tasq()
            .args(["list", "--tag", "gitlab"])
            .output()
            .unwrap();
        assert_eq!(stdout(&explicit), stdout(&out));
    }

    #[test]
    fn by_priority() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("A").output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
        let explicit = env.tasq().args(["list", "--prio", "A"]).output().unwrap();
        assert_eq!(stdout(&explicit), stdout(&out));
    }

    #[test]
    fn combined_flags_and_text() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .args([
                "list",
                "--tag",
                "gitlab",
                "--status",
                "in-progress",
                "--text",
                "RUST",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn nothing_matches() {
        let env = TestEnv::fixture();
        env.tasq()
            .arg("nosuch")
            .assert()
            .success()
            .stdout("No open todos tagged #nosuch.\n");
        env.tasq()
            .arg("blocked")
            .assert()
            .success()
            .stdout("No open todos with status blocked.\n");
        env.tasq()
            .args(["list", "--prio", "C"])
            .assert()
            .success()
            .stdout("No open todos with priority #C.\n");
    }

    #[test]
    fn all_adds_a_done_group_after_the_open_ones() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["list", "--all"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
        // Filters still apply inside the done group.
        env.tasq()
            .args(["list", "--all", "--tag", "nosuch"])
            .assert()
            .success()
            .stdout("No todos tagged #nosuch.\n");
        let out = env
            .tasq()
            .args(["--json", "list", "--all"])
            .output()
            .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        let tasks = doc["tasks"].as_array().unwrap();
        let done: Vec<&serde_json::Value> = tasks.iter().filter(|t| t["done"] == true).collect();
        assert_eq!(done.len(), 1, "{tasks:?}");
        assert_eq!(tasks.last().unwrap()["done"], true, "done tasks come last");
    }

    /// A closed task keeps its `## Rank` for when it is reopened, but the
    /// done list does not follow it.
    #[test]
    fn done_list_ignores_a_kept_rank() {
        let env = TestEnv::fixture();
        env.write_task(
            "20260907150000.todo.md",
            "# [x] Closed with a rank\n\n## Rank\n\n-1024\n\n## Tags\n\n#C\n\n## Progress\n\n- 2026-10-07 15:00: created\n",
        );
        env.register("20260907150000.todo.md");
        let out = env.tasq().args(["list", "--done"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "DONE\n  \
             [ 4] #C Ship the release notes  #gitlab \n  \
             [ 8] #C Closed with a rank\n\n"
        );
        let json = env
            .tasq()
            .args(["list", "--done", "--json"])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["tasks"][1]["rank"], -1024);
    }

    #[test]
    fn done_lists_only_closed_tasks() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["list", "--done"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "DONE\n  [ 4] #C Ship the release notes  #gitlab \n\n"
        );
        env.tasq()
            .args(["list", "--done", "--prio", "A"])
            .assert()
            .success()
            .stdout("No done todos with priority #A.\n");
        env.tasq()
            .args(["list", "--all", "--done"])
            .assert()
            .code(2);
        let env = TestEnv::empty();
        env.tasq()
            .args(["list", "--all"])
            .assert()
            .success()
            .stdout("No todos.\n");
        env.tasq()
            .args(["list", "--done"])
            .assert()
            .success()
            .stdout("No done todos.\n");
    }

    #[test]
    fn empty_notebook() {
        let env = TestEnv::empty();
        env.tasq().assert().success().stdout("No open todos.\n");
        env.tasq()
            .arg("gitlab")
            .assert()
            .success()
            .stdout("No open todos.\n");
    }

    #[test]
    fn unknown_status_flag() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["list", "--status", "nope"])
            .assert()
            .code(1)
            .stderr(
                "tasq: unknown status 'nope' (statuses: in-progress ready waiting blocked later)\n",
            );
    }

    #[test]
    fn json() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("--json").output().unwrap();
        assert!(out.status.success());
        let text = stdout(&out);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["tasks"].as_array().unwrap().len(), 4);
        assert_snapshot!(text);
    }

    #[test]
    fn json_for_an_empty_result_is_an_empty_array() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["--json", "nosuch"])
            .assert()
            .success()
            .stdout("{\n  \"schema\": 1,\n  \"tasks\": []\n}\n");
    }

    #[test]
    fn custom_workflow_status_and_color() {
        let env = TestEnv::fixture();
        env.write_project_config(
            "[workflow]\nstatuses = [\"in-progress\", \"review\", \"ready\"]\ndefault_status = \"ready\"\n\n[ui.colors]\nreview = \"208\"\n",
        );
        env.write_task(
            "20260907150000.todo.md",
            "# [ ] Needs a review\n\n## Tags\n\n#review\n",
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(env.notebook().join(".index"))
            .unwrap();
        let index = std::fs::read_to_string(env.notebook().join(".index")).unwrap();
        std::fs::write(
            env.notebook().join(".index"),
            format!("{index}20260907150000.todo.md\n"),
        )
        .unwrap();
        let out = env.tasq().args(["--color", "always"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
    }
}

mod store {
    use super::*;

    #[test]
    fn info() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["store", "info"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    #[test]
    fn info_json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .args(["store", "info", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    /// `tasq store sync` and `tasq store reconcile` went with git sync
    /// (ADR-0026): clap rejects them like any unknown subcommand.
    #[test]
    fn sync_and_reconcile_are_unknown_subcommands() {
        let env = TestEnv::fixture();
        for gone in ["sync", "reconcile"] {
            let out = env.tasq().args(["store", gone]).output().unwrap();
            assert_eq!(out.status.code(), Some(2), "{gone}: {}", stderr(&out));
            assert!(
                stderr(&out).contains(&format!("unrecognized subcommand '{gone}'")),
                "{gone}: {}",
                stderr(&out)
            );
        }
    }

    #[test]
    fn verbose_prints_the_store_location() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["-v", "store", "info"]).output().unwrap();
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: store: nb at [ROOT]/nb/home\n"
        );
    }
}

mod doctor {
    use super::*;

    #[test]
    fn fails_on_an_inconsistent_index() {
        // The fixture index names a file that does not exist.
        let env = TestEnv::fixture();
        let out = env.tasq().arg("doctor").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    #[test]
    fn passes_once_the_index_is_consistent() {
        let env = TestEnv::fixture();
        env.write_task("20260905130000.todo.md", "# [ ] Restored\n");
        let out = env.tasq().arg("doctor").output().unwrap();
        assert!(out.status.success(), "{}", stdout(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    #[test]
    fn json() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["doctor", "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["ok"], false);
        let checks = value["checks"].as_array().unwrap();
        assert_eq!(checks[0]["name"], "config");
        assert_eq!(checks[0]["status"], "ok");
        assert!(
            checks
                .iter()
                .any(|c| c["name"] == "index" && c["status"] == "fail")
        );
    }

    #[test]
    fn reports_a_broken_config_instead_of_dying() {
        let env = TestEnv::fixture();
        env.write_global_config("[store]\nnotebook = 3\n");
        let out = env.tasq().arg("doctor").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stderr(&out), "");
        let text = env.normalize(&stdout(&out));
        assert!(text.starts_with("FAIL  config"), "{text}");
        assert!(
            !text.contains("notebook at"),
            "store checks must be skipped: {text}"
        );
        assert!(text.contains("WARN  glow"), "{text}");
    }

    #[test]
    fn reports_a_missing_notebook() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOTEBOOK", "nope")
            .arg("doctor")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let text = stdout(&out);
        assert!(text.contains("FAIL  store"), "{text}");
    }
}

mod config {
    use super::*;

    #[test]
    fn show_defaults() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["config", "show"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    #[test]
    fn show_with_layers() {
        let env = TestEnv::fixture();
        env.write_global_config(
            "[ui]\npager = \"cat\"\n\n[profile.work]\n[profile.work.store]\nnotebook = \"work\"\n",
        );
        env.write_project_config("[work]\ndefault_project = \"~/code/x\"\n");
        let out = env
            .tasq()
            .env("TASQ_GLOW_STYLE", "light")
            .args(["config", "show", "--set", "ui.no_osc8=true"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));
    }

    /// `store.sync_interval` went with git sync (ADR-0026); a config that
    /// still sets it fails naming the file, line and column.
    #[test]
    fn sync_interval_is_an_unknown_key() {
        let env = TestEnv::fixture();
        let file = env.write_global_config("[store]\nnotebook = \"home\"\nsync_interval = 60\n");
        let out = env.tasq().args(["config", "show"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with(&format!(
                "tasq: {}:3:1: unknown field `sync_interval`",
                file.display()
            )),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn show_json_with_profile() {
        let env = TestEnv::fixture();
        env.write_global_config("[profile.work]\n[profile.work.store]\nnotebook = \"work\"\n");
        let out = env
            .tasq()
            .args(["--profile", "work", "config", "show", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["profile"], "work");
        assert_eq!(value["config"]["store"]["notebook"], "work");
        assert_eq!(value["layers"][0]["origin"], "defaults");
        assert_eq!(value["layers"].as_array().unwrap().len(), 3);
    }
}

mod completions {
    use super::*;

    #[test]
    fn bash() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["completions", "bash"])
            .assert()
            .success()
            .stdout(predicate::str::contains("_tasq"));
    }

    #[test]
    fn needs_no_config_or_notebook() {
        let env = TestEnv::empty();
        env.write_global_config("not toml at all [[[");
        env.tasq()
            .env("TASQ_NOTEBOOK", "nope")
            .args(["completions", "zsh"])
            .assert()
            .success()
            .stdout(predicate::str::contains("#compdef tasq"));
    }
}

#[test]
fn a_word_and_a_subcommand_together_are_rejected() {
    let env = TestEnv::fixture();
    env.tasq()
        .args(["ready", "store", "info"])
        .assert()
        .code(1)
        .stderr("tasq: unexpected argument \"ready\" before the 'store' command\n");
}

/// A fixed "now" for every write in these tests.
const NOW: &str = "2026-10-07 09:30";

mod edit {
    use super::*;

    fn support_file(env: &TestEnv) -> String {
        std::fs::read_to_string(env.notebook().join("20260902100000.todo.md")).unwrap()
    }

    #[test]
    fn set_priority_with_note() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["set", "3", "A", "bumped"])
            .assert()
            .success()
            .stdout("[3] -> priority #A (bumped)\n");
        assert_snapshot!(support_file(&env));
    }

    #[test]
    fn set_status() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["set", "3", "#blocked"])
            .assert()
            .success()
            .stdout("[3] -> blocked\n");
        assert_snapshot!(support_file(&env));
    }

    #[test]
    fn set_rejects_unknown_values() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["set", "3", "urgent"])
            .assert()
            .code(1)
            .stderr("tasq: unknown status or priority 'urgent' (statuses: in-progress ready waiting blocked later; priorities: A B C)\n");
        env.tasq()
            .args(["set", "99", "A"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    #[test]
    fn log_appends_a_dated_note() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["log", "3", "talked to the customer"])
            .assert()
            .success()
            .stdout("[3] logged: talked to the customer\n");
        assert!(support_file(&env).ends_with(
            "- 2026-10-02 10:00: created via tasks create\n- 2026-10-07 09:30: talked to the customer\n"
        ));
        env.tasq()
            .args(["log", "3", "  "])
            .assert()
            .code(1)
            .stderr("tasq: the note must not be empty\n");
    }

    #[test]
    fn log_json_returns_the_task() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["--json", "log", "3", "note"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["task"]["id"], "3");
        let progress = value["task"]["progress"].as_array().unwrap();
        assert_eq!(progress.last().unwrap()["at"], NOW);
        assert_eq!(progress.last().unwrap()["note"], "note");
    }

    #[test]
    fn done_with_a_final_note() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["done", "3", "answered"])
            .assert()
            .success()
            .stdout("[3] done: Answer the support ticket\n");
        assert_snapshot!(support_file(&env));
        // Done tasks leave the list.
        let out = env.tasq().output().unwrap();
        assert!(!stdout(&out).contains("Answer the support ticket"));
    }

    #[test]
    fn done_on_a_done_task_is_refused() {
        let env = TestEnv::fixture();
        let before =
            std::fs::read_to_string(env.notebook().join("20260903110000.todo.md")).unwrap();
        env.tasq()
            .args(["done", "4", "--note", "again"])
            .assert()
            .code(1)
            .stdout("")
            .stderr("tasq: task 4 is already done\n");
        let after = std::fs::read_to_string(env.notebook().join("20260903110000.todo.md")).unwrap();
        assert_eq!(after, before);
    }

    fn release_file(env: &TestEnv) -> String {
        std::fs::read_to_string(env.notebook().join("20260903110000.todo.md")).unwrap()
    }

    #[test]
    fn reopen_with_the_default_status() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["reopen", "4"])
            .assert()
            .success()
            .stdout("[4] reopened -> ready: Ship the release notes\n");
        assert_eq!(
            release_file(&env),
            "# [ ] Ship the release notes\n\n## Tags\n\n#gitlab #C #ready\n\n## Progress\n\n\
             - 2026-10-03 11:00: created via tasks create\n- 2026-10-03 16:30: shipped\n\
             - 2026-10-07 09:30: reopened\n"
        );
        env.tasq()
            .args(["reopen", "4"])
            .assert()
            .code(1)
            .stderr("tasq: task 4 is not done\n");
    }

    #[test]
    fn reopen_with_a_status_and_a_note() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["reopen", "4", "#blocked", "typo in the notes"])
            .assert()
            .success()
            .stdout("[4] reopened -> blocked: Ship the release notes\n");
        let file = release_file(&env);
        assert!(file.starts_with("# [ ] Ship the release notes\n"), "{file}");
        assert!(file.contains("\n#gitlab #C #blocked\n"), "{file}");
        assert!(
            file.ends_with("- 2026-10-07 09:30: typo in the notes\n"),
            "{file}"
        );
    }

    #[test]
    fn reopen_takes_a_lone_non_status_word_as_the_note() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["--json", "reopen", "4", "not shipped"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["task"]["id"], "4");
        assert_eq!(value["task"]["done"], false);
        assert_eq!(value["task"]["status"], "ready");
        let progress = value["task"]["progress"].as_array().unwrap();
        assert_eq!(progress.last().unwrap()["note"], "not shipped");
    }

    #[test]
    fn reopen_rejects_an_unknown_status_and_empty_notes() {
        let env = TestEnv::fixture();
        let before = release_file(&env);
        env.tasq()
            .args(["reopen", "4", "soon", "x"])
            .assert()
            .code(1)
            .stderr(
                "tasq: unknown status 'soon' (statuses: in-progress ready waiting blocked later)\n",
            );
        env.tasq()
            .args(["reopen", "4", "ready", " "])
            .assert()
            .code(1)
            .stderr("tasq: the note must not be empty\n");
        assert_eq!(release_file(&env), before);
        env.tasq()
            .args(["reopen", "99"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    #[test]
    fn note_flag_is_an_alias_of_the_positional_note() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["set", "3", "A", "--note", "bumped"])
            .assert()
            .success()
            .stdout("[3] -> priority #A (bumped)\n");
        assert!(support_file(&env).ends_with("- 2026-10-07 09:30: bumped\n"));
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["done", "3", "--note", "answered"])
            .assert()
            .success()
            .stdout("[3] done: Answer the support ticket\n");
        assert!(support_file(&env).ends_with("- 2026-10-07 09:30: answered\n"));
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["reopen", "3", "--note", "not answered"])
            .assert()
            .success()
            .stdout("[3] reopened -> ready: Answer the support ticket\n");
        assert!(support_file(&env).ends_with("- 2026-10-07 09:30: not answered\n"));
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["reopen", "4", "blocked", "--note", "typo"])
            .assert()
            .success()
            .stdout("[4] reopened -> blocked: Ship the release notes\n");
        assert!(release_file(&env).ends_with("- 2026-10-07 09:30: typo\n"));
    }

    #[test]
    fn note_flag_and_positional_note_together_are_a_usage_error() {
        let env = TestEnv::fixture();
        let before = support_file(&env);
        for args in [
            &["set", "3", "A", "x", "--note", "y"][..],
            &["done", "3", "x", "--note", "y"],
            &["reopen", "4", "ready", "x", "--note", "y"],
        ] {
            env.tasq()
                .args(args)
                .assert()
                .code(2)
                .stderr(predicate::str::contains("cannot be used with"));
        }
        // With --note, a lone word after the id must be a status.
        env.tasq()
            .args(["reopen", "4", "not shipped", "--note", "y"])
            .assert()
            .code(1)
            .stderr(
                "tasq: unknown status 'not shipped' (statuses: in-progress ready waiting blocked later)\n",
            );
        assert_eq!(support_file(&env), before);
    }

    #[test]
    fn invalid_tasq_now_is_a_user_error() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", "yesterday")
            .args(["log", "3", "x"])
            .assert()
            .code(1)
            .stderr("tasq: TASQ_NOW=\"yesterday\": expected YYYY-MM-DD HH:MM\n");
    }
}

/// `tasq move`: the rank within a status group.
mod rank {
    use super::*;

    /// Task 1, the fixture's `#A` in-progress task.
    const FIRST: &str = "20260901090000.todo.md";
    /// Task 8, the `#C` in-progress task the tests add below task 1.
    const SECOND: &str = "20260907150000.todo.md";

    /// The fixture plus task 8, a `#C` in-progress task below task 1, with
    /// `rank_section` (`## Rank` and its body, or nothing) in its file.
    fn env_with_a_second_in_progress_task(rank_section: &str) -> TestEnv {
        let env = TestEnv::fixture();
        env.write_task(
            SECOND,
            &format!(
                "# [ ] Second\n\n{rank_section}## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-07 15:00: created\n"
            ),
        );
        env.register(SECOND);
        env
    }

    fn next_task(env: &TestEnv) -> String {
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["next", "--dry-run", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out).lines().next().unwrap().to_owned()
    }

    #[test]
    fn top_ranks_the_task_first_and_next_follows() {
        let env = env_with_a_second_in_progress_task("");
        assert_eq!(
            next_task(&env),
            "Task: [1] Rewrite the tasks script in Rust"
        );
        env.tasq()
            .args(["move", "8", "--top"])
            .assert()
            .success()
            .stdout("[8] moved to the top\n");
        assert_eq!(
            env.read_task(SECOND),
            "# [ ] Second\n\n## Rank\n\n1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-07 15:00: created\n"
        );
        assert_eq!(next_task(&env), "Task: [8] Second");
        // Now task 1 goes above it.
        env.tasq()
            .args(["move", "1", "--top"])
            .assert()
            .success()
            .stdout("[1] moved to the top\n");
        assert!(
            env.read_task(FIRST).contains("## Rank\n\n0\n"),
            "{}",
            env.read_task(FIRST)
        );
        assert_eq!(
            next_task(&env),
            "Task: [1] Rewrite the tasks script in Rust"
        );
    }

    #[test]
    fn top_overwrites_an_invalid_hand_written_rank() {
        // A non-integer `## Rank` reads as unranked (ADR 0025) and the move
        // replaces its body rather than adding a second section.
        let env = env_with_a_second_in_progress_task("## Rank\n\nsoon\n\n");
        env.tasq()
            .args(["move", "8", "--top"])
            .assert()
            .success()
            .stdout("[8] moved to the top\n");
        assert_eq!(
            env.read_task(SECOND),
            "# [ ] Second\n\n## Rank\n\n1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-07 15:00: created\n"
        );
    }

    #[test]
    fn top_json_names_the_rank_as_the_change() {
        let env = env_with_a_second_in_progress_task("");
        let out = env
            .tasq()
            .args(["move", "8", "--top", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn unrank_removes_the_section_and_is_idempotent() {
        let env = env_with_a_second_in_progress_task("");
        env.tasq().args(["move", "8", "--top"]).assert().success();
        env.tasq()
            .args(["move", "8", "--unrank"])
            .assert()
            .success()
            .stdout("[8] unranked\n");
        let before = env.read_task(SECOND);
        assert!(!before.contains("## Rank"), "{before}");
        assert_eq!(
            next_task(&env),
            "Task: [1] Rewrite the tasks script in Rust"
        );
        env.tasq()
            .args(["move", "8", "--unrank"])
            .assert()
            .success()
            .stdout("[8] already unranked\n");
        assert_eq!(env.read_task(SECOND), before);
    }

    #[test]
    fn a_move_that_changes_nothing_writes_nothing() {
        let env = env_with_a_second_in_progress_task("");
        let before = env.read_task(FIRST);
        env.tasq()
            .args(["move", "1", "--top"])
            .assert()
            .success()
            .stdout("[1] already at the top\n");
        assert_eq!(env.read_task(FIRST), before);
        let out = env
            .tasq()
            .args(["move", "1", "--top", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["task"]["id"], "1");
        assert_eq!(value["task"].get("rank"), None);
        assert_eq!(value["changed"], serde_json::json!([]));
        assert_eq!(value["ranked"], serde_json::json!([]));
        let second = env.read_task(SECOND);
        let out = env
            .tasq()
            .args(["move", "8", "--unrank", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["changed"], serde_json::json!([]));
        assert_eq!(value["ranked"], serde_json::json!([]));
        assert_eq!(env.read_task(FIRST), before);
        assert_eq!(env.read_task(SECOND), second);
    }

    #[test]
    fn a_done_or_missing_task_is_refused() {
        let env = TestEnv::fixture();
        let before = env.read_task("20260903110000.todo.md");
        env.tasq()
            .args(["move", "4", "--top"])
            .assert()
            .code(1)
            .stdout("")
            .stderr("tasq: task 4 is done\n");
        env.tasq()
            .args(["move", "4", "--unrank"])
            .assert()
            .code(1)
            .stderr("tasq: task 4 is done\n");
        assert_eq!(env.read_task("20260903110000.todo.md"), before);
        env.tasq()
            .args(["move", "99", "--top"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    /// Task 9, a second `#C` in-progress task, below task 8 by id.
    const THIRD: &str = "20260908160000.todo.md";

    /// [`env_with_a_second_in_progress_task`] plus task 9, another `#C`
    /// in-progress task, with `rank_9` in its file.
    fn env_with_three_in_progress_tasks(rank_8: &str, rank_9: &str) -> TestEnv {
        let env = env_with_a_second_in_progress_task(rank_8);
        env.write_task(
            THIRD,
            &format!(
                "# [ ] Third\n\n{rank_9}## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-08 16:00: created\n"
            ),
        );
        env.register(THIRD);
        env
    }

    #[test]
    fn before_and_after_place_the_task_next_to_the_anchor() {
        let env = env_with_three_in_progress_tasks("", "");
        env.tasq()
            .args(["move", "9", "--before", "1"])
            .assert()
            .success()
            .stdout("[9] moved before task 1\n");
        assert_eq!(
            env.read_task(THIRD),
            "# [ ] Third\n\n## Rank\n\n1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-08 16:00: created\n"
        );
        assert_eq!(next_task(&env), "Task: [9] Third");
        // Into the unranked part: tasks 1 and 8 get ranks first.
        env.tasq()
            .args(["move", "9", "--after", "8"])
            .assert()
            .success()
            .stdout("[9] moved after task 8 (also ranked: 1, 8)\n");
        assert!(env.read_task(FIRST).contains("## Rank\n\n1024\n"));
        assert!(env.read_task(SECOND).contains("## Rank\n\n2048\n"));
        assert!(env.read_task(THIRD).contains("## Rank\n\n3072\n"));
        assert_eq!(
            next_task(&env),
            "Task: [1] Rewrite the tasks script in Rust"
        );
        // Between two ranked neighbours: the midpoint, one file written.
        let first = env.read_task(FIRST);
        let second = env.read_task(SECOND);
        env.tasq()
            .args(["move", "9", "--before", "8"])
            .assert()
            .success()
            .stdout("[9] moved before task 8\n");
        assert!(env.read_task(THIRD).contains("## Rank\n\n1536\n"));
        assert_eq!(env.read_task(FIRST), first);
        assert_eq!(env.read_task(SECOND), second);
    }

    #[test]
    fn after_json_lists_the_other_tasks_ranked() {
        let env = env_with_three_in_progress_tasks("", "");
        let out = env
            .tasq()
            .args(["move", "1", "--after", "9", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn a_task_already_beside_its_anchor_is_left_alone() {
        let env = env_with_three_in_progress_tasks("", "");
        let first = env.read_task(FIRST);
        let second = env.read_task(SECOND);
        env.tasq()
            .args(["move", "8", "--after", "1"])
            .assert()
            .success()
            .stdout("[8] already after task 1\n");
        env.tasq()
            .args(["move", "1", "--before", "8"])
            .assert()
            .success()
            .stdout("[1] already before task 8\n");
        let out = env
            .tasq()
            .args(["move", "8", "--before", "9", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["changed"], serde_json::json!([]));
        assert_eq!(value["ranked"], serde_json::json!([]));
        assert_eq!(value["task"].get("rank"), None);
        assert_eq!(env.read_task(FIRST), first);
        assert_eq!(env.read_task(SECOND), second);
    }

    #[test]
    fn an_anchor_outside_the_group_is_refused() {
        let env = env_with_a_second_in_progress_task("");
        let before = env.read_task(SECOND);
        env.tasq()
            .args(["move", "8", "--before", "8"])
            .assert()
            .code(1)
            .stdout("")
            .stderr("tasq: task 8 cannot be moved relative to itself\n");
        env.tasq()
            .args(["move", "8", "--after", "3"])
            .assert()
            .code(1)
            .stderr("tasq: task 3 is ready but task 8 is in-progress; a move stays within one status group\n");
        env.tasq()
            .args(["move", "8", "--before", "4"])
            .assert()
            .code(1)
            .stderr("tasq: task 4 is done but task 8 is in-progress; a move stays within one status group\n");
        env.tasq()
            .args(["move", "7", "--after", "8"])
            .assert()
            .code(1)
            .stderr("tasq: task 8 is in-progress but task 7 is no-status; a move stays within one status group\n");
        env.tasq()
            .args(["move", "8", "--before", "99"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
        assert_eq!(env.read_task(SECOND), before);
    }

    #[test]
    fn a_slot_with_no_rank_left_renumbers_the_group() {
        let env = env_with_three_in_progress_tasks("## Rank\n\n1024\n\n", "## Rank\n\n1025\n\n");
        let second = env.read_task(SECOND);
        let out = env
            .tasq()
            .args(["move", "1", "--after", "8", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["changed"], serde_json::json!(["rank"]));
        assert_eq!(value["ranked"], serde_json::json!(["9"]));
        assert_eq!(value["task"]["rank"], serde_json::json!(2048));
        // 8 already holds 1024 and is not rewritten; 1 takes 2048 and 9
        // moves down to 3072.
        assert_eq!(env.read_task(SECOND), second);
        assert!(env.read_task(FIRST).contains("## Rank\n\n2048\n"));
        assert_eq!(
            env.read_task(THIRD),
            "# [ ] Third\n\n## Rank\n\n3072\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-08 16:00: created\n"
        );
        let out = env.tasq().args(["list", "--json"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let in_progress: Vec<&str> = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["status"] == "in-progress")
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(in_progress, ["8", "1", "9"]);
    }

    #[test]
    fn exactly_one_target_is_required() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["move", "3"]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert_snapshot!("no_target", stderr(&out));
        let out = env
            .tasq()
            .args(["move", "3", "--top", "--unrank"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert_snapshot!("two_targets", stderr(&out));
        env.tasq()
            .args(["move", "3", "--before", "1", "--after", "2"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"));
        env.tasq()
            .args(["move", "3", "--before"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("a value is required"));
        env.tasq().args(["move"]).assert().code(2);
    }
}

/// `tasq rename`, `due`, `tag` and `desc`: one field each, an optional
/// `--note`, and the task as stored with `--json`.
mod fields {
    use super::*;

    const SUPPORT: &str = "20260902100000.todo.md";

    fn support(env: &TestEnv) -> String {
        env.read_task(SUPPORT)
    }

    fn json(out: &std::process::Output) -> serde_json::Value {
        assert!(out.status.success(), "{}", stderr(out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        value
    }

    #[test]
    fn rename() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["rename", "3", "Answer the export question"])
            .assert()
            .success()
            .stdout("[3] renamed\n");
        assert_eq!(
            support(&env),
            "# [ ] Answer the export question\n\n## Description\n\n\
             Customer asks about the export format.\n\n## Tags\n\n#support #B #ready\n\n\
             ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
        );
    }

    #[test]
    fn rename_with_a_note_and_json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["rename", "3", "Reply", "--note", "shorter", "--json"])
            .output()
            .unwrap();
        let value = json(&out);
        assert_eq!(value["task"]["id"], "3");
        assert_eq!(value["task"]["title"], "Reply");
        assert_eq!(value["changed"], serde_json::json!(["title"]));
        assert!(
            support(&env).ends_with("- 2026-10-07 09:30: shorter\n"),
            "{}",
            support(&env)
        );
    }

    #[test]
    fn due_sets_a_date_after_the_description() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["due", "3", "2026-10-17"])
            .assert()
            .success()
            .stdout("[3] due 2026-10-17\n");
        assert_eq!(
            support(&env),
            "# [ ] Answer the support ticket\n\n## Description\n\n\
             Customer asks about the export format.\n\n## Due\n\n2026-10-17\n\n\
             ## Tags\n\n#support #B #ready\n\n\
             ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
        );
    }

    #[test]
    fn due_takes_relative_days_a_note_and_json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["due", "3", "Tomorrow", "--note", "promised", "--json"])
            .output()
            .unwrap();
        let value = json(&out);
        assert_eq!(value["task"]["id"], "3");
        assert_eq!(value["task"]["due"], "2026-10-08");
        assert_eq!(value["changed"], serde_json::json!(["due"]));
        assert!(
            support(&env).ends_with("- 2026-10-07 09:30: promised\n"),
            "{}",
            support(&env)
        );
    }

    #[test]
    fn due_none_clears_the_date() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["due", "1", "NONE"])
            .assert()
            .success()
            .stdout("[1] due cleared\n");
        let file = env.read_task("20260901090000.todo.md");
        assert!(!file.contains("## Due"), "{file}");
        assert!(
            file.contains("## Project\n\n/home/pau/code/tasks\n\n## Related\n"),
            "{file}"
        );
    }

    #[test]
    fn due_errors_write_nothing() {
        let env = TestEnv::fixture();
        let before = support(&env);
        env.tasq()
            .args(["due", "3", "friday"])
            .assert()
            .code(1)
            .stderr("tasq: invalid due date \"friday\" (expected YYYY-MM-DD, today, tomorrow, yesterday or none)\n");
        env.tasq()
            .args(["due", "3", "today", "--note", " "])
            .assert()
            .code(1)
            .stderr("tasq: the note must not be empty\n");
        env.tasq().args(["due", "3"]).assert().code(2);
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["due", "99", "none"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    #[test]
    fn tag_adds_and_removes_only_the_named_tags() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["tag", "3", "+needs-info", "-support", "urgent"])
            .assert()
            .success()
            .stdout("[3] tags: +needs-info -support +urgent\n");
        assert_eq!(
            support(&env),
            "# [ ] Answer the support ticket\n\n## Description\n\n\
             Customer asks about the export format.\n\n## Tags\n\n\
             #needs-info #urgent #B #ready\n\n\
             ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
        );
    }

    #[test]
    fn tag_takes_the_note_before_or_after_the_tags() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["tag", "3", "-support", "--note", "triaged", "--json"])
            .output()
            .unwrap();
        let value = json(&out);
        assert_eq!(value["task"]["tags"], serde_json::json!([]));
        assert_eq!(value["changed"], serde_json::json!(["tags"]));
        assert!(support(&env).ends_with("- 2026-10-07 09:30: triaged\n"));
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["tag", "3", "--note", "again", "-x", "+support"])
            .assert()
            .success()
            .stdout("[3] tags: +support\n");
        assert!(support(&env).contains("\n#support #B #ready\n"));
        assert!(support(&env).ends_with("- 2026-10-07 09:30: again\n"));
    }

    #[test]
    fn tag_reports_only_real_changes_and_writes_nothing_without_one() {
        let env = TestEnv::fixture();
        let before = support(&env);
        env.tasq()
            .args(["tag", "3", "support", "-absent", "+x", "-x"])
            .assert()
            .success()
            .stdout("[3] tags unchanged\n");
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["tag", "3", "-support", "+support", "+new"])
            .assert()
            .success()
            .stdout("[3] tags: +new\n");
        assert!(
            support(&env).contains("\n#support #new #B #ready\n"),
            "order kept"
        );
    }

    #[test]
    fn tag_adds_a_tags_section_when_there_is_none() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["tag", "7", "new"])
            .assert()
            .success()
            .stdout("[7] tags: +new\n");
        assert_eq!(
            env.read_task("20260906140000.todo.md"),
            "# [ ] Task without a tags section\n\n## Progress\n\n\
             - 2026-10-06 14:00: created via tasks create\n\n## Tags\n\n#new\n"
        );
    }

    #[test]
    fn tag_rejects_status_and_priority_words_before_writing() {
        let env = TestEnv::fixture();
        let before = support(&env);
        env.tasq()
            .args(["tag", "3", "+x", "-ready"])
            .assert()
            .code(1)
            .stderr("tasq: 'ready' is a status, not a tag; use set to change it\n");
        env.tasq()
            .args(["tag", "3", "#A"])
            .assert()
            .code(1)
            .stderr("tasq: 'A' is a priority, not a tag; use set to change it\n");
        env.tasq()
            .args(["tag", "3", "+"])
            .assert()
            .code(1)
            .stderr("tasq: invalid tag \"\": must not be empty\n");
        env.tasq().args(["tag", "3"]).assert().code(2);
        env.tasq()
            .args(["tag", "3", "-b", "--note", "n", "+a"])
            .assert()
            .code(2)
            .stderr(predicate::str::starts_with(
                "error: tag: put the options before or after the tags, not on both sides\n",
            ));
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["tag", "99", "x"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    const NO_TAGS: &str = "20260906140000.todo.md";

    #[test]
    fn desc_sets_the_description() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "3", "Export as CSV.\n\nThey use Excel."])
            .assert()
            .success()
            .stdout("[3] description updated\n");
        assert_eq!(
            support(&env),
            "# [ ] Answer the support ticket\n\n## Description\n\n\
             Export as CSV.\n\nThey use Excel.\n\n## Tags\n\n#support #B #ready\n\n\
             ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
        );
        env.tasq()
            .args(["desc", "7", "New"])
            .assert()
            .success()
            .stdout("[7] description updated\n");
        assert_eq!(
            env.read_task(NO_TAGS),
            "# [ ] Task without a tags section\n\n## Description\n\nNew\n\n## Progress\n\n\
             - 2026-10-06 14:00: created via tasks create\n"
        );
    }

    #[test]
    fn desc_dash_reads_stdin() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "3", "-"])
            .write_stdin("From a pipe.\n\nSecond paragraph.\n")
            .assert()
            .success()
            .stdout("[3] description updated\n");
        env.tasq()
            .args(["desc", "3"])
            .assert()
            .success()
            .stdout("From a pipe.\n\nSecond paragraph.\n");
        // A description that is literally "-" goes through stdin.
        env.tasq()
            .args(["desc", "3", "-"])
            .write_stdin("-")
            .assert()
            .success();
        env.tasq().args(["desc", "3"]).assert().stdout("-\n");
    }

    /// `<producer> | tasq desc 3 -` through a real pipe, so the producer
    /// can act after `tasq` started.
    fn desc_from_pipe(env: &TestEnv, producer: &str) -> std::process::Output {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "{{ {producer}; }} | {} desc 3 -",
                env!("CARGO_BIN_EXE_tasq")
            ))
            .env_clear()
            .env("TASQ_NOW", NOW)
            .env("HOME", &env.home)
            .env("NB_DIR", &env.nb_dir)
            .env("PATH", &env.bin)
            .current_dir(&env.home)
            .output()
            .unwrap()
    }

    #[test]
    fn desc_dash_conflicts_when_the_task_changed_while_stdin_was_open() {
        let env = TestEnv::fixture();
        let file = env.notebook().join(SUPPORT);
        let out = desc_from_pipe(
            &env,
            &format!(
                "/bin/sleep 1; printf -- '- 2026-10-07 09:00: elsewhere\\n' >> {}; printf 'Mine.'",
                file.display()
            ),
        );
        assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
        assert!(
            stderr(&out).starts_with("tasq: task 3 changed since it was read ("),
            "{}",
            stderr(&out)
        );
        assert!(support(&env).contains("Customer asks about the export format."));
        assert!(support(&env).ends_with("- 2026-10-07 09:00: elsewhere\n"));
    }

    #[test]
    fn desc_dash_with_the_same_text_writes_nothing() {
        let env = TestEnv::fixture();
        let out = desc_from_pipe(&env, "printf 'Customer asks about the export format.\\n'");
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stdout(&out), "[3] description unchanged\n");
    }

    #[test]
    fn desc_dash_normalises_like_the_file_reads_back() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "3", "-"])
            .write_stdin("\r\n \r\nPasted  \r\n\r\n  indented\r\n\r\n\t\r\n")
            .assert()
            .success()
            .stdout("[3] description updated\n");
        env.tasq()
            .args(["desc", "3"])
            .assert()
            .stdout("Pasted  \n\n  indented\n");
    }

    #[test]
    fn desc_without_text_prints_the_description() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "1"])
            .assert()
            .success()
            .stdout("Port the bash script to a Rust workspace.\nKeep the nb files unchanged.\n");
        env.tasq().args(["desc", "7"]).assert().success().stdout("");
        let value = json(&env.tasq().args(["desc", "3", "--json"]).output().unwrap());
        assert_eq!(
            value["task"]["description"],
            "Customer asks about the export format."
        );
        env.tasq()
            .args(["desc", "99"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    #[test]
    fn desc_empty_text_clears_the_description() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "3", ""])
            .assert()
            .success()
            .stdout("[3] description cleared\n");
        assert_eq!(
            support(&env),
            "# [ ] Answer the support ticket\n\n## Tags\n\n#support #B #ready\n\n\
             ## Progress\n\n- 2026-10-02 10:00: created via tasks create\n"
        );
        env.tasq()
            .args(["desc", "1", "-"])
            .write_stdin("\n")
            .assert()
            .success()
            .stdout("[1] description cleared\n");
        env.tasq()
            .args(["desc", "1", "-"])
            .write_stdin("")
            .assert()
            .success()
            .stdout("[1] description unchanged\n");
    }

    #[test]
    fn desc_with_a_note_and_json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["desc", "3", "-", "--note", "from the ticket", "--json"])
            .write_stdin("Pasted.\n")
            .output()
            .unwrap();
        let value = json(&out);
        assert_eq!(value["task"]["description"], "Pasted.");
        assert_eq!(value["changed"], serde_json::json!(["description"]));
        assert!(support(&env).ends_with("- 2026-10-07 09:30: from the ticket\n"));
    }

    #[test]
    fn desc_errors_write_nothing() {
        let env = TestEnv::fixture();
        let before = support(&env);
        env.tasq()
            .args(["desc", "3", "x", "--note", " "])
            .assert()
            .code(1)
            .stderr("tasq: the note must not be empty\n");
        env.tasq()
            .args(["desc", "3", "--note", "n"])
            .assert()
            .code(2);
        env.tasq()
            .args(["desc", "3", "-"])
            .write_stdin("Intro\n## Steps\n")
            .assert()
            .code(1)
            .stderr("tasq: a description line cannot start with '## ' (it would start a new section): ## Steps\n");
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["desc", "99", "x"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    /// `tasq desc 3 --edit` with `VISUAL=fake-editor`, a script that runs
    /// `body` with the temp file as `$1`.
    fn edit_with(env: &TestEnv, body: &str) -> assert_cmd::assert::Assert {
        env.fake_tool("fake-editor", body);
        env.tasq()
            .env("TASQ_NOW", NOW)
            .env("VISUAL", "fake-editor")
            .env("EDITOR", "not-this-one")
            .args(["desc", "3", "--edit"])
            .assert()
    }

    #[test]
    fn desc_edit_opens_the_description_and_saves_it() {
        let env = TestEnv::fixture();
        let seen = env.home.join("seen");
        edit_with(
            &env,
            &format!(
                "/bin/cat \"$1\" > {}\nprintf 'Edited.\\n\\nTwice.\\n' > \"$1\"",
                seen.display()
            ),
        )
        .success()
        .stdout("[3] description updated\n");
        assert_eq!(
            std::fs::read_to_string(&seen).unwrap(),
            "Customer asks about the export format.\n"
        );
        env.tasq()
            .args(["desc", "3"])
            .assert()
            .stdout("Edited.\n\nTwice.\n");
    }

    #[test]
    fn desc_edit_falls_back_to_editor_and_takes_a_note() {
        let env = TestEnv::fixture();
        env.fake_tool("fake-editor", "printf 'Via EDITOR' > \"$1\"");
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .env("VISUAL", " ")
            .env("EDITOR", "fake-editor")
            .args(["desc", "3", "--edit", "--note", "rewritten", "--json"])
            .output()
            .unwrap();
        let value = json(&out);
        assert_eq!(value["task"]["description"], "Via EDITOR");
        assert_eq!(value["changed"], serde_json::json!(["description"]));
        assert!(support(&env).ends_with("- 2026-10-07 09:30: rewritten\n"));
    }

    #[test]
    fn desc_edit_clears_on_an_empty_file() {
        let env = TestEnv::fixture();
        edit_with(&env, ": > \"$1\"")
            .success()
            .stdout("[3] description cleared\n");
        assert!(!support(&env).contains("## Description"));
    }

    #[test]
    fn desc_edit_writes_nothing_when_unchanged_or_aborted() {
        let env = TestEnv::fixture();
        let before = support(&env);
        edit_with(&env, ":")
            .success()
            .stdout("[3] description unchanged\n");
        edit_with(
            &env,
            "printf '\\n\\nCustomer asks about the export format.\\n\\n\\n' > \"$1\"",
        )
        .success()
        .stdout("[3] description unchanged\n");
        edit_with(&env, "exit 3").code(1).stderr(
            "tasq: fake-editor exited with exit status: 3; the description was not changed\n",
        );
        assert_eq!(support(&env), before);
    }

    /// The path a failed `desc --edit` names as holding the edited text.
    fn kept_text(assert: &assert_cmd::assert::Assert) -> String {
        let err = stderr(assert.get_output());
        let path = err
            .trim_end()
            .rsplit_once("; the edited text is kept in ")
            .unwrap_or_else(|| panic!("no kept path in {err:?}"))
            .1
            .to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        text
    }

    #[test]
    fn desc_edit_keeps_the_text_when_the_save_fails() {
        let env = TestEnv::fixture();
        let before = support(&env);
        let assert = edit_with(&env, "printf 'Lost.' > \"$1\"\nexit 3").code(1);
        assert!(stderr(assert.get_output()).starts_with(
            "tasq: fake-editor exited with exit status: 3; the description was not changed; the edited text is kept in "
        ));
        assert_eq!(kept_text(&assert), "Lost.");
        let assert = edit_with(&env, "printf 'Intro\\n## Steps\\n' > \"$1\"").code(1);
        assert!(stderr(assert.get_output()).starts_with(
            "tasq: a description line cannot start with '## ' (it would start a new section): ## Steps; the edited text is kept in "
        ));
        assert_eq!(kept_text(&assert), "Intro\n## Steps\n");
        assert_eq!(support(&env), before);
    }

    #[test]
    fn field_edits_that_change_nothing_write_nothing_and_say_so() {
        let env = TestEnv::fixture();
        let before = support(&env);
        for (args, line) in [
            (&["set", "3", "ready"][..], "[3] status unchanged\n"),
            (&["set", "3", "#B"], "[3] priority unchanged\n"),
            (
                &["rename", "3", " Answer the support ticket "],
                "[3] title unchanged\n",
            ),
            (&["due", "3", "none"], "[3] due unchanged\n"),
            (&["tag", "3", "+support"], "[3] tags unchanged\n"),
        ] {
            env.tasq().args(args).assert().success().stdout(line);
            let field = line["[3] ".len()..].trim_end_matches(" unchanged\n");
            env.tasq()
                .args(args)
                .args(["--note", "why"])
                .assert()
                .code(1)
                .stdout("")
                .stderr(format!(
                    "tasq: {field} unchanged; the note was not logged (use log to add it alone)\n"
                ));
        }
        assert_eq!(support(&env), before);
    }

    #[test]
    fn json_names_the_fields_written_and_is_empty_for_a_no_op() {
        let env = TestEnv::fixture();
        let before = support(&env);
        for args in [
            &["set", "3", "ready"][..],
            &["set", "3", "#B"],
            &["rename", "3", "Answer the support ticket"],
            &["due", "3", "none"],
            &["tag", "3", "+support"],
            &["desc", "3", "Customer asks about the export format."],
        ] {
            let value = json(&env.tasq().args(args).arg("--json").output().unwrap());
            assert_eq!(value["changed"], serde_json::json!([]), "{args:?}");
            assert_eq!(value["task"]["id"], "3", "{args:?}");
        }
        assert_eq!(support(&env), before);
        for (args, changed) in [
            (&["set", "3", "waiting"][..], "status"),
            (&["set", "3", "A"], "priority"),
            (&["rename", "3", "Reply"], "title"),
            (&["due", "3", "2026-10-17"], "due"),
            (&["tag", "3", "+export"], "tags"),
            (&["desc", "3", ""], "description"),
        ] {
            let value = json(&env.tasq().args(args).arg("--json").output().unwrap());
            assert_eq!(value["changed"], serde_json::json!([changed]), "{args:?}");
        }
        // Commands that are not one-field edits keep the plain document.
        let value = json(&env.tasq().args(["desc", "3", "--json"]).output().unwrap());
        assert_eq!(value.get("changed"), None);
    }

    #[test]
    fn desc_with_a_note_and_nothing_to_change_writes_nothing_and_says_so() {
        let env = TestEnv::fixture();
        let before = support(&env);
        let refused =
            "tasq: description unchanged; the note was not logged (use log to add it alone)\n";
        env.fake_tool(
            "fake-editor",
            "printf '\\nCustomer asks about the export format.\\n\\n' > \"$1\"",
        );
        env.tasq()
            .env("VISUAL", "fake-editor")
            .args(["desc", "3", "--edit", "--note", "still accurate"])
            .assert()
            .code(1)
            .stderr(refused);
        env.tasq()
            .args(["desc", "3", "-", "--note", "same"])
            .write_stdin("Customer asks about the export format.\n")
            .assert()
            .code(1)
            .stderr(refused);
        env.tasq()
            .args([
                "desc",
                "3",
                "Customer asks about the export format.",
                "--note",
                "same",
            ])
            .assert()
            .code(1)
            .stderr(refused);
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["desc", "3", "Customer asks about the export format."])
            .assert()
            .success()
            .stdout("[3] description unchanged\n");
    }

    #[test]
    fn desc_edit_conflicts_when_the_task_changed_meanwhile() {
        let env = TestEnv::fixture();
        let file = env.notebook().join(SUPPORT);
        let assert = edit_with(
            &env,
            &format!(
                "printf -- '- 2026-10-07 09:00: elsewhere\\n' >> {}\nprintf 'Mine.' > \"$1\"",
                file.display()
            ),
        )
        .code(1);
        assert!(
            stderr(assert.get_output()).starts_with("tasq: task 3 changed since it was read ("),
            "{}",
            stderr(assert.get_output())
        );
        assert_eq!(kept_text(&assert), "Mine.");
        assert!(support(&env).ends_with("- 2026-10-07 09:00: elsewhere\n"));
        assert!(support(&env).contains("Customer asks about the export format."));
    }

    #[test]
    fn desc_edit_needs_an_editor_and_no_text() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["desc", "3", "--edit"])
            .assert()
            .code(1)
            .stderr("tasq: no editor: set $VISUAL or $EDITOR\n");
        env.tasq()
            .env("VISUAL", "missing-editor")
            .args(["desc", "3", "--edit"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: could not run missing-editor: ",
            ));
        env.tasq()
            .env("VISUAL", "fake-editor")
            .args(["desc", "3", "x", "--edit"])
            .assert()
            .code(2);
        env.tasq()
            .env("VISUAL", "fake-editor")
            .args(["desc", "99", "--edit"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }

    #[test]
    fn rename_errors_write_nothing() {
        let env = TestEnv::fixture();
        let before = support(&env);
        env.tasq()
            .args(["rename", "3", "  "])
            .assert()
            .code(1)
            .stderr("tasq: the title must not be empty\n");
        env.tasq()
            .args(["rename", "3", "a\nb"])
            .assert()
            .code(1)
            .stderr("tasq: the title must be a single line\n");
        env.tasq()
            .args(["rename", "3", "x", "--note", ""])
            .assert()
            .code(1)
            .stderr("tasq: the note must not be empty\n");
        env.tasq().args(["rename", "3"]).assert().code(2);
        assert_eq!(support(&env), before);
        env.tasq()
            .args(["rename", "99", "x"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 99\n");
    }
}

mod create {
    use super::*;

    const NEW_FILE: &str = "20261007093000.todo.md";

    fn index(env: &TestEnv) -> String {
        std::fs::read_to_string(env.notebook().join(".index")).unwrap()
    }

    #[test]
    fn every_section() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        std::fs::create_dir(&project).unwrap();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args([
                "create",
                "Reply to the auditor",
                "--desc",
                "They asked for the Q3 export.",
                "--status",
                "waiting",
                "--prio",
                "A",
                "--due",
                "tomorrow",
                "--project",
                project.to_str().unwrap(),
                "--tag",
                "audit",
                "--tag",
                "#support",
                "--related",
                "https://example.invalid/ticket/9",
                "--note",
                "mail sent",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        assert_eq!(
            stdout(&out),
            "[8] created: Reply to the auditor (#waiting #A)\n"
        );
        assert!(index(&env).ends_with(&format!("20260906140000.todo.md\n{NEW_FILE}\n")));
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert_snapshot!(env.normalize(&file));
    }

    #[test]
    fn defaults() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["create", "Minimal"])
            .assert()
            .success()
            .stdout("[8] created: Minimal (#ready #B)\n");
        // Without --project the task tracks the directory tasq ran in.
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert_eq!(
            env.normalize(&file),
            "# [ ] Minimal\n\n## Project\n\n[ROOT]/home\n\n## Tags\n\n#B #ready\n\n## Progress\n\n- 2026-10-07 09:30: created via tasq create\n"
        );
        // The new task shows up in the list under READY with its id.
        let out = env.tasq().arg("ready").output().unwrap();
        assert!(stdout(&out).contains("[ 8] #B Minimal"), "{}", stdout(&out));
    }

    #[test]
    fn same_second_bumps_the_filename() {
        let env = TestEnv::fixture();
        for (id, title) in [("8", "First"), ("9", "Second")] {
            env.tasq()
                .env("TASQ_NOW", NOW)
                .args(["create", title])
                .assert()
                .success()
                .stdout(format!("[{id}] created: {title} (#ready #B)\n"));
        }
        assert!(env.notebook().join(NEW_FILE).is_file());
        assert!(env.notebook().join("20261007093001.todo.md").is_file());
    }

    #[test]
    fn status_done_creates_a_closed_task() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["create", "Already done", "--status", "done", "--prio", "C"])
            .assert()
            .success()
            .stdout("[8] created: Already done (done, #C)\n");
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert!(file.starts_with("# [x] Already done\n"), "{file}");
        assert!(file.contains("\n## Tags\n\n#C\n"), "{file}");
    }

    #[test]
    fn json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["--json", "create", "From JSON", "--due", "2026-12-24"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["task"]["id"], "8");
        assert_eq!(value["task"]["title"], "From JSON");
        assert_eq!(value["task"]["due"], "2026-12-24");
        assert_eq!(value["task"]["status"], "ready");
    }

    #[test]
    fn merge_request_without_a_lookup_gets_the_short_reference() {
        let env = TestEnv::fixture();
        let url = "https://gitlab.example.invalid/group/project/-/merge_requests/77";
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["create", "Review it", "--mr", url])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "", "no forge configured: silent fallback");
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert_snapshot!(env.normalize(&file));
        env.tasq()
            .args(["create", "Other", "--mr", "https://example.invalid/x"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: could not resolve the merge request title for https://example.invalid/x",
            ));
    }

    #[test]
    fn errors_write_nothing() {
        let env = TestEnv::fixture();
        let before = index(&env);
        env.tasq()
            .args(["create", "X", "--project", "/definitely/not/here"])
            .assert()
            .code(1)
            .stderr("tasq: project path not found: /definitely/not/here\n");
        env.tasq()
            .args(["create", "X", "--status", "nope"])
            .assert()
            .code(1)
            .stderr("tasq: unknown status 'nope' (statuses: in-progress ready waiting blocked later, done)\n");
        env.tasq()
            .args(["create", "X", "--due", "someday"])
            .assert()
            .code(1)
            .stderr("tasq: invalid date or timestamp \"someday\" (expected YYYY-MM-DD or YYYY-MM-DD HH:MM)\n");
        env.tasq()
            .args(["create", "X", "--prio", "D"])
            .assert()
            .code(1)
            .stderr("tasq: priority must be A, B or C\n");
        env.tasq().args(["create"]).assert().code(2);
        assert_eq!(index(&env), before);
        assert!(!env.notebook().join(NEW_FILE).exists());
    }
}

mod view {
    use super::*;

    const FULL: &str = "20260901090000.todo.md";

    #[test]
    fn piped_output_is_the_plain_file() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["view", "1"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stdout(&out), env.read_task(FULL));
    }

    #[test]
    fn raw_is_verbatim_even_with_glow_installed() {
        let env = TestEnv::fixture();
        env.fake_tool("glow", "echo rendered");
        let out = env.tasq().args(["view", "1", "--raw"]).output().unwrap();
        assert_eq!(stdout(&out), env.read_task(FULL));
    }

    #[test]
    fn json_and_errors() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["view", "1", "--json"]).output().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["task"]["title"], "Rewrite the tasks script in Rust");
        env.tasq()
            .args(["view", "5"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 5\n");
        env.tasq()
            .args(["view", "2"])
            .assert()
            .code(1)
            .stderr("tasq: no task with id 2\n");
    }
}

mod project {
    use super::*;

    #[test]
    fn show_and_set() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["project", "1"])
            .assert()
            .success()
            .stdout("/home/pau/code/tasks\n");
        env.tasq()
            .args(["project", "3"])
            .assert()
            .success()
            .stdout("no project tracked (default: unset)\n");
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", "/srv/app")
            .args(["project", "3"])
            .assert()
            .success()
            .stdout("no project tracked (default: /srv/app)\n");
        let dir = env.home.join("proj");
        std::fs::create_dir(&dir).unwrap();
        let out = env
            .tasq()
            .args(["project", "3", dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] project: [ROOT]/home/proj\n"
        );
        assert_snapshot!(env.normalize(&env.read_task("20260902100000.todo.md")));
        env.tasq()
            .args(["project", "3"])
            .assert()
            .success()
            .stdout(format!("{}\n", dir.display()));
        env.tasq()
            .args(["project", "3", "/definitely/not/here"])
            .assert()
            .code(1)
            .stderr("tasq: project path not found: /definitely/not/here\n");
    }

    #[test]
    fn show_json_includes_the_default() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", "/srv/app")
            .args(["project", "3", "--json"])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["task"]["project"], serde_json::Value::Null);
        assert_eq!(value["default_project"], "/srv/app");
    }
}

mod worktree {
    use super::*;

    #[test]
    fn track_records_the_branch_and_is_idempotent() {
        let env = TestEnv::fixture();
        let wt = env.home.join("wt");
        env.git_repo(&wt);
        env.git(&wt, &["checkout", "-q", "-b", "feature/x"]);
        let out = env
            .tasq_wt()
            .args(["worktree", "3", wt.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] worktree: [ROOT]/home/wt (feature/x)\n"
        );
        assert!(
            env.read_task("20260902100000.todo.md")
                .contains("\n## Worktrees\n\n- "),
        );
        assert!(
            env.normalize(&env.read_task("20260902100000.todo.md"))
                .contains("- [ROOT]/home/wt (`feature/x`)\n"),
        );
        let again = env
            .tasq_wt()
            .args(["worktree", "3", wt.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            env.normalize(&stdout(&again)),
            "[3] worktree already tracked: [ROOT]/home/wt\n"
        );
        // A plain directory has no branch.
        let plain = env.home.join("plain");
        std::fs::create_dir(&plain).unwrap();
        let out = env
            .tasq_wt()
            .args(["worktree", "3", plain.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] worktree: [ROOT]/home/plain\n"
        );
        env.tasq_wt()
            .args(["worktree", "3", "/definitely/not/here"])
            .assert()
            .code(1)
            .stderr("tasq: worktree path not found: /definitely/not/here\n");
        env.tasq_wt()
            .args(["worktree", "3"])
            .assert()
            .code(1)
            .stderr("tasq: task 3 tracks no project and work.default_project is unset; set one with tasq project 3 <path>\n");
    }

    /// A git repository at `<home>/proj` with an origin, tracked as task
    /// 3's project.
    fn project_repo(env: &TestEnv) -> std::path::PathBuf {
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.git(
            &project,
            &["remote", "add", "origin", "git@host:group/app.git"],
        );
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        project
    }

    #[test]
    fn declare_writes_the_entry_and_runs_nothing() {
        let env = TestEnv::fixture();
        project_repo(&env);
        let declare = |args: &[&str]| {
            let out = env
                .tasq_wt()
                .env("PATH", "")
                .args(["worktree", "3"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert_eq!(stderr(&out), "", "{args:?}");
            env.normalize(&stdout(&out))
        };
        assert_eq!(
            declare(&["feature/x"]),
            "[3] worktree declared: [ROOT]/home/proj-feature_x (feature/x)\n"
        );
        assert_eq!(
            declare(&["feature/x"]),
            "[3] worktree already declared: [ROOT]/home/proj-feature_x (feature/x)\n"
        );
        assert_eq!(
            declare(&[]),
            "[3] worktree declared: [ROOT]/home/proj-3_answer_the_support_ticket \
             (3-answer-the-support-ticket)\n"
        );
        assert!(
            env.normalize(&env.read_task("20260902100000.todo.md"))
                .contains(
                    "\n## Worktrees\n\n- [ROOT]/home/proj-feature_x (`feature/x`)\n\
                 - [ROOT]/home/proj-3_answer_the_support_ticket (`3-answer-the-support-ticket`)\n"
                ),
            "{}",
            env.read_task("20260902100000.todo.md")
        );
        assert!(!env.home.join("proj-feature_x").exists(), "nothing on disk");
        env.tasq_wt()
            .args(["worktree", "3", "a~b"])
            .assert()
            .code(1)
            .stderr("tasq: worktree path not found: a~b\n");
    }

    #[test]
    fn templates_see_the_main_checkout_and_the_repo() {
        let env = TestEnv::fixture();
        let project = project_repo(&env);
        let linked = env.home.join("linked");
        env.git(
            &project,
            &["worktree", "add", "-q", "-b", "l", linked.to_str().unwrap()],
        );
        env.tasq_wt()
            .args(["project", "3", linked.join("sub").to_str().unwrap()])
            .assert()
            .code(1);
        std::fs::create_dir(linked.join("sub")).unwrap();
        env.tasq_wt()
            .args(["project", "3", linked.join("sub").to_str().unwrap()])
            .assert()
            .success();
        let out = env
            .tasq_wt()
            .env("PATH", "")
            .env(
                "TASQ_WORKTREE_PATH",
                "{{project}}/../wt/{{repo}}/{{project_name}}-{{branch}}",
            )
            .args(["worktree", "3", "b"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] worktree declared: [ROOT]/home/proj/../wt/app/proj-b (b)\n"
        );
        // A git directory kept elsewhere: the checkout is the project.
        let separate = env.home.join("sep");
        env.git(
            &env.home,
            &[
                "init",
                "-q",
                "--separate-git-dir",
                env.root.path().join("store").to_str().unwrap(),
                separate.to_str().unwrap(),
            ],
        );
        env.tasq_wt()
            .args(["project", "3", separate.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .env("PATH", "")
            .args(["worktree", "3", "c"])
            .assert()
            .success()
            .stdout(predicate::str::ends_with("/home/sep-c (c)\n"));
    }

    #[test]
    fn create_declares_on_flag_or_in_a_git_checkout() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        env.git_repo(&project);
        let plain = env.home.join("plain");
        std::fs::create_dir(&plain).unwrap();
        let create = |args: &[&str]| {
            let out = env
                .tasq_wt()
                .env("PATH", "")
                .arg("create")
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert_eq!(stderr(&out), "", "{args:?}");
            env.normalize(&stdout(&out))
        };
        let p = project.to_str().unwrap();
        assert_eq!(
            create(&["Fix login", "--project", p, "--worktree"]),
            "[8] created: Fix login (#ready #B)\n\
             [8] worktree declared: [ROOT]/home/proj-8_fix_login (8-fix-login)\n"
        );
        assert_eq!(
            create(&["Auto", "--project", p]),
            "[9] created: Auto (#ready #B)\n\
             [9] worktree declared: [ROOT]/home/proj-9_auto (9-auto)\n"
        );
        assert_eq!(
            create(&["Named", "--project", p, "--worktree", "feat/n"]),
            "[10] created: Named (#ready #B)\n\
             [10] worktree declared: [ROOT]/home/proj-feat_n (feat/n)\n"
        );
        assert_eq!(
            create(&["Plain", "--project", plain.to_str().unwrap()]),
            "[11] created: Plain (#ready #B)\n"
        );
        assert_eq!(
            create(&["Never", "--project", p, "--set", "worktree.on_create=never"]),
            "[12] created: Never (#ready #B)\n"
        );
        let json = env
            .tasq_wt()
            .args(["create", "J", "--project", p, "--json"])
            .output()
            .unwrap();
        assert!(
            env.normalize(&stdout(&json))
                .contains("\"path\": \"[ROOT]/home/proj-13_j\""),
            "{}",
            stdout(&json)
        );
    }

    #[test]
    fn create_checks_the_flag_before_writing() {
        let env = TestEnv::fixture();
        let index = env.read_task(".index");
        env.tasq_wt()
            .args(["create", "T", "--worktree", "a b"])
            .assert()
            .code(1)
            .stderr("tasq: invalid branch name \"a b\"\n");
        env.tasq()
            .args(["create", "T", "--worktree"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
        assert_eq!(env.read_task(".index"), index, "nothing created");
        // Disabled, a git checkout gets no worktree.
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.tasq()
            .args(["create", "Off", "--project", project.to_str().unwrap()])
            .assert()
            .success()
            .stdout("[8] created: Off (#ready #B)\n");
    }

    #[test]
    fn create_declares_and_creates_now() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        // Another worktree that exists does not stand in for the new one.
        let other = env.home.join("other");
        std::fs::create_dir(&other).unwrap();
        env.tasq_wt()
            .args(["worktree", "3", other.to_str().unwrap()])
            .assert()
            .success();
        let out = env
            .tasq_wt()
            .args(["worktree", "3", "--create", "feature/y"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] created worktree [ROOT]/home/proj-feature_y on feature/y (new from HEAD)\n"
        );
        assert!(env.home.join("proj-feature_y").join("README").is_file());
        assert!(
            env.normalize(&env.read_task("20260902100000.todo.md"))
                .contains("- [ROOT]/home/proj-feature_y (`feature/y`)\n")
        );
        env.tasq_wt()
            .args(["worktree", "3", "--create", "feature/y"])
            .assert()
            .success()
            .stdout(predicate::str::ends_with(
                "/home/proj-feature_y (feature/y)\n",
            ));
    }

    #[test]
    fn up_tracks_a_branch_only_the_remote_has() {
        let env = TestEnv::fixture();
        let seed = env.home.join("seed");
        env.git_repo(&seed);
        env.git(&env.home, &["clone", "-q", "--bare", "seed", "origin.git"]);
        env.git(&env.home, &["clone", "-q", "origin.git", "proj"]);
        env.git(&seed, &["push", "-q", "../origin.git", "main:feat/r"]);
        let project = env.home.join("proj");
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", "feat/r"])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "up", "3", "--dry-run"])
            .assert()
            .success()
            .stdout(predicate::str::ends_with(
                "/home/proj-feat_r on feat/r (tracking origin/feat/r)\n",
            ));
        assert!(
            !env.git(&project, &["branch", "-r"]).contains("feat/r"),
            "a dry run fetches nothing"
        );
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        let wt = env.home.join("proj-feat_r");
        assert_eq!(
            env.git(&wt, &["rev-parse", "--abbrev-ref", "@{upstream}"])
                .trim(),
            "origin/feat/r"
        );
    }

    /// Task 3 in a repository at `<home>/proj` whose own `.tasq.toml` (not
    /// the one in the directory tasq runs from) sets the layout and the
    /// provisioning; templates live next to that file.
    fn provisioned_project(env: &TestEnv, template: &str) -> std::path::PathBuf {
        let project = env.home.join("proj");
        env.git_repo(&project);
        std::fs::write(
            project.join(".gitignore"),
            "node_modules/\n.env\n.tasq.toml\ntpl/\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.join("node_modules/pkg")).unwrap();
        std::fs::write(project.join(".env"), "SECRET=1\n").unwrap();
        std::fs::create_dir(project.join("tpl")).unwrap();
        std::fs::write(project.join("tpl/envrc"), template).unwrap();
        std::fs::write(
            project.join(".tasq.toml"),
            "[worktree]\n\
             path = \"{{project_parent}}/wt-{{branch_safe}}\"\n\
             symlinks = [\"node_modules\", \"missing\"]\n\
             copies = [\".env\"]\n\
             templates = [{ source = \"tpl/envrc\", target = \".envrc\" }]\n",
        )
        .unwrap();
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", "b"])
            .assert()
            .success();
        project
    }

    #[test]
    fn up_provisions_from_the_repository_config() {
        let env = TestEnv::fixture();
        let project = provisioned_project(&env, "export BRANCH={{branch}} TASK={{id}}\n");
        let wt = env.home.join("wt-b");
        let dry = env
            .tasq_wt()
            .args(["worktree", "up", "3", "--dry-run"])
            .output()
            .unwrap();
        assert!(dry.status.success(), "{}", stderr(&dry));
        assert_eq!(
            env.normalize(&stdout(&dry)),
            "[3] would create worktree [ROOT]/home/wt-b on b (new from HEAD)\n  \
             copy .env from [ROOT]/home/proj/.env\n  \
             link node_modules -> [ROOT]/home/proj/node_modules\n  \
             link missing -> [ROOT]/home/proj/missing\n  \
             render .envrc from [ROOT]/home/proj/tpl/envrc\n"
        );
        assert!(!wt.exists());
        let out = env
            .tasq_wt()
            .args(["worktree", "up", "3"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            env.normalize(&stderr(&out))
                .contains("tasq: warning: link: [ROOT]/home/proj/missing not found, skipped\n"),
            "{}",
            stderr(&out)
        );
        assert_eq!(
            std::fs::read_link(wt.join("node_modules")).unwrap(),
            project.join("node_modules")
        );
        assert_eq!(
            std::fs::read_to_string(wt.join(".env")).unwrap(),
            "SECRET=1\n"
        );
        assert_eq!(
            std::fs::read_to_string(wt.join(".envrc")).unwrap(),
            "export BRANCH=b TASK=3\n"
        );
    }

    #[test]
    fn a_broken_template_stops_before_git_runs() {
        let env = TestEnv::fixture();
        provisioned_project(&env, "{{nope}}\n");
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "/home/proj/tpl/envrc: unknown placeholder {{nope}}\n",
            ));
        assert!(!env.home.join("wt-b").exists());
        env.tasq_wt()
            .args(["worktree", "up", "3", "--dry-run"])
            .assert()
            .code(1);
    }

    /// Task 3 in a repository at `<home>/proj` declaring `b`, with a `hook`
    /// program that logs where it ran and what it got to `<home>/hooks.log`.
    fn hooked_project(env: &TestEnv, repo_hooks: &str) -> std::path::PathBuf {
        let project = env.home.join("proj");
        env.git_repo(&project);
        std::fs::write(project.join(".gitignore"), ".tasq.toml\n").unwrap();
        std::fs::write(project.join(".tasq.toml"), repo_hooks).unwrap();
        env.fake_tool(
            "hook",
            "echo \"$1|$TASQ_TASK_ID|$TASQ_BRANCH|$TASQ_WORKTREE|$TASQ_VAR_BRANCH_SAFE|$(pwd)|$2\" >> \"$HOME/hooks.log\"",
        );
        env.fake_tool("fail", "echo nope >&2; exit 3");
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", "b"])
            .assert()
            .success();
        project
    }

    const REPO_HOOKS: &str = "[worktree.hooks]\n\
        pre-create = [\"hook pre\"]\n\
        post-create = [\"hook post '{{title}}'\", \"hook after\"]\n";

    fn hook_log(env: &TestEnv) -> String {
        env.normalize(&std::fs::read_to_string(env.home.join("hooks.log")).unwrap_or_default())
    }

    /// Removes the worktree `b` so the next `up` creates it again.
    fn remove_b(env: &TestEnv, project: &std::path::Path) {
        let wt = env.home.join("proj-b");
        env.git(
            project,
            &["worktree", "remove", "--force", wt.to_str().unwrap()],
        );
        env.git(project, &["branch", "-D", "b"]);
        let _ = std::fs::remove_file(env.home.join("hooks.log"));
    }

    #[test]
    fn repository_hooks_run_only_once_approved() {
        let env = TestEnv::fixture();
        let project = hooked_project(&env, REPO_HOOKS);
        // Not approved, no terminal: skipped with a warning; the worktree is made.
        let out = env
            .tasq_wt()
            .args(["worktree", "up", "3"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            env.normalize(&stderr(&out)).contains(
                "tasq: warning: hooks from [ROOT]/home/proj/.tasq.toml are not approved and were \
                 skipped: \"hook pre\", \"hook post '{{title}}'\", \"hook after\"; approve them with tasq worktree \
                 trust [ROOT]/home/proj, or pass --yes\n"
            ),
            "{}",
            stderr(&out)
        );
        assert_eq!(hook_log(&env), "");
        assert!(env.home.join("proj-b").is_dir());
        // --yes runs them this once: pre-create in the main checkout,
        // post-create in the worktree, both with the variables.
        remove_b(&env, &project);
        env.tasq_wt()
            .args(["worktree", "up", "3", "--yes"])
            .assert()
            .success();
        assert_eq!(
            hook_log(&env),
            "pre|3|b|[ROOT]/home/proj-b|b|[ROOT]/home/proj|\n\
             post|3|b|[ROOT]/home/proj-b|b|[ROOT]/home/proj-b|Answer the support ticket\n\
             after|3|b|[ROOT]/home/proj-b|b|[ROOT]/home/proj-b|\n"
        );
        remove_b(&env, &project);
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        assert_eq!(hook_log(&env), "", "--yes stored nothing");
        // `trust` approves ahead of time.
        remove_b(&env, &project);
        env.tasq_wt()
            .current_dir(&project)
            .args(["worktree", "trust"])
            .assert()
            .success()
            .stdout(predicate::str::ends_with(
                "/home/proj/.tasq.toml:\n  pre-create: hook pre\n  post-create: hook post '{{title}}'\n  post-create: hook after\n",
            ));
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        assert_eq!(hook_log(&env).lines().count(), 3);
        // An edited line asks again; the unchanged one still runs.
        remove_b(&env, &project);
        std::fs::write(
            project.join(".tasq.toml"),
            REPO_HOOKS.replace("hook post", "hook changed"),
        )
        .unwrap();
        let dry = env
            .tasq_wt()
            .args(["worktree", "up", "3", "--dry-run"])
            .output()
            .unwrap();
        assert_eq!(
            env.normalize(&stdout(&dry)),
            "[3] would create worktree [ROOT]/home/proj-b on b (new from HEAD)\n  \
             run pre-create hook: hook pre\n  \
             run post-create hook: hook after\n  \
             skip post-create hook (not approved): hook changed '{{title}}'\n"
        );
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        let log = hook_log(&env);
        let ran: Vec<&str> = log.lines().map(|l| l.split('|').next().unwrap()).collect();
        assert_eq!(ran, ["pre", "after"], "only the edited line is skipped");
        env.tasq_wt()
            .args(["worktree", "trust", env.home.join("nope").to_str().unwrap()])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with("/home/nope\n"));
    }

    #[test]
    fn hook_output_shows_with_verbose_and_a_broken_trust_file_is_an_error() {
        let env = TestEnv::fixture();
        let project = hooked_project(&env, "");
        env.fake_tool("say", "echo hello");
        env.write_global_config(
            "[worktree.hooks]\npre-create = [\"hook pre\"]\npost-create = [\"say\"]\n",
        );
        let out = env
            .tasq_wt()
            .args(["worktree", "up", "3", "-v"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("tasq: post-create hook \"say\": hello\n"),
            "{}",
            stderr(&out)
        );
        assert!(
            !stderr(&out).contains("pre-create hook"),
            "a silent hook says nothing"
        );
        remove_b(&env, &project);
        std::fs::write(project.join(".tasq.toml"), REPO_HOOKS).unwrap();
        let state = env.home.join(".local/state/tasq");
        std::fs::create_dir_all(state.join("trust.toml")).unwrap();
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("/state/tasq/trust.toml: "));
        std::fs::remove_dir(state.join("trust.toml")).unwrap();
        std::fs::write(state.join("trust.toml"), "not toml [").unwrap();
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("/state/tasq/trust.toml: "));
    }

    #[test]
    fn global_hooks_run_without_asking_and_pre_create_can_stop() {
        let env = TestEnv::fixture();
        let project = hooked_project(&env, "");
        env.write_global_config(
            "[worktree.hooks]\npre-create = [\"hook pre\"]\npost-create = [\"fail\"]\n",
        );
        let out = env
            .tasq_wt()
            .args(["worktree", "up", "3"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stderr(&out).ends_with(
                "\ntasq: warning: post-create hook \"fail\" failed: exit status 3: nope\n"
            ),
            "{}",
            stderr(&out)
        );
        assert!(hook_log(&env).starts_with("pre|3|b|"));
        assert!(
            env.home.join("proj-b").is_dir(),
            "a post-create failure keeps the worktree"
        );
        remove_b(&env, &project);
        env.write_global_config("[worktree.hooks]\npre-create = [\"fail\"]\n");
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "\ntasq: pre-create hook \"fail\" failed: exit status 3: nope\n",
            ));
        assert!(
            !env.home.join("proj-b").exists(),
            "a pre-create failure creates nothing"
        );
        env.tasq_wt()
            .args(["worktree", "trust", project.to_str().unwrap()])
            .assert()
            .success()
            .stdout(predicate::str::ends_with("/home/proj/.tasq.toml\n"))
            .stdout(predicate::str::starts_with("no hooks to approve in "));
    }

    #[test]
    fn trust_json_lists_the_approved_lines() {
        let env = TestEnv::fixture();
        let project = hooked_project(&env, REPO_HOOKS);
        let out = env
            .tasq_wt()
            .args(["worktree", "trust", project.to_str().unwrap(), "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "{\n  \"approved\": [\n    {\n      \"kind\": \"pre-create\",\n      \
             \"line\": \"hook pre\"\n    },\n    {\n      \"kind\": \"post-create\",\n      \
             \"line\": \"hook post '{{title}}'\"\n    },\n    {\n      \
             \"kind\": \"post-create\",\n      \"line\": \"hook after\"\n    }\n  ],\n  \
             \"file\": \"[ROOT]/home/proj/.tasq.toml\",\n  \"schema\": 1\n}\n"
        );
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        assert_eq!(hook_log(&env).lines().count(), 3, "--json approved them");
        let other = env.home.join("other");
        env.git_repo(&other);
        let out = env
            .tasq_wt()
            .args(["--json", "worktree", "trust", other.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "{\n  \"approved\": [],\n  \"file\": \"[ROOT]/home/other\",\n  \"schema\": 1\n}\n"
        );
    }

    #[test]
    fn up_creates_the_declared_worktree() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.git(&project, &["branch", "start"]);
        env.git(&project, &["checkout", "-q", "-b", "elsewhere"]);
        std::fs::write(project.join("later"), "x\n").unwrap();
        env.git(&project, &["add", "later"]);
        env.git(&project, &["commit", "-q", "-m", "later"]);
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(
                "tasq: task 3 declares no worktree with a branch; declare one with tasq worktree 3 [BRANCH]\n",
            );
        env.tasq_wt()
            .args(["worktree", "3", "b"])
            .assert()
            .success();
        let up = |args: &[&str]| {
            let out = env
                .tasq_wt()
                .args(["worktree", "up", "3"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            env.normalize(&stdout(&out))
        };
        assert_eq!(
            up(&["--from", "start", "--dry-run"]),
            "[3] would create worktree [ROOT]/home/proj-b on b (new from start)\n"
        );
        assert!(!env.home.join("proj-b").exists());
        assert_eq!(
            up(&["--from", "start"]),
            "[3] created worktree [ROOT]/home/proj-b on b (new from start)\n"
        );
        assert!(
            !env.home.join("proj-b").join("later").exists(),
            "from start"
        );
        assert_eq!(up(&[]), "[3] worktree exists: [ROOT]/home/proj-b (b)\n");
        env.tasq()
            .args(["worktree", "up", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
    }

    #[test]
    fn create_errors() {
        let env = TestEnv::fixture();
        env.tasq_wt()
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr("tasq: task 3 tracks no project and work.default_project is unset; set one with tasq project 3 <path>\n");
        let project = env.home.join("proj");
        std::fs::create_dir(&project).unwrap();
        // The removed command manager keys name their replacement.
        env.tasq_wt()
            .args([
                "worktree",
                "3",
                "--create",
                "b",
                "--set",
                "work.worktree_manager=command",
            ])
            .assert()
            .code(1)
            .stderr("tasq: --set: unknown config key \"work.worktree_manager\"\n");
        // git needs a repository; nothing is left at the path.
        env.tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains(
                "tasq: git worktree add --no-track -b b ",
            ));
        assert!(!env.home.join("proj-b").exists());
    }
}

mod session {
    use super::*;

    #[test]
    fn track_with_hint_and_idempotence() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["session", "3", "sess-1", "first look"])
            .assert()
            .success()
            .stdout("[3] session: sess-1 (resume: claude --resume sess-1)\n");
        assert!(
            env.read_task("20260902100000.todo.md")
                .contains("\n## Sessions\n\n- 2026-10-07 09:30: `sess-1` \u{2014} first look\n"),
            "{}",
            env.read_task("20260902100000.todo.md")
        );
        env.tasq()
            .args(["session", "3", "sess-1"])
            .assert()
            .success()
            .stdout("[3] session already tracked: sess-1\n");
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["session", "3", "sess-2", "--launcher", "shell"])
            .assert()
            .success()
            .stdout("[3] session: sess-2\n");
        env.tasq()
            .args(["session", "3", "a`b"])
            .assert()
            .code(1)
            .stderr("tasq: the session id must not be empty or contain backticks\n");
    }
}

mod mr {
    use super::*;

    #[test]
    fn track_resolve_and_idempotence() {
        let env = TestEnv::fixture();
        let url = "https://gitlab.example.invalid/group/project/-/merge_requests/9";
        env.tasq()
            .args(["mr", "3", url])
            .assert()
            .success()
            .stdout("[3] MR: group/project!9\n");
        env.tasq()
            .args(["mr", "3", url, "Explicit title"])
            .assert()
            .success()
            .stdout(format!("[3] MR already tracked: {url}\n"));
        env.tasq()
            .args([
                "mr",
                "3",
                "https://example.invalid/review/1",
                "Reviewed by hand",
            ])
            .assert()
            .success()
            .stdout("[3] MR: Reviewed by hand\n");
        assert_snapshot!(env.read_task("20260902100000.todo.md"));
        env.tasq()
            .args(["mr", "3", "https://example.invalid/review/2"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: could not resolve the merge request title",
            ));
        // The fixture's full task already tracks !123.
        env.tasq()
            .args([
                "mr",
                "1",
                "https://gitlab.example.invalid/group/project/-/merge_requests/123",
            ])
            .assert()
            .success()
            .stdout(predicate::str::starts_with("[1] MR already tracked:"));
    }
}

mod apply {
    use super::*;

    #[test]
    fn view_json_piped_back_is_a_no_op() {
        let env = TestEnv::fixture();
        let file = env.notebook().join("20260901090000.todo.md");
        let before = std::fs::read_to_string(&file).unwrap();
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
        let json = env.tasq().args(["view", "1", "--json"]).output().unwrap();
        env.tasq()
            .arg("apply")
            .write_stdin(json.stdout)
            .assert()
            .success()
            .stdout("[1] applied\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), mtime);
    }

    /// Task 3 (`ready`, `#B`) with a hand-written rank.
    const RANKED: &str = "# [ ] Answer the support ticket\n\n## Description\n\nCustomer asks about the export format.\n\n## Rank\n\n1024\n\n## Tags\n\n#support #B #ready\n\n## Progress\n\n- 2026-10-02 10:00: created via tasks create\n";

    #[test]
    fn rank_is_a_number_in_json_kept_without_the_key_and_cleared_by_null() {
        let env = TestEnv::fixture();
        env.write_task("20260902100000.todo.md", RANKED);
        let json = env.tasq().args(["view", "3", "--json"]).output().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["task"]["rank"], 1024);
        // The round trip is a no-op on a ranked task.
        env.tasq()
            .arg("apply")
            .write_stdin(json.stdout.clone())
            .assert()
            .success()
            .stdout("[3] applied\n");
        assert_eq!(env.read_task("20260902100000.todo.md"), RANKED);
        // A document without the key (older or hand-written) keeps the rank.
        value["task"].as_object_mut().unwrap().remove("rank");
        value["task"]["priority"] = "A".into();
        env.tasq()
            .arg("apply")
            .write_stdin(value.to_string())
            .assert()
            .success();
        assert_eq!(
            env.read_task("20260902100000.todo.md"),
            RANKED.replace("#support #B #ready", "#support #ready #A")
        );
        // An unranked task has no `rank` key, and `null` clears the rank.
        let unranked = env.tasq().args(["view", "1", "--json"]).output().unwrap();
        let unranked: serde_json::Value = serde_json::from_slice(&unranked.stdout).unwrap();
        assert_eq!(unranked["task"].get("rank"), None);
        value["task"]["rank"] = serde_json::Value::Null;
        value["task"]["priority"] = "B".into();
        env.tasq()
            .arg("apply")
            .write_stdin(value.to_string())
            .assert()
            .success();
        assert_eq!(
            env.read_task("20260902100000.todo.md"),
            RANKED
                .replace("## Rank\n\n1024\n\n", "")
                .replace("#support #B #ready", "#support #ready #B")
        );
        // And a number sets it, at the same place in the file.
        value["task"]["rank"] = 2048.into();
        env.tasq()
            .arg("apply")
            .write_stdin(value.to_string())
            .assert()
            .success();
        assert_eq!(
            env.read_task("20260902100000.todo.md"),
            RANKED
                .replace("1024", "2048")
                .replace("#support #B #ready", "#support #ready #B")
        );
    }

    #[test]
    fn edits_go_through_the_store() {
        let env = TestEnv::fixture();
        let json = env.tasq().args(["view", "3", "--json"]).output().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        value["task"]["status"] = "blocked".into();
        value["task"]["priority"] = "A".into();
        value["task"]["progress"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"at": NOW, "note": "via apply"}));
        let file = env.home.join("task.json");
        std::fs::write(&file, value.to_string()).unwrap();
        let out = env
            .tasq()
            .args(["apply", file.to_str().unwrap(), "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let back: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(back["task"]["status"], "blocked");
        assert_snapshot!(env.read_task("20260902100000.todo.md"));
    }

    #[test]
    fn unsupported_edits_and_bad_input_are_refused() {
        let env = TestEnv::fixture();
        let json = env.tasq().args(["view", "3", "--json"]).output().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        value["task"]["progress"] = serde_json::json!([]);
        let before = env.read_task("20260902100000.todo.md");
        env.tasq()
            .arg("apply")
            .write_stdin(value.to_string())
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: unsupported: changing progress of task 3",
            ));
        assert_eq!(env.read_task("20260902100000.todo.md"), before);
        env.tasq()
            .arg("apply")
            .write_stdin("{\"schema\": 2, \"task\": {}}")
            .assert()
            .code(1)
            .stderr("tasq: apply: unsupported schema 2 (expected 1)\n");
        env.tasq()
            .args(["apply", "/definitely/not/here.json"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: apply: cannot read /definitely/not/here.json",
            ));
    }
}

mod launch {
    use super::*;

    const PROMPT_START: &str = "Work with me on my next task: todo [3]";

    #[test]
    fn pick_dry_run_shows_the_plan_without_writing() {
        let env = TestEnv::fixture();
        let before = env.read_task("20260902100000.todo.md");
        let out = env
            .tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        assert_snapshot!(env.normalize(&stdout(&out)));
        assert_eq!(env.read_task("20260902100000.todo.md"), before);
    }

    #[test]
    fn pick_runs_claude_with_the_prompt_and_marks_in_progress() {
        let env = TestEnv::fixture();
        let log = env.home.join("claude.log");
        env.fake_tool(
            "claude",
            &format!(
                "[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\n{{ echo \"cwd=$PWD\"; echo \"id=$TASQ_TASK_ID nb=$TASQ_NOTEBOOK\"; echo \"argc=$#\"; printf '%.60s\\n' \"$1\"; }} > '{}'",
                log.display()
            ),
        );
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["pick", "3"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] -> in-progress\nTask: [3] Answer the support ticket\nStarting in project: [ROOT]/home\n"
        );
        let logged = env.normalize(&std::fs::read_to_string(&log).unwrap());
        assert_eq!(
            logged,
            format!(
                "cwd=[ROOT]/home\nid=3 nb=home\nargc=1\n{PROMPT_START} from my nb notebook (\n"
            )
        );
        assert!(
            env.read_task("20260902100000.todo.md")
                .contains("#support #B #in-progress"),
        );
        // Already in progress: no status line the second time.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3"])
            .output()
            .unwrap();
        assert!(stdout(&out).starts_with("Task: [3]"), "{}", stdout(&out));
    }

    #[test]
    fn next_picks_the_first_in_progress_task() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["next", "--dry-run", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        // Task 1 tracks a project that does not exist here.
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: tracked project not found: /home/pau/code/tasks; using work.default_project\n"
        );
        assert_eq!(
            env.normalize(&stdout(&out)),
            "Task: [1] Rewrite the tasks script in Rust\nStarting in project: [ROOT]/home\nLauncher: shell (dry run)\ncd [ROOT]/home\nTASQ_TASK_ID=1 TASQ_NOTEBOOK=home exec sh\n"
        );
    }

    #[test]
    fn next_picks_the_top_ranked_task_of_the_group() {
        let env = TestEnv::fixture();
        env.write_task(
            "20260907150000.todo.md",
            "# [ ] Ranked\n\n## Rank\n\n1024\n\n## Tags\n\n#C #in-progress\n\n## Progress\n\n- 2026-10-07 15:00: created\n",
        );
        env.register("20260907150000.todo.md");
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["next", "--dry-run", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stdout(&out).starts_with("Task: [8] Ranked\n"),
            "{}",
            stdout(&out)
        );
    }

    #[test]
    fn next_without_candidates() {
        let env = TestEnv::empty();
        env.tasq()
            .args(["next"])
            .assert()
            .code(1)
            .stderr("tasq: no in-progress or ready todos\n");
    }

    /// Task 3 with a git repository at `<home>/proj` as its project and a
    /// worktree declared on `branch`; a fake `$SHELL` that prints where it
    /// runs.
    fn declared(env: &TestEnv, branch: &str) -> std::path::PathBuf {
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", branch])
            .assert()
            .success();
        env.fake_tool("fakesh", "pwd");
        project
    }

    #[test]
    fn a_launch_creates_the_declared_worktree_and_starts_there() {
        let env = TestEnv::fixture();
        declared(&env, "feat/x");
        let wt = env.home.join("proj-feat_x");
        let dry = env
            .tasq_wt()
            .args(["pick", "3", "--launcher", "shell", "--dry-run"])
            .output()
            .unwrap();
        assert!(dry.status.success(), "{}", stderr(&dry));
        assert_eq!(
            env.normalize(&stdout(&dry)),
            "[3] -> in-progress (skipped: dry run)\n\
             Task: [3] Answer the support ticket\n\
             Starting in tracked worktree: [ROOT]/home/proj-feat_x\n\
             Launcher: shell (dry run)\n\
             create worktree [ROOT]/home/proj-feat_x on feat/x (new from HEAD)\n\
             cd [ROOT]/home/proj-feat_x\n\
             TASQ_TASK_ID=3 TASQ_NOTEBOOK=home exec sh\n"
        );
        assert!(
            stderr(&dry).contains("origin/HEAD is not set; feat/x starts from HEAD"),
            "{}",
            stderr(&dry)
        );
        assert!(
            stderr(&dry).contains(
                "tasq: warning: branch feat/x is gone or new: neither the repository nor \
                 origin has it; it starts fresh from HEAD\n"
            ),
            "{}",
            stderr(&dry)
        );
        assert!(!wt.exists(), "a dry run creates nothing");
        let out = env
            .tasq_wt()
            .env("SHELL", env.bin.join("fakesh"))
            .args(["pick", "3", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] -> in-progress\n\
             Task: [3] Answer the support ticket\n\
             Created worktree: [ROOT]/home/proj-feat_x on feat/x (new from HEAD)\n\
             Starting in tracked worktree: [ROOT]/home/proj-feat_x\n\
             [ROOT]/home/proj-feat_x\n"
        );
        assert_eq!(env.git(&wt, &["branch", "--show-current"]).trim(), "feat/x");
        // Next time it exists and is reused.
        let again = env
            .tasq_wt()
            .env("SHELL", env.bin.join("fakesh"))
            .args(["pick", "3", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(!stdout(&again).contains("Created"), "{}", stdout(&again));
        assert!(stdout(&again).ends_with("/home/proj-feat_x\n"));
    }

    #[test]
    fn a_worktree_that_cannot_be_created_aborts_the_launch() {
        let env = TestEnv::fixture();
        // `main` is checked out in the project itself.
        declared(&env, "main");
        let before = env.read_task("20260902100000.todo.md");
        env.tasq_wt()
            .env("SHELL", env.bin.join("fakesh"))
            .args(["pick", "3", "--launcher", "shell"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(
                predicate::str::starts_with("tasq: git worktree add ")
                    .and(predicate::str::contains("'main'")),
            );
        assert_eq!(
            env.read_task("20260902100000.todo.md"),
            before,
            "the task is not set in-progress"
        );
        assert!(!env.home.join("proj-main").exists());
    }

    #[test]
    fn a_gone_worktree_without_a_branch_starts_in_the_project() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        std::fs::create_dir(&project).unwrap();
        let plain = env.home.join("plain");
        std::fs::create_dir(&plain).unwrap();
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", plain.to_str().unwrap()])
            .assert()
            .success();
        std::fs::remove_dir(&plain).unwrap();
        let out = env
            .tasq_wt()
            .args(["pick", "3", "--launcher", "shell", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: tracked worktree is gone: [ROOT]/home/plain; it records no branch to create it on, starting in the project\n"
        );
        assert!(stdout(&out).contains("Starting in project: "));
    }

    #[test]
    fn launcher_errors() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3", "--launcher", "tmux", "--dry-run"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "tasq: tmux: not inside a tmux session ($TMUX is unset)\n",
            ));
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3", "--launcher", "nope", "--dry-run"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "tasq: nope: unknown launcher (available: auto, claude, shell, tmux, herdr)\n",
            ));
        env.tasq()
            .args(["pick", "4", "--dry-run"])
            .assert()
            .code(1)
            .stderr("tasq: task 4 is done; reopen it first (tasq reopen 4)\n");
        env.tasq()
            .args(["pick", "3", "--dry-run"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "tasq: task 3 tracks no project and work.default_project is unset; set one with tasq project 3 <path>\n",
            ));
    }

    #[test]
    fn detached_uses_launch_detached_and_no_focus_stays_put() {
        let env = TestEnv::fixture();
        // Inside herdr, `auto` is herdr; `--no-focus` drops the focus step.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("HERDR_ENV", "1")
            .args([
                "pick",
                "3",
                "--detached",
                "--no-focus",
                "--dry-run",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["launcher"], "herdr");
        let steps: Vec<&str> = value["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert!(steps[0].starts_with("herdr workspace create"), "{steps:?}");
        assert!(
            steps.contains(&"(no focus change: the session opens in the background)"),
            "{steps:?}"
        );
        assert!(
            !steps.iter().any(|s| s.starts_with("herdr workspace focus")),
            "{steps:?}"
        );
        // Inside tmux only, `auto` is tmux and the window is created detached.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TMUX", "/tmp/tmux-1000/default,1,0")
            .args([
                "pick",
                "3",
                "--detached",
                "--no-focus",
                "--dry-run",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["launcher"], "tmux");
        assert!(
            value["steps"][0]
                .as_str()
                .unwrap()
                .starts_with("tmux new-window -d -c "),
            "{}",
            value["steps"]
        );
        // With focus (the default) the tmux window is selected.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TMUX", "/tmp/tmux-1000/default,1,0")
            .args(["pick", "3", "--detached", "--dry-run", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(
            value["steps"][0]
                .as_str()
                .unwrap()
                .starts_with("tmux new-window -c "),
            "{}",
            value["steps"]
        );
    }

    #[test]
    fn detached_launch_traces_to_stderr_at_verbose_and_to_launch_log() {
        let env = TestEnv::fixture();
        env.fake_tool(
            "tmux",
            "echo 'created' ; echo 'no server running' >&2; exit 1",
        );
        let log = env.home.join("logs/launch.log");
        let home = env.home.display().to_string();
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", &home)
            .env("TASQ_LAUNCH_LOG", "~/logs/launch.log")
            .env("TMUX", "/tmp/tmux-1000/default,1,0")
            .args(["-v", "pick", "3", "--detached", "--no-focus"])
            .assert()
            .code(1)
            .stderr(predicate::str::contains(format!(
                "$ tmux new-window -d -c {home} -n "
            )))
            .stderr(predicate::str::contains(
                "  failed\n  stdout:\n    created\n  stderr:\n    no server running\n",
            ))
            .stderr(predicate::str::ends_with(
                "tasq: tmux failed: no server running\n",
            ));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.starts_with(&format!(
                "--- 2026-10-06 09:00:00 task 3 with tmux in {home} (no focus)\n$ tmux new-window -d "
            )),
            "{text}"
        );
        assert!(
            text.ends_with("    no server running\nerror: tmux failed: no server running\n"),
            "{text}"
        );
        // Without -v nothing is traced on stderr; an opened window is logged.
        env.fake_tool("tmux", "exit 0");
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", &home)
            .env("TASQ_LAUNCH_LOG", log.to_str().unwrap())
            .env("TMUX", "/tmp/tmux-1000/default,1,0")
            .args(["pick", "3", "--detached"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(!stderr(&out).contains("$ tmux"), "{}", stderr(&out));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains(&format!(
                "--- 2026-10-06 09:00:00 task 3 with tmux in {home}\n$ tmux new-window -c "
            )),
            "{text}"
        );
        assert!(
            text.contains("  ok\nopened: opened tmux window \"3 "),
            "{text}"
        );
    }

    #[test]
    fn detached_outside_a_window_manager_is_refused_before_any_write() {
        let env = TestEnv::fixture();
        // Outside herdr and tmux there is nowhere to open: the task stays ready.
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3", "--detached"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "tasq: launch.detached: auto: no window to open a session in: not inside herdr or tmux (set launch.detached)\n",
            ));
        let out = env.tasq().args(["view", "3", "--json"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["task"]["status"], "ready");
        // `--launcher` wins over `launch.detached`; `--no-focus` needs `--detached`.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args([
                "pick",
                "3",
                "--detached",
                "--launcher",
                "shell",
                "--dry-run",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["launcher"], "shell");
        env.tasq()
            .args(["pick", "3", "--no-focus", "--dry-run"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("--detached"));
    }

    #[test]
    fn auto_launcher_is_herdr_inside_herdr() {
        let env = TestEnv::fixture();
        let out = env
            .tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("HERDR_ENV", "1")
            .env("TASQ_LAUNCHER", "auto")
            .args(["pick", "3", "--dry-run", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["launcher"], "herdr");
        assert_eq!(value["in_worktree"], false);
        assert_eq!(value["env"][0][0], "TASQ_TASK_ID");
        let steps = value["steps"].as_array().unwrap();
        // The session is in the default project, so no holding-workspace lookup.
        assert!(
            steps[0]
                .as_str()
                .unwrap()
                .starts_with("herdr workspace create")
        );
        assert!(
            steps
                .iter()
                .any(|s| s.as_str().unwrap().contains("herdr workspace rename"))
        );
        let outside = env
            .tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCHER", "auto")
            .args(["pick", "3", "--dry-run", "--json"])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&outside.stdout).unwrap();
        assert_eq!(value["launcher"], "claude");
    }

    #[test]
    fn custom_prompt_template_and_profile_env() {
        let env = TestEnv::fixture();
        let template = env.home.join("prompt.md");
        std::fs::write(
            &template,
            "Custom prompt for [{{id}}] {{title}} in {{workdir}}\n",
        )
        .unwrap();
        env.write_global_config(&format!(
            "[work]\ndefault_project = \"{}\"\n\n[launch.claude]\nprompt_file = \"{}\"\n\n[profile.work]\n",
            env.home.display(),
            template.display()
        ));
        let out = env
            .tasq()
            .args(["--profile", "work", "pick", "3", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let text = env.normalize(&stdout(&out));
        assert!(
            text.contains("TASQ_TASK_ID=3 TASQ_NOTEBOOK=home TASQ_PROFILE=work exec claude"),
            "{text}"
        );
        assert!(
            text.ends_with(
                "--- prompt ---\nCustom prompt for [3] Answer the support ticket in [ROOT]/home\n"
            ),
            "{text}"
        );
        std::fs::write(&template, "{{bogus}}").unwrap();
        env.tasq()
            .args(["pick", "3", "--dry-run"])
            .assert()
            .code(1)
            .stderr(predicate::str::ends_with(
                "tasq: prompt template: unknown placeholder {{bogus}}\n",
            ));
    }

    #[test]
    fn session_hint_comes_from_the_launcher() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["session", "3", "s-1", "--launcher", "nope"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: nope: unknown launcher"));
        env.tasq()
            .env("TASQ_NOW", NOW)
            .env("HERDR_ENV", "1")
            .args(["session", "3", "s-1", "--launcher", "auto"])
            .assert()
            .success()
            .stdout("[3] session: s-1 (resume: claude --resume s-1)\n");
    }
}

mod sync {
    use super::*;
    use support::FakeHttp;

    const USER: &str = "{\"id\":42,\"username\":\"pau\"}";
    const MR7: &str = "{\"iid\":7,\"title\":\"Add parser\",\"web_url\":\"https://gl.test/group/project/-/merge_requests/7\",\"state\":\"opened\",\"draft\":false}";
    const LIST: &str = "/api/v4/merge_requests?scope=all&state=opened&reviewer_id=42&per_page=100";
    const MR7_API: &str = "/api/v4/projects/group%2Fproject/merge_requests/7";

    fn bridge_config(env: &TestEnv, name: &str, command: &str, extra: &str) -> String {
        format!(
            "[[source]]\nname = \"{name}\"\nkind = \"llm-bridge\"\ncommand = \"{command}\"\nprompt_file = \"{}\"\ntags = [\"inbox\"]\n{extra}\n",
            env.home.join("prompt.md").display()
        )
    }

    fn install_bridge(env: &TestEnv) {
        std::fs::write(env.home.join("prompt.md"), "triage\n").unwrap();
        env.fake_tool(
            "inbox-bridge",
            "[ -n \"${FAKE_PROBE:-}\" ] && exit 0\nread -r prompt\necho '[{\"external_id\":\"slack:C1/1\",\"url\":\"https://slack.test/C1/1\",\"title\":\"Reply to Ana\",\"body\":\"Export format question.\",\"status\":\"ready\",\"priority\":\"A\",\"tags\":[\"slack\"]},{\"external_id\":\"slack:C1/2\",\"title\":\"reply  to ana\"}]'",
        );
    }

    #[test]
    fn bridge_creates_tasks_once() {
        let env = TestEnv::fixture();
        install_bridge(&env);
        env.write_project_config(&bridge_config(&env, "inbox", "inbox-bridge", ""));
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .arg("sync")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "inbox: 1 change(s)\n  [8] created: Reply to Ana\n"
        );
        assert_snapshot!(env.read_task("20261007093000.todo.md"));
        // The same items again: matched by `## Source`, nothing to do.
        env.tasq()
            .env("TASQ_NOW", NOW)
            .arg("sync")
            .assert()
            .success()
            .stdout("inbox: up to date\n");
        // Re-checking by id: the bridge reports open, nothing happens.
        env.tasq()
            .args(["sync", "8"])
            .assert()
            .success()
            .stdout("inbox: up to date\n");
    }

    #[test]
    fn dry_run_and_json() {
        let env = TestEnv::fixture();
        install_bridge(&env);
        env.write_project_config(&bridge_config(
            &env,
            "inbox",
            "inbox-bridge",
            "status = \"later\"",
        ));
        let before = std::fs::read_to_string(env.notebook().join(".index")).unwrap();
        env.tasq()
            .args(["sync", "--dry-run"])
            .assert()
            .success()
            .stdout("inbox: 1 change(s) (dry run)\n  create: Reply to Ana\n");
        assert_eq!(
            std::fs::read_to_string(env.notebook().join(".index")).unwrap(),
            before
        );
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .args(["sync", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["ok"], true);
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["sources"][0]["name"], "inbox");
        assert_eq!(value["sources"][0]["applied"][0]["id"], "8");
        assert_eq!(value["sources"][0]["error"], serde_json::Value::Null);
        assert!(
            env.read_task("20261007093000.todo.md")
                .contains("#inbox #slack #A #ready") // the item's status wins over the source default
        );
    }

    #[test]
    fn a_failing_source_does_not_stop_the_others() {
        let env = TestEnv::fixture();
        install_bridge(&env);
        env.fake_tool(
            "bad-bridge",
            "[ -n \"${FAKE_PROBE:-}\" ] && exit 0\necho 'quota exceeded' >&2\nexit 2",
        );
        let config = format!(
            "{}{}",
            bridge_config(&env, "bad", "bad-bridge", ""),
            bridge_config(&env, "inbox", "inbox-bridge", "")
        );
        env.write_project_config(&config);
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .arg("sync")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stdout(&out),
            "bad: failed: command \"bad-bridge\" failed: quota exceeded\ninbox: 1 change(s)\n  [8] created: Reply to Ana\n"
        );
        env.tasq()
            .args(["sync", "--source", "nope"])
            .assert()
            .code(1)
            .stderr("tasq: no enabled source called \"nope\" (sources: bad, inbox)\n");
        env.tasq()
            .args(["sync", "--source", "inbox"])
            .assert()
            .success()
            .stdout("inbox: up to date\n");
        // `--source` repeats; the first unknown name is the error, and the
        // named sources still run in config order.
        env.tasq()
            .args(["sync", "--source", "inbox", "--source", "nope"])
            .assert()
            .code(1)
            .stderr("tasq: no enabled source called \"nope\" (sources: bad, inbox)\n");
        let out = env
            .tasq()
            .args(["sync", "--source", "inbox", "--source", "bad"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stdout(&out),
            "bad: failed: command \"bad-bridge\" failed: quota exceeded\ninbox: up to date\n"
        );
    }

    #[test]
    fn auto_false_sources_run_only_when_named() {
        let env = TestEnv::fixture();
        install_bridge(&env);
        env.fake_tool(
            "quiet-bridge",
            "[ -n \"${FAKE_PROBE:-}\" ] && exit 0\necho '[]'",
        );
        let config = format!(
            "{}{}",
            bridge_config(&env, "inbox", "inbox-bridge", "auto = false"),
            bridge_config(&env, "quiet", "quiet-bridge", "")
        );
        env.write_project_config(&config);
        // A bare sync skips `inbox`: the only line is the auto source's.
        env.tasq()
            .env("TASQ_NOW", NOW)
            .arg("sync")
            .assert()
            .success()
            .stdout("quiet: up to date\n");
        // Named, it runs, auto or not, and the auto source is left out.
        env.tasq()
            .env("TASQ_NOW", NOW)
            .args(["sync", "--source", "inbox"])
            .assert()
            .success()
            .stdout("inbox: 1 change(s)\n  [8] created: Reply to Ana\n");
        // Nothing runs by default when every enabled source is on demand.
        env.write_project_config(&bridge_config(
            &env,
            "inbox",
            "inbox-bridge",
            "auto = false",
        ));
        env.tasq()
            .arg("sync")
            .assert()
            .code(1)
            .stderr(
                "tasq: every enabled source has auto = false; name one with --source (sources: inbox)\n",
            );
        // `enabled = false` is still "cannot run", even when named.
        env.write_project_config(&bridge_config(
            &env,
            "inbox",
            "inbox-bridge",
            "enabled = false\nauto = false",
        ));
        env.tasq()
            .args(["sync", "--source", "inbox"])
            .assert()
            .code(1)
            .stderr("tasq: no [[source]] is configured (see docs/config.md and docs/sources.md)\n");
    }

    #[test]
    fn without_sources() {
        let env = TestEnv::fixture();
        env.tasq()
            .arg("sync")
            .assert()
            .code(1)
            .stderr("tasq: no [[source]] is configured (see docs/config.md and docs/sources.md)\n");
    }

    fn gitlab_config(base: &str) -> String {
        format!(
            "[forge.gitlab]\nhost = \"gl.test\"\nurl = \"{base}/api/v4\"\n\n[[source]]\nname = \"gitlab-review-requests\"\nkind = \"gitlab-review-requests\"\nforge = \"gitlab\"\ntags = [\"gitlab\", \"review-request\"]\nstatus = \"ready\"\nflag = \"review-request\"\n"
        )
    }

    #[test]
    fn gitlab_review_requests_create_then_close() {
        let env = TestEnv::fixture();
        let open = FakeHttp::start(vec![
            ("/api/v4/user", 200, USER),
            (LIST, 200, &format!("[{MR7}]")),
        ]);
        env.write_project_config(&gitlab_config(&open.base));
        let out = env
            .tasq()
            .env("TASQ_NOW", NOW)
            .env("GITLAB_TOKEN", "secret")
            .arg("sync")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "gitlab-review-requests: 1 change(s)\n  [8] created: Review MR !7: Add parser\n"
        );
        assert_eq!(open.requests(), vec!["/api/v4/user", LIST]);
        assert_snapshot!(env.read_task("20261007093000.todo.md"));
        // Later the MR is merged: the sweep no longer lists it and the
        // individual check closes the task.
        let merged = FakeHttp::start(vec![
            ("/api/v4/user", 200, USER),
            (LIST, 200, "[]"),
            (MR7_API, 200, &MR7.replace("\"opened\"", "\"merged\"")),
            (&format!("{MR7_API}/approvals"), 200, "{\"approved_by\":[]}"),
        ]);
        env.write_project_config(&gitlab_config(&merged.base));
        let out = env
            .tasq()
            .env("TASQ_NOW", "2026-10-08 10:00")
            .env("GITLAB_TOKEN", "secret")
            .arg("sync")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "gitlab-review-requests: 1 change(s)\n  [8] done: sync(gitlab-review-requests): merged\n"
        );
        let file = env.read_task("20261007093000.todo.md");
        assert!(
            file.starts_with("# [x] Review MR !7: Add parser\n"),
            "{file}"
        );
        assert!(
            file.contains("- 2026-10-08 10:00: sync(gitlab-review-requests): merged\n"),
            "{file}"
        );
        assert!(
            file.contains("\n#gitlab #review-request #B\n"),
            "status tag stripped: {file}"
        );
        // A third sweep: the done task is matched and left alone.
        env.tasq()
            .env("GITLAB_TOKEN", "secret")
            .arg("sync")
            .assert()
            .success()
            .stdout("gitlab-review-requests: up to date\n");
    }

    #[test]
    fn missing_token_is_reported_per_source() {
        let env = TestEnv::fixture();
        env.write_project_config(&gitlab_config("http://127.0.0.1:9"));
        env.tasq()
            .arg("sync")
            .assert()
            .code(1)
            .stdout("gitlab-review-requests: failed: authentication: no token for forge.gitlab: set forge.gitlab.token_cmd or the GITLAB_TOKEN environment variable\n");
    }

    #[test]
    fn mr_titles_come_from_the_forge() {
        let env = TestEnv::fixture();
        let server = FakeHttp::start(vec![
            ("/api/v4/user", 200, USER),
            (
                "/api/v4/projects/g%2Fp/merge_requests/9",
                200,
                "{\"iid\":9,\"title\":\"Real title\",\"web_url\":\"https://gl.test/g/p/-/merge_requests/9\",\"state\":\"opened\"}",
            ),
            (
                "/api/v4/projects/g%2Fp/merge_requests/9/approvals",
                200,
                "{\"approved_by\":[]}",
            ),
        ]);
        env.write_project_config(&format!(
            "[forge.gitlab]\nhost = \"gl.test\"\nurl = \"{}/api/v4\"\n",
            server.base
        ));
        env.tasq()
            .env("GITLAB_TOKEN", "secret")
            .args(["mr", "3", "https://gl.test/g/p/-/merge_requests/9"])
            .assert()
            .success()
            .stdout("[3] MR: Real title\n");
        // The lookup fails: warning and the short reference.
        let out = env
            .tasq()
            .env("GITLAB_TOKEN", "secret")
            .args(["mr", "3", "https://gl.test/g/p/-/merge_requests/10"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stdout(&out), "[3] MR: g/p!10\n");
        assert_eq!(stderr(&out), "", "a gone MR is not an error");
        // Another host: no forge, silent fallback.
        env.tasq()
            .args(["mr", "3", "https://other.test/g/p/-/merge_requests/1"])
            .assert()
            .success()
            .stdout("[3] MR: g/p!1\n");
        // Without a token the lookup fails loudly but the MR is still tracked.
        let out = env
            .tasq()
            .args([
                "create",
                "Review it",
                "--mr",
                "https://gl.test/g/p/-/merge_requests/11",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(stderr(&out).starts_with("tasq: warning: could not look up the title of https://gl.test/g/p/-/merge_requests/11: authentication:"), "{}", stderr(&out));
    }
    #[test]
    fn interactive_dry_run_prints_the_command() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["sync", "--interactive", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        assert_eq!(
            env.normalize(&stdout(&out)),
            "Launcher: claude (dry run)\ncd [ROOT]/home\nTASQ_NOTEBOOK=home exec claude \"/tasq:sync\"\n"
        );
        // The default strategy with an allowed .envrc wraps in direnv exec;
        // the profile travels with the notebook.
        std::fs::write(env.home.join(".envrc"), "").unwrap();
        env.fake_tool("direnv", "echo 'Found RC allowed true'");
        env.write_project_config("[profile.work]\n");
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args([
                "--profile",
                "work",
                "sync",
                "--interactive",
                "--dry-run",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let home = env.home.to_str().unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["launcher"], "claude");
        assert_eq!(value["workdir"], home);
        assert_eq!(
            value["command"],
            serde_json::json!(["direnv", "exec", home, "claude", "/tasq:sync"])
        );
        assert_eq!(
            value["env"],
            serde_json::json!([["TASQ_NOTEBOOK", "home"], ["TASQ_PROFILE", "work"]])
        );
        assert_eq!(
            value["steps"],
            serde_json::json!([
                format!("cd {home}"),
                format!(
                    "TASQ_NOTEBOOK=home TASQ_PROFILE=work exec direnv exec {home} claude \"/tasq:sync\""
                ),
            ])
        );
        // A .envrc direnv refuses is a warning step, not a wrapped command.
        env.fake_tool("direnv", "echo 'Found RC allowed false'");
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["sync", "--interactive", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "Launcher: claude (dry run)\n\
             warning: [ROOT]/home/.envrc is not allowed by direnv; run: direnv allow [ROOT]/home\n\
             cd [ROOT]/home\n\
             TASQ_NOTEBOOK=home exec claude \"/tasq:sync\"\n"
        );
    }

    #[test]
    fn interactive_runs_claude_in_the_default_project() {
        let env = TestEnv::fixture();
        let project = env.home.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let log = env.home.join("claude.log");
        env.fake_tool(
            "claude",
            &format!(
                "[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\n{{ echo \"cwd=$PWD\"; echo \"nb=$TASQ_NOTEBOOK task=${{TASQ_TASK_ID:-unset}}\"; echo \"argc=$#\"; echo \"$1\"; }} > '{}'",
                log.display()
            ),
        );
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["sync", "--interactive"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stdout(&out), "");
        assert_eq!(stderr(&out), "");
        assert_eq!(
            env.normalize(&std::fs::read_to_string(&log).unwrap()),
            "cwd=[ROOT]/home/project\nnb=home task=unset\nargc=1\n/tasq:sync\n"
        );
        // A .envrc direnv refuses: the warning goes to stderr, claude still runs.
        std::fs::write(project.join(".envrc"), "").unwrap();
        env.fake_tool("direnv", "echo 'Found RC allowed false'");
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .args(["sync", "--interactive"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: [ROOT]/home/project/.envrc is not allowed by direnv; run: direnv allow [ROOT]/home/project\n"
        );
    }

    #[test]
    fn interactive_needs_a_default_project_and_a_claude() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["sync", "--interactive", "--dry-run"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(
                "tasq: work.default_project is unset; the briefing session needs a directory to run in\n",
            );
        let gone = env.home.join("gone");
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", gone.to_str().unwrap())
            .args(["sync", "--interactive", "--dry-run"])
            .assert()
            .code(1)
            .stdout("")
            .stderr(format!(
                "tasq: work.default_project not found: {}\n",
                gone.display()
            ));
        // No claude on PATH: exec fails and reports the program.
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["sync", "--interactive"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: claude: "));
        // --interactive is the whole command: no sources, no ids.
        env.tasq()
            .args(["sync", "--interactive", "--source", "inbox"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(
                "'--interactive' cannot be used with '--source <NAME>'",
            ));
        env.tasq()
            .args(["sync", "--interactive", "3"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(
                "'--interactive' cannot be used with '[ID]...'",
            ));
    }
}

mod summary {
    use super::*;

    /// A Monday; the last working day is Friday 2026-10-02.
    const MONDAY: &str = "2026-10-05 09:00";
    /// A Wednesday.
    const WEDNESDAY: &str = "2026-10-07 09:00";

    /// A fake `claude` that records its arguments and stdin under `home`
    /// and prints a canned summary.
    fn install_claude(env: &TestEnv) {
        let home = env.home.display().to_string();
        env.fake_tool(
            "claude",
            &format!(
                "[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\necho \"$*\" > {home}/claude-args\n/bin/cat > {home}/claude-stdin\necho '- Parser done, in review ([!123](https://gitlab.example.invalid/group/project/-/merge_requests/123))'\necho 'Also:'\necho '- Waiting on the security review'"
            ),
        );
    }

    #[test]
    fn raw_groups_the_days_notes_per_task() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-10-04", "--raw"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
        // Done tasks are included and marked; weekday names look back.
        env.tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["summary", "saturday", "--raw"])
            .assert()
            .success()
            .stdout(
                "Saturday 2026-10-03\n- [4] Ship the release notes (done)\n    - created via tasks create\n    - shipped\n",
            );
        env.tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["summary", "last sunday", "--raw", "--color", "always"])
            .assert()
            .success()
            .stdout(predicate::str::starts_with(
                "\x1b[1mSunday 2026-10-04\x1b[0m\n- [1] Rewrite the tasks script in Rust\n",
            ));
    }

    #[test]
    fn defaults_to_the_last_working_day() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "--raw"])
            .assert()
            .success()
            .stdout(
                "Friday 2026-10-02\n- [3] Answer the support ticket — created via tasks create\n",
            );
    }

    #[test]
    fn nothing_logged_is_not_an_error() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-09-01"])
            .assert()
            .success()
            .stdout("Nothing logged on Tuesday 2026-09-01.\n");
        let out = env
            .tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-09-01", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["day"], "2026-09-01");
        assert_eq!(value["tasks"], serde_json::json!([]));
        assert_eq!(value["notes"], "");
        assert_eq!(value["summary"], serde_json::Value::Null);
    }

    #[test]
    fn raw_json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-10-04", "--raw", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn llm_runs_the_command_with_the_prompt_on_stdin() {
        let env = TestEnv::fixture();
        install_claude(&env);
        let out = env
            .tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "sunday", "--set", "report.summary.model=sonnet"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "## Sunday 2026-10-04\n\n- Parser done, in review ([!123](https://gitlab.example.invalid/group/project/-/merge_requests/123))\nAlso:\n- Waiting on the security review\n"
        );
        assert_eq!(stderr(&out), "");
        assert_eq!(
            std::fs::read_to_string(env.home.join("claude-args")).unwrap(),
            "-p --model sonnet\n"
        );
        let prompt = std::fs::read_to_string(env.home.join("claude-stdin")).unwrap();
        assert!(
            prompt.starts_with(
                "Below are the raw progress notes from my task tracker for Sunday 2026-10-04, "
            ),
            "{prompt}"
        );
        assert!(
            prompt.ends_with(
                "The notes:\n\n- [1] Rewrite the tasks script in Rust\n    - created via tasks create\n    - parser done\n- [6] Wait for the security review — created via tasks create\n"
            ),
            "{prompt}"
        );
        // --json carries both the notes and the distilled text.
        let out = env
            .tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "sunday", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["summarizer"], "llm");
        assert_eq!(value["header"], "Sunday 2026-10-04");
        assert_eq!(value["tasks"][1]["id"], "6");
        assert!(value["notes"].as_str().unwrap().starts_with("- [1] "));
        assert!(
            value["summary"]
                .as_str()
                .unwrap()
                .starts_with("- Parser done")
        );
        assert_eq!(
            std::fs::read_to_string(env.home.join("claude-args")).unwrap(),
            "-p\n"
        );
    }

    #[test]
    fn custom_prompt_file_and_command() {
        let env = TestEnv::fixture();
        std::fs::write(env.home.join("standup.md"), "{{date}}|{{day}}\n{{notes}}\n").unwrap();
        env.fake_tool("llm", "[ -n \"${FAKE_PROBE:-}\" ] && exit 0\n/bin/cat");
        env.write_project_config(
            "[report.summary]\ncommand = \"llm\"\nprompt_file = \"~/standup.md\"\n",
        );
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-10-03"])
            .assert()
            .success()
            .stdout(
                "## Saturday 2026-10-03\n\n2026-10-03|Saturday 2026-10-03\n- [4] Ship the release notes (done)\n    - created via tasks create\n    - shipped\n",
            );
        env.write_project_config(
            "[report.summary]\ncommand = \"llm\"\nprompt_file = \"~/missing.md\"\n",
        );
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "2026-10-03"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: report.summary.prompt_file [ROOT]"
                    .replace("[ROOT]", &env.home.display().to_string()),
            ));
    }

    #[test]
    fn raw_never_runs_the_command() {
        let env = TestEnv::fixture();
        install_claude(&env);
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .args(["summary", "--raw"])
            .assert()
            .success();
        assert!(!env.home.join("claude-args").exists());
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .env("TASQ_SUMMARIZER", "raw")
            .arg("summary")
            .assert()
            .success()
            .stdout(predicate::str::starts_with("Friday 2026-10-02\n"));
        assert!(!env.home.join("claude-args").exists());
    }

    #[test]
    fn errors() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .arg("summary")
            .assert()
            .code(1)
            .stderr("tasq: summarizer llm: claude is not on PATH (install it, use --raw for the notes themselves, or set report.summary.summarizer = \"raw\")\n");
        env.fake_tool(
            "claude",
            "[ -n \"${FAKE_PROBE:-}\" ] && exit 0\necho 'quota exceeded' >&2\nexit 2",
        );
        env.tasq()
            .env("TASQ_NOW", MONDAY)
            .arg("summary")
            .assert()
            .code(1)
            .stderr("tasq: summarizer llm failed: claude -p failed: quota exceeded\n");
        env.tasq()
            .args(["summary", "someday"])
            .assert()
            .code(1)
            .stderr("tasq: unrecognized date \"someday\" (expected YYYY-MM-DD, today, yesterday, a weekday name or last <weekday>)\n");
        env.tasq()
            .args(["summary", "tomorrow", "--raw"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: unrecognized date \"tomorrow\"",
            ));
    }
}

mod dates {
    use super::*;

    /// A Wednesday.
    const WEDNESDAY: &str = "2026-10-07 09:00";

    #[test]
    fn resolves_specs_split_across_arguments() {
        let env = TestEnv::fixture();
        let cases: &[(&[&str], &str)] = &[
            (&[], "2026-10-07 2026-10-07"),
            (&["today"], "2026-10-07 2026-10-07"),
            (&["yesterday"], "2026-10-06 2026-10-06"),
            (&["last", "week"], "2026-09-28 2026-10-02"),
            (&["Last Week"], "2026-09-28 2026-10-02"),
            (&["this", "week"], "2026-10-05 2026-10-07"),
            (&["last", "month"], "2026-09-01 2026-09-30"),
            (&["last", "7", "days"], "2026-10-01 2026-10-07"),
            (&["monday"], "2026-10-05 2026-10-05"),
            (&["last", "monday"], "2026-10-05 2026-10-05"),
            (&["2026-10-02", "2026-09-28"], "2026-09-28 2026-10-02"),
        ];
        for (args, expected) in cases {
            let out = env
                .tasq()
                .env("TASQ_NOW", WEDNESDAY)
                .arg("dates")
                .args(*args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert_eq!(stdout(&out), format!("{expected}\n"), "{args:?}");
        }
    }

    #[test]
    fn json() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["dates", "last", "week", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn errors() {
        let env = TestEnv::fixture();
        env.tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["dates", "next", "week"])
            .assert()
            .code(1)
            .stderr("tasq: unrecognized date range \"next week\" (expected a day, two days, this|last week, this|last month or last N days)\n");
        env.tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["dates", "2026-10-08"])
            .assert()
            .code(1)
            .stderr("tasq: 2026-10-08 is after today (2026-10-07); reports never look ahead\n");
        env.tasq()
            .env("TASQ_NOW", WEDNESDAY)
            .args(["dates", "2026-10-01", "2026-12-31"])
            .assert()
            .success()
            .stdout("2026-10-01 2026-10-07\n");
    }
}

mod ui {
    use super::*;

    #[test]
    fn needs_a_terminal() {
        let env = TestEnv::fixture();
        let out = env.tasq().arg("ui").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(stderr(&out), "tasq: tasq ui needs a terminal\n");
    }

    #[test]
    fn has_no_json_mode() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["ui", "--json"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stderr(&out), "tasq: tasq ui has no --json output\n");
    }

    #[test]
    fn help_lists_the_keys() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["help", "ui"]).output().unwrap();
        assert!(out.status.success());
        assert_snapshot!(stdout(&out));
    }

    #[test]
    fn bad_key_bindings_are_refused_before_the_terminal_check() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .args(["--set", "ui.keys.lunch=enter", "ui"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "tasq: ui.keys.lunch: unknown action (see `tasq ui --help`)\n"
        );

        let file = env.write_global_config("[ui.keys]\nquit = [\"q\", \"meta+q\"]\n");
        let out = env.tasq().arg("ui").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            format!(
                "tasq: {}: ui.keys.quit: \"meta+q\": unknown modifier `meta` (ctrl, alt, shift)\n",
                file.display()
            )
        );

        env.write_global_config("[ui.keys]\nsync = \"j\"\n");
        let out = env.tasq().arg("ui").output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            format!(
                "tasq: {}: ui.keys.sync: j is already bound to down\n",
                file.display()
            )
        );

        env.write_global_config("[ui.keys]\nlaunch-detached = \"alt+enter\"\n");
        let out = env.tasq().arg("ui").output().unwrap();
        assert_eq!(stderr(&out), "tasq: tasq ui needs a terminal\n");
    }
}

mod plugins {
    use super::*;

    const PROBE: &str = "[ -n \"${FAKE_PROBE:-}\" ] && exit 0";

    fn tasq_bin() -> String {
        std::fs::canonicalize(env!("CARGO_BIN_EXE_tasq"))
            .unwrap()
            .display()
            .to_string()
    }

    #[test]
    fn dispatches_to_tasq_dash_name_with_the_passthrough_env() {
        let env = TestEnv::fixture();
        env.fake_tool(
            "tasq-echo",
            &format!(
                "{PROBE}\necho \"args=$*\"\necho \"bin=$TASQ_BIN\"\n\
                 echo \"profile=${{TASQ_PROFILE-unset}} config=${{TASQ_CONFIG-unset}}\"\n\
                 echo \"set=${{TASQ_SET-unset}}\"\nexit 3"
            ),
        );
        std::fs::write(env.home.join("c.toml"), "").unwrap();
        // Dispatch happens before the config is loaded: the profile need not exist.
        let out = env
            .tasq()
            .env("TASQ_SET", "ui.no_osc8=true")
            .args([
                "--json",
                "-v",
                "--profile",
                "p",
                "--config",
                "c.toml",
                "--set",
                "a=1",
                "--set=b=2",
                "echo",
                "one",
                "--set",
                "two",
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(stderr(&out), "");
        assert_eq!(
            env.normalize(&stdout(&out)),
            format!(
                "args=one --set two\nbin={}\nprofile=p config=[ROOT]/home/c.toml\nset=ui.no_osc8=true\na=1\nb=2\n",
                env.normalize(&tasq_bin())
            )
        );
        // Nothing in effect: none of the variables is set (TASQ_BIN always is).
        let out = env.tasq().arg("echo").output().unwrap();
        assert_eq!(
            env.normalize(&stdout(&out)),
            format!(
                "args=\nbin={}\nprofile=unset config=unset\nset=unset\n",
                env.normalize(&tasq_bin())
            )
        );
    }

    #[test]
    fn plugin_wins_over_the_filter_word_but_never_over_a_builtin() {
        let env = TestEnv::fixture();
        env.fake_tool("tasq-ready", &format!("{PROBE}\necho plugin ready"));
        env.fake_tool("tasq-list", &format!("{PROBE}\necho plugin list"));
        // The ready plugin shadows the bare `tasq ready` filter...
        env.tasq()
            .arg("ready")
            .assert()
            .success()
            .stdout("plugin ready\n");
        // ...but `tasq list ready` is still the filter view.
        let out = env.tasq().args(["list", "ready"]).output().unwrap();
        assert!(stdout(&out).starts_with("ready\n"), "{}", stdout(&out));
        // A built-in name is never dispatched.
        let out = env.tasq().arg("list").output().unwrap();
        assert!(
            stdout(&out).starts_with("IN PROGRESS\n"),
            "{}",
            stdout(&out)
        );
        // Without an executable the word is a tag filter as before.
        env.tasq()
            .arg("nope-plugin")
            .assert()
            .success()
            .stdout("No open todos tagged #nope-plugin.\n");
        // A plugin file that is not executable does not count.
        std::fs::write(env.bin.join("tasq-plain"), "#!/bin/sh\necho ran\n").unwrap();
        env.tasq()
            .arg("plain")
            .assert()
            .success()
            .stdout("No open todos tagged #plain.\n");
    }

    #[test]
    fn list_shows_plugins_and_hooks() {
        let env = TestEnv::fixture();
        env.fake_tool("tasq-tlogs", PROBE);
        env.fake_tool("tasq-zed", PROBE);
        env.fake_tool("unrelated", PROBE);
        env.write_project_config(
            "[hooks]\npost-create = [\"tasq-notify\", \"~/bin/log-it --quiet\"]\npre-launch = [\"check-vpn\"]\n",
        );
        let out = env.tasq().args(["plugins", "list"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));

        let out = env
            .tasq()
            .args(["--json", "plugins", "list"])
            .output()
            .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(doc["schema"], 1);
        assert_eq!(doc["plugins"][0]["name"], "tlogs");
        assert_eq!(
            env.normalize(doc["plugins"][0]["path"].as_str().unwrap()),
            "[ROOT]/bin/tasq-tlogs"
        );
        assert_eq!(doc["plugins"][1]["name"], "zed");
        assert_eq!(doc["plugins"].as_array().unwrap().len(), 2);
        assert_eq!(doc["hooks"]["post-create"][1], "~/bin/log-it --quiet");
        assert_eq!(doc["hooks"]["post-done"], serde_json::json!([]));
        assert_eq!(doc["hooks"]["pre-launch"][0], "check-vpn");

        let env = TestEnv::fixture();
        env.tasq().args(["plugins", "list"]).assert().success().stdout(
            "Plugins on PATH (tasq-<name>):\n  (none)\n\nHooks ([hooks] in the config):\n  (none)\n",
        );
    }

    #[test]
    fn hooks_get_the_document_on_stdin_and_the_event_in_the_environment() {
        let env = TestEnv::fixture();
        let log = env.home.join("hooks.log");
        env.fake_tool(
            "hook-log",
            &format!(
                "{PROBE}\n{{ echo \"hook=$TASQ_HOOK id=$TASQ_TASK_ID bin=$TASQ_BIN\"; /bin/cat; echo; }} >> '{}'",
                log.display()
            ),
        );
        env.fake_tool("claude", &format!("{PROBE}\necho \"claude $TASQ_TASK_ID\""));
        env.write_project_config(
            "[hooks]\npost-create = [\"hook-log\"]\npost-done = [\"hook-log\"]\npre-launch = [\"hook-log\"]\n",
        );
        let read_log = || env.normalize(&std::fs::read_to_string(&log).unwrap_or_default());

        let out = env
            .tasq_wt()
            .env("TASQ_NOW", "2026-10-07 09:30")
            .args(["create", "Hooked", "--tag", "x"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        let logged = read_log();
        let bin = env.normalize(&tasq_bin());
        assert!(
            logged.starts_with(&format!("hook=post-create id=8 bin={bin}\n{{\"hook\":\"post-create\",\"schema\":1,\"task\":{{")),
            "{logged}"
        );
        assert!(logged.contains("\"title\":\"Hooked\""), "{logged}");

        std::fs::remove_file(&log).unwrap();
        env.tasq_wt()
            .args(["done", "8", "shipped"])
            .assert()
            .success()
            .stderr("");
        let logged = read_log();
        assert!(logged.starts_with("hook=post-done id=8 "), "{logged}");
        assert!(logged.contains("\"done\":true"), "{logged}");
        assert!(logged.contains("\"note\":\"shipped\""), "{logged}");

        // A dry run lists the hook as a skipped step and runs nothing.
        std::fs::remove_file(&log).unwrap();
        let out = env
            .tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "3", "--dry-run", "--launcher", "shell"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stdout(&out).contains(
                "Launcher: shell (dry run)\npre-launch hook (skipped: dry run): hook-log\n"
            ),
            "{}",
            stdout(&out)
        );
        assert!(!log.exists());

        // The real launch runs it with the resolved directory and launcher.
        let out = env
            .tasq_wt()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["pick", "3"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(stdout(&out).ends_with("claude 3\n"), "{}", stdout(&out));
        let logged = read_log();
        assert!(logged.starts_with("hook=pre-launch id=3 "), "{logged}");
        assert!(logged.contains("\"in_worktree\":false"), "{logged}");
        assert!(logged.contains("\"launcher\":\"claude\""), "{logged}");
        assert!(logged.contains("\"workdir\":\"[ROOT]/home\""), "{logged}");
    }

    #[test]
    fn failing_hooks_warn_or_abort() {
        let env = TestEnv::fixture();
        env.fake_tool("hook-fail", &format!("{PROBE}\necho nope >&2\nexit 4"));
        env.fake_tool("claude", &format!("{PROBE}\necho \"claude $TASQ_TASK_ID\""));
        env.write_project_config(
            "[hooks]\npost-create = [\"hook-fail\", \"missing-hook --flag\"]\npre-launch = [\"hook-fail\"]\n",
        );
        // post-*: the task is created, each failure is a warning.
        let out = env
            .tasq()
            .args(["create", "Still created"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stdout(&out).starts_with("[8] created: Still created"),
            "{}",
            stdout(&out)
        );
        let err = stderr(&out);
        assert!(
            err.starts_with("tasq: warning: post-create hook \"hook-fail\" failed: exit status 4: nope\ntasq: warning: post-create hook \"missing-hook --flag\" failed: could not run missing-hook: "),
            "{err}"
        );
        // pre-launch: the launch is aborted with the hook's message.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("TASQ_LAUNCH_ENV", "inherit")
            .args(["pick", "3"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "tasq: pre-launch hook \"hook-fail\" failed: exit status 4: nope\n"
        );
        assert!(!stdout(&out).contains("claude 3"), "{}", stdout(&out));
        // A bad command line is an error, not a warning: it is a config mistake.
        env.write_project_config("[hooks]\npost-create = [\"unterminated 'quote\"]\n");
        let out = env.tasq().args(["create", "Bad hook"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq: hook command \"unterminated 'quote\": "),
            "{}",
            stderr(&out)
        );
    }

    /// The reference plugin under `examples/plugins` runs through dispatch.
    /// Needs `bash` and `jq`; skipped (with a note) when either is missing.
    #[test]
    fn example_tlogs_plugin_runs_through_dispatch() {
        let env = TestEnv::fixture();
        for tool in ["bash", "jq"] {
            let Some(path) = support::find_on_path(tool) else {
                eprintln!("skipping: {tool} not installed");
                return;
            };
            std::os::unix::fs::symlink(path, env.bin.join(tool)).unwrap();
        }
        let examples = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/plugins")
            .canonicalize()
            .unwrap();
        let path = std::env::join_paths([env.bin.clone(), examples]).unwrap();
        let out = env
            .tasq()
            .env("PATH", &path)
            .args(["tlogs", "--propose", "2026-10-02", "2026-10-04"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "2026-10-02 (Friday)\n  [3] Answer the support ticket  8h  (1 note)\n"
        );
        let out = env
            .tasq()
            .env("PATH", &path)
            .env("TLOGS_HOURS", "6")
            .args([
                "--set",
                "store.notebook=home",
                "tlogs",
                "--propose",
                "--json",
                "2026-10-02",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(doc["schema"], 1);
        assert_eq!(doc["days"][0]["tasks"][0]["hours"], 6);
        // The default mode launches `claude "/time-logs FROM TO"` from
        // `work.default_project`; --dry-run prints that instead. No direnv on
        // this PATH, so the command is bare.
        let project = env.home.to_str().unwrap();
        let out = env
            .tasq()
            .env("PATH", &path)
            .args([
                "--set",
                &format!("work.default_project={project}"),
                "tlogs",
                "--dry-run",
                "2026-10-02",
                "2026-10-03",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            stdout(&out),
            format!(
                "cd {project} && TASKS_NB_NOTEBOOK=home claude /time-logs\\ 2026-10-02\\ 2026-10-03 \n"
            )
        );
        // Without a directory it refuses, naming the key to set.
        let out = env
            .tasq()
            .env("PATH", &path)
            .args(["tlogs", "--dry-run", "2026-10-02"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).contains("work.default_project is not set"),
            "{}",
            stderr(&out)
        );
        let out = env
            .tasq()
            .env("PATH", &path)
            .args(["plugins", "list"])
            .output()
            .unwrap();
        assert!(stdout(&out).contains("  tlogs  "), "{}", stdout(&out));
    }
}

/// ADR 0022: with the default configuration nothing mentions worktrees.
mod worktrees_disabled {
    use super::*;

    fn mentions_worktrees(text: &str) -> bool {
        text.to_lowercase().contains("worktree")
    }

    #[test]
    fn help_completions_and_json_say_nothing_about_worktrees() {
        let env = TestEnv::fixture();
        for args in [
            vec!["--help"],
            vec!["create", "--help"],
            vec!["help", "pick"],
            vec!["view", "1", "--json"],
            vec!["list", "--all", "--json"],
            vec!["completions", "zsh"],
            vec!["completions", "bash"],
        ] {
            let out = env.tasq().args(&args).output().unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert!(
                !mentions_worktrees(&stdout(&out)),
                "{args:?}: {}",
                stdout(&out)
            );
        }
        // Disabled completions still complete the rest of the commands.
        let zsh = env.tasq().args(["completions", "zsh"]).output().unwrap();
        for expected in [
            "#compdef tasq",
            "'--version",
            "(create)",
            "'--project=",
            "(list)",
        ] {
            assert!(stdout(&zsh).contains(expected), "{expected}");
        }
        // Enabled, the same commands show them.
        let help = env.tasq_wt().arg("--help").output().unwrap();
        assert!(stdout(&help).contains("  worktree "), "{}", stdout(&help));
        let zsh = env.tasq_wt().args(["completions", "zsh"]).output().unwrap();
        assert!(stdout(&zsh).contains("worktree"));
        let view = env
            .tasq_wt()
            .args(["view", "1", "--json"])
            .output()
            .unwrap();
        assert!(stdout(&view).contains("\"worktrees\""), "{}", stdout(&view));
        // `--set` and `--config` before parsing decide too.
        let set = env
            .tasq()
            .args(["--set", "worktree.enabled=true", "--help"])
            .output()
            .unwrap();
        assert!(stdout(&set).contains("  worktree "), "{}", stdout(&set));
    }

    #[test]
    fn the_worktree_command_is_refused() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr("tasq: worktrees are disabled; set worktree.enabled = true (docs/config.md#worktrees)\n");
    }

    #[test]
    fn apply_without_the_key_keeps_the_stored_worktrees() {
        let env = TestEnv::fixture();
        let before = env.read_task("20260901090000.todo.md");
        let json = env.tasq().args(["view", "1", "--json"]).output().unwrap();
        assert!(!mentions_worktrees(&stdout(&json)));
        env.tasq()
            .arg("apply")
            .write_stdin(json.stdout)
            .assert()
            .success()
            .stdout("[1] applied\n");
        assert_eq!(env.read_task("20260901090000.todo.md"), before);
        let on = env
            .tasq_wt()
            .args(["view", "1", "--json"])
            .output()
            .unwrap();
        assert!(
            stdout(&on).contains("tasks-wt/feature-a"),
            "{}",
            stdout(&on)
        );
    }

    #[test]
    fn launch_ignores_tracked_worktrees() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "1", "--dry-run", "--launcher", "claude"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let text = format!("{}{}", stdout(&out), stderr(&out));
        // The task file is quoted verbatim in the prompt; outside it the
        // launch says nothing about worktrees.
        let without_file: String = text
            .lines()
            .filter(|l| !l.contains("tasks-wt") && *l != "## Worktrees")
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!mentions_worktrees(&without_file), "{text}");
        let json = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "1", "--dry-run", "--launcher", "shell", "--json"])
            .output()
            .unwrap();
        assert!(!mentions_worktrees(&stdout(&json)), "{}", stdout(&json));
    }
}

/// Releasing worktrees at close (plan 4.2 step 3, WT-206): `tasq done`
/// and `tasq worktree release`.
mod worktree_release {
    use super::*;

    const TASK: &str = "20260902100000.todo.md";

    /// Task 3 in a repository at `<home>/proj` (`.tasq.toml` from
    /// `repo_config`, ignored), with worktree `b` declared and created at
    /// `<home>/proj-b`.
    fn released_project(
        env: &TestEnv,
        repo_config: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let project = env.home.join("proj");
        env.git_repo(&project);
        std::fs::write(project.join(".gitignore"), ".tasq.toml\n").unwrap();
        env.git(&project, &["add", ".gitignore"]);
        env.git(&project, &["commit", "-q", "-m", "ignore"]);
        std::fs::write(project.join(".tasq.toml"), repo_config).unwrap();
        env.tasq_wt()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "3", "b"])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "up", "3", "--yes"])
            .assert()
            .success();
        let wt = env.home.join("proj-b");
        assert!(wt.is_dir());
        (project, wt)
    }

    /// Commits a file in `dir`.
    fn commit(env: &TestEnv, dir: &Path, file: &str) {
        std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
        env.git(dir, &["add", file]);
        env.git(dir, &["commit", "-q", "-m", file]);
    }

    fn worktrees_section(env: &TestEnv) -> String {
        let text = env.read_task(TASK);
        let start = text.find("## Worktrees").expect("a Worktrees section");
        text[start..].to_owned()
    }

    fn branches(env: &TestEnv, project: &Path) -> String {
        env.git(project, &["branch", "--format=%(refname:short)"])
    }

    fn run(env: &TestEnv, args: &[&str]) -> (String, String) {
        let out = env.tasq_wt().args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        (env.normalize(&stdout(&out)), env.normalize(&stderr(&out)))
    }

    #[test]
    fn done_removes_a_clean_merged_worktree_and_its_branch() {
        let env = TestEnv::fixture();
        let (project, wt) = released_project(&env, "[worktree]\ncopies = [\"local.env\"]\n");
        commit(&env, &wt, "feature");
        env.git(&project, &["merge", "-q", "--no-ff", "-m", "merge b", "b"]);
        // Copied by provisioning, untracked in the worktree: not a change.
        assert!(!wt.join("local.env").exists(), "nothing to copy yet");
        std::fs::write(wt.join("local.env"), "X=1\n").unwrap();
        let before = worktrees_section(&env);
        let (out, err) = run(&env, &["done", "3"]);
        assert_eq!(
            out,
            "[3] done: Answer the support ticket\n\
             [3] removed worktree [ROOT]/home/proj-b\n\
             [3] deleted branch b (merged into main)\n"
        );
        assert_eq!(err, "");
        assert!(!wt.exists());
        assert_eq!(branches(&env, &project), "main\n");
        assert_eq!(
            worktrees_section(&env),
            before,
            "the entry stays byte for byte"
        );
        // Reopened and launched again: the branch is gone, so it starts
        // fresh from the base.
        env.tasq_wt().args(["reopen", "3"]).assert().success();
        let (out, _) = run(&env, &["worktree", "up", "3"]);
        assert_eq!(
            out,
            "[3] created worktree [ROOT]/home/proj-b on b (new from HEAD)\n"
        );
    }

    #[test]
    fn done_on_a_done_task_is_refused_before_the_release() {
        let env = TestEnv::fixture();
        let (project, wt) = released_project(&env, "");
        commit(&env, &wt, "feature");
        std::fs::write(wt.join("README"), "edited\n").unwrap();
        let (_, err) = run(&env, &["done", "3"]);
        assert!(err.contains("kept worktree"), "{err}");
        // Clean and merged now, so a release would remove both.
        env.git(&wt, &["checkout", "--", "README"]);
        env.git(&project, &["merge", "-q", "--no-ff", "-m", "merge b", "b"]);
        let before = env.read_task(TASK);
        env.tasq_wt()
            .args(["done", "3"])
            .assert()
            .code(1)
            .stdout("")
            .stderr("tasq: task 3 is already done\n");
        assert!(wt.is_dir(), "a refused done releases nothing");
        assert_eq!(branches(&env, &project), "b\nmain\n");
        assert_eq!(env.read_task(TASK), before);
    }

    #[test]
    fn dirty_and_unmerged_are_kept_with_one_warning_each() {
        let env = TestEnv::fixture();
        let (project, wt) = released_project(&env, "");
        commit(&env, &wt, "unmerged");
        std::fs::write(wt.join("README"), "edited\n").unwrap();
        let (out, err) = run(&env, &["done", "3"]);
        assert_eq!(out, "[3] done: Answer the support ticket\n");
        assert_eq!(
            err,
            "tasq: warning: kept worktree [ROOT]/home/proj-b: it has uncommitted or untracked \
             changes; tasq worktree release 3 --force removes it\n"
        );
        assert!(wt.is_dir());
        // Clean again: the directory goes, the unmerged branch stays.
        env.git(&wt, &["checkout", "--", "README"]);
        let (out, err) = run(&env, &["worktree", "release", "3", "--dry-run"]);
        assert_eq!(out, "[3] would remove worktree [ROOT]/home/proj-b\n");
        assert_eq!(
            err,
            "tasq: warning: kept branch b: it is neither merged into the default branch nor \
             pushed; tasq worktree release 3 --force deletes it\n"
        );
        assert!(wt.is_dir(), "a dry run changes nothing");
        let (out, err) = run(&env, &["worktree", "release", "3"]);
        assert_eq!(out, "[3] removed worktree [ROOT]/home/proj-b\n");
        assert!(err.contains("kept branch b"), "{err}");
        assert!(!wt.exists());
        assert_eq!(branches(&env, &project), "b\nmain\n");
        // Launching again checks the kept branch out.
        let (out, _) = run(&env, &["worktree", "up", "3"]);
        assert_eq!(
            out,
            "[3] created worktree [ROOT]/home/proj-b on b (the local branch)\n"
        );
        assert!(wt.join("unmerged").is_file());
        // --force removes a dirty worktree and deletes the branch.
        std::fs::write(wt.join("scratch"), "x\n").unwrap();
        let (out, err) = run(&env, &["worktree", "release", "3", "--force"]);
        assert_eq!(
            out,
            "[3] removed worktree [ROOT]/home/proj-b\n[3] deleted branch b (forced)\n"
        );
        assert_eq!(err, "");
        assert_eq!(branches(&env, &project), "main\n");
        let (out, _) = run(&env, &["worktree", "release", "3"]);
        assert_eq!(out, "[3] nothing released\n");
    }

    #[test]
    fn locked_in_use_and_main_are_kept() {
        let env = TestEnv::fixture();
        let (project, wt) = released_project(&env, "");
        env.git(&project, &["worktree", "lock", wt.to_str().unwrap()]);
        let (_, err) = run(&env, &["worktree", "release", "3"]);
        assert_eq!(
            err,
            "tasq: warning: kept worktree [ROOT]/home/proj-b: it is locked; git worktree unlock \
             [ROOT]/home/proj-b then tasq worktree release 3 removes it\n"
        );
        env.git(&project, &["worktree", "unlock", wt.to_str().unwrap()]);
        let out = env
            .tasq_wt()
            .current_dir(wt.join("."))
            .args(["worktree", "release", "3", "--force"])
            .output()
            .unwrap();
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: kept worktree [ROOT]/home/proj-b: the current directory is inside it; \
             tasq worktree release 3 from another directory removes it\n"
        );
        assert!(wt.is_dir());
        // The main checkout, tracked by hand, is kept without a word; a
        // directory git does not list is named.
        env.tasq_wt()
            .args(["worktree", "3", project.to_str().unwrap()])
            .assert()
            .success();
        let stray = env.home.join("stray");
        std::fs::create_dir(&stray).unwrap();
        env.tasq_wt()
            .args(["worktree", "3", stray.to_str().unwrap()])
            .assert()
            .success();
        let (out, err) = run(&env, &["worktree", "release", "3"]);
        assert_eq!(
            out,
            "[3] removed worktree [ROOT]/home/proj-b\n[3] deleted branch b (same commit as main)\n"
        );
        assert_eq!(
            err,
            "tasq: warning: kept worktree [ROOT]/home/stray: git does not list it as a worktree \
             of the repository; tasq does not remove it, delete it by hand if it is not needed\n"
        );
        assert!(project.is_dir() && stray.is_dir());
    }

    #[test]
    fn remove_hooks_run_around_the_removal_and_pre_remove_can_keep_it() {
        let env = TestEnv::fixture();
        env.fake_tool(
            "hook",
            "echo \"$1|$TASQ_BRANCH|$TASQ_WORKTREE|$(pwd)\" >> \"$HOME/hooks.log\"",
        );
        env.fake_tool("fail", "echo nope >&2; exit 3");
        let config = "[worktree.hooks]\n\
            pre-remove = [\"hook pre\"]\n\
            post-remove = [\"hook post\", \"fail\"]\n";
        let (project, wt) = released_project(&env, config);
        let log = || {
            env.normalize(&std::fs::read_to_string(env.home.join("hooks.log")).unwrap_or_default())
        };
        // Not approved, no terminal: skipped, the worktree still goes.
        let (out, err) = run(&env, &["worktree", "release", "3", "--dry-run"]);
        assert_eq!(
            out,
            "[3] would remove worktree [ROOT]/home/proj-b\n  \
             skip pre-remove hook (not approved): hook pre\n  \
             skip post-remove hook (not approved): hook post\n  \
             skip post-remove hook (not approved): fail\n\
             [3] would delete branch b (same commit as main)\n"
        );
        assert_eq!(err, "");
        let (_, err) = run(&env, &["done", "3", "--yes"]);
        assert_eq!(
            err,
            "tasq: warning: post-remove hook \"fail\" failed: exit status 3: nope\n"
        );
        assert_eq!(
            log(),
            "pre|b|[ROOT]/home/proj-b|[ROOT]/home/proj-b\n\
             post|b|[ROOT]/home/proj-b|[ROOT]/home/proj\n"
        );
        assert!(!wt.exists());
        // A failing pre-remove keeps the worktree and its branch.
        env.tasq_wt().args(["reopen", "3"]).assert().success();
        env.tasq_wt()
            .args(["worktree", "up", "3", "--yes"])
            .assert()
            .success();
        std::fs::write(
            project.join(".tasq.toml"),
            "[worktree.hooks]\npre-remove = [\"fail\"]\n",
        )
        .unwrap();
        let (out, err) = run(&env, &["worktree", "release", "3", "--yes"]);
        assert_eq!(out, "[3] nothing released\n");
        assert_eq!(
            err,
            "tasq: warning: kept worktree [ROOT]/home/proj-b: pre-remove hook \"fail\" failed: \
             exit status 3: nope\n"
        );
        assert!(wt.is_dir());
        assert_eq!(branches(&env, &project), "b\nmain\n");
    }

    /// `tasq worktree prune` (WT-301).
    #[test]
    fn prune_releases_what_closes_left_behind() {
        let env = TestEnv::fixture();
        // Task 1 of the fixture declares a worktree in a real project of
        // the author's; prune must never reach it (see worktree_nav).
        let full = "20260901090000.todo.md";
        let text = env.read_task(full).replace(
            "## Worktrees\n\n- /home/pau/code/tasks-wt/feature-a (`feature-a`)\n\n",
            "",
        );
        assert!(!text.contains("Worktrees"), "the fixture changed: {text}");
        env.write_task(full, &text);
        // Task 3 closed with on_done = "keep": its clean worktree stays.
        let (project, wt) = released_project(&env, "[worktree]\non_done = \"keep\"\n");
        run(&env, &["done", "3"]);
        assert!(wt.is_dir());
        // Task 4 (done) with a dirty worktree, task 6 (open) with a clean one.
        for (id, branch) in [("4", "e"), ("6", "d")] {
            for args in [
                vec!["project", id, project.to_str().unwrap()],
                vec!["worktree", id, branch],
                vec!["worktree", "up", id],
            ] {
                env.tasq_wt().args(args).assert().success();
            }
        }
        let dirty = env.home.join("proj-e");
        let open = env.home.join("proj-d");
        std::fs::write(dirty.join("scratch"), "x\n").unwrap();
        // A worktree deleted by hand: git still lists it.
        let stray = env.home.join("proj-x");
        env.git(
            &project,
            &["worktree", "add", "-q", "-b", "x", stray.to_str().unwrap()],
        );
        std::fs::remove_dir_all(&stray).unwrap();
        let prune = |dir: &Path, args: &[&str]| {
            let out = env
                .tasq_wt()
                .current_dir(dir)
                .args(["worktree", "prune"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            (env.normalize(&stdout(&out)), env.normalize(&stderr(&out)))
        };
        let kept = "tasq: warning: kept worktree [ROOT]/home/proj-e: it has uncommitted or \
                    untracked changes; tasq worktree release 4 --force removes it\n";
        let forgotten = "missing worktrees in [ROOT]/home/proj:\n  \
                         Removing worktrees/proj-x: gitdir file points to non-existent location\n";
        // Another repository is out of scope without --all.
        let other = env.home.join("other");
        env.git_repo(&other);
        assert_eq!(
            prune(&other, &["--dry-run"]),
            ("nothing to prune\n".to_owned(), String::new())
        );
        let (out, err) = prune(&other, &["--dry-run", "--all"]);
        assert_eq!(
            out,
            format!(
                "would forget {forgotten}\
                 [3] would remove worktree [ROOT]/home/proj-b\n\
                 [3] would delete branch b (same commit as main)\n"
            )
        );
        assert_eq!(err, kept);
        assert!(wt.is_dir(), "a dry run changes nothing");
        let (json, _) = prune(&project, &["--dry-run", "--json"]);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({
                "schema": 1, "dry_run": true,
                "released": [{"task": "3", "lines": [
                    "would remove worktree [ROOT]/home/proj-b",
                    "would delete branch b (same commit as main)",
                ]}],
                "forgotten": [{"repo": "[ROOT]/home/proj", "lines": [
                    "Removing worktrees/proj-x: gitdir file points to non-existent location",
                ]}],
            })
        );
        let (out, err) = prune(&project, &[]);
        assert_eq!(
            out,
            format!(
                "forgot {forgotten}\
                 [3] removed worktree [ROOT]/home/proj-b\n\
                 [3] deleted branch b (same commit as main)\n"
            )
        );
        assert_eq!(err, kept);
        assert!(!wt.exists());
        assert!(dirty.is_dir() && open.is_dir());
        assert_eq!(branches(&env, &project), "d\ne\nmain\nx\n");
        assert_eq!(
            prune(&project, &[]),
            ("nothing to prune\n".to_owned(), kept.to_owned())
        );
        env.tasq()
            .args(["worktree", "prune"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
    }

    #[test]
    fn on_done_keep_and_errors() {
        let env = TestEnv::fixture();
        let (_, wt) = released_project(&env, "[worktree]\non_done = \"keep\"\n");
        let (out, err) = run(&env, &["done", "3"]);
        assert_eq!(
            (out.as_str(), err.as_str()),
            ("[3] done: Answer the support ticket\n", "")
        );
        assert!(wt.is_dir());
        env.tasq()
            .args(["worktree", "release", "3"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
        // A project that is not a repository: closed anyway, with a warning.
        let plain = env.home.join("plain");
        std::fs::create_dir(&plain).unwrap();
        env.tasq_wt()
            .args(["project", "7", plain.to_str().unwrap()])
            .assert()
            .success();
        env.tasq_wt()
            .args(["worktree", "7", wt.to_str().unwrap()])
            .assert()
            .success();
        let out = env.tasq_wt().args(["done", "7"]).output().unwrap();
        assert!(out.status.success());
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: worktrees not released: [ROOT]/home/plain is not in a git repository\n"
        );
    }
}

/// `tasq worktree ls`, `switch`, `path` and `shell-init` (WT-207).
mod worktree_nav {
    use super::*;

    /// A repository at `<home>/proj` for tasks 3, 4 and 6 (4 is done):
    /// task 3 has `b` created and `c` declared, task 6 `d` declared, task
    /// 4 `e` declared and never created.
    fn workspace(env: &TestEnv) -> std::path::PathBuf {
        // Task 1 of the fixture declares a worktree under the author's real
        // home; a test that lists or creates every task's worktrees must
        // never see it.
        let full = "20260901090000.todo.md";
        let text = env.read_task(full).replace(
            "## Worktrees\n\n- /home/pau/code/tasks-wt/feature-a (`feature-a`)\n\n",
            "",
        );
        assert!(!text.contains("Worktrees"), "the fixture changed: {text}");
        env.write_task(full, &text);
        let project = env.home.join("proj");
        env.git_repo(&project);
        std::fs::write(
            project.join(".tasq.toml"),
            "[worktree]\ncopies = [\".tasq.toml\"]\n",
        )
        .unwrap();
        std::fs::write(project.join(".gitignore"), "\n").unwrap();
        for (id, branch) in [("3", "b"), ("3", "c"), ("6", "d"), ("4", "e")] {
            env.tasq_wt()
                .args(["project", id, project.to_str().unwrap()])
                .assert()
                .success();
            env.tasq_wt()
                .args(["worktree", id, branch])
                .assert()
                .success();
        }
        env.tasq_wt()
            .args(["worktree", "up", "3"])
            .assert()
            .success();
        project
    }

    fn run_in(env: &TestEnv, dir: &Path, args: &[&str]) -> (String, String) {
        let out = env.tasq_wt().current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        (env.normalize(&stdout(&out)), env.normalize(&stderr(&out)))
    }

    fn fails_in(env: &TestEnv, dir: &Path, args: &[&str]) -> String {
        let out = env.tasq_wt().current_dir(dir).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", stdout(&out));
        env.normalize(&stderr(&out))
    }

    #[test]
    fn ls_shows_every_declared_worktree_with_its_state() {
        let env = TestEnv::fixture();
        let project = workspace(&env);
        let wt = env.home.join("proj-b");
        // The copied .tasq.toml is provisioning's, not a change.
        assert!(wt.join(".tasq.toml").is_file());
        std::fs::write(wt.join("scratch"), "x\n").unwrap();
        let (out, _) = run_in(&env, &env.home, &["worktree", "ls"]);
        assert_eq!(
            out,
            "[3]  ready    b  1 changed    [ROOT]/home/proj-b\n\
             [3]  ready    c  not created  [ROOT]/home/proj-c\n\
             [6]  waiting  d  not created  [ROOT]/home/proj-d\n"
        );
        let (only, _) = run_in(&env, &env.home, &["worktree", "ls", "4"]);
        assert_eq!(
            only, "[4]  done  e  not created  [ROOT]/home/proj-e\n",
            "a closed task's own list shows everything"
        );
        // Inside the repository, its tasks; in another, none (unless --all).
        let (inside, _) = run_in(&env, &wt, &["worktree", "ls"]);
        assert_eq!(inside, out);
        let other = env.home.join("other");
        env.git_repo(&other);
        let (none, _) = run_in(&env, &other, &["worktree", "ls"]);
        assert_eq!(none, "no declared worktrees\n");
        let (all, _) = run_in(&env, &other, &["worktree", "ls", "--all"]);
        assert_eq!(all, out);
        let (json, _) = run_in(&env, &project, &["worktree", "ls", "3", "--json"]);
        assert_eq!(
            json,
            "{\n  \"schema\": 1,\n  \"worktrees\": [\n    {\n      \"branch\": \"b\",\n      \
             \"changes\": [\n        \"scratch\"\n      ],\n      \"done\": false,\n      \
             \"exists\": true,\n      \"path\": \"[ROOT]/home/proj-b\",\n      \
             \"status\": \"ready\",\n      \"task\": \"3\",\n      \
             \"title\": \"Answer the support ticket\"\n    },\n    {\n      \
             \"branch\": \"c\",\n      \"changes\": null,\n      \"done\": false,\n      \
             \"exists\": false,\n      \"path\": \"[ROOT]/home/proj-c\",\n      \
             \"status\": \"ready\",\n      \"task\": \"3\",\n      \
             \"title\": \"Answer the support ticket\"\n    }\n  ]\n}\n"
        );
        env.tasq()
            .args(["worktree", "ls"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
    }

    #[test]
    fn path_resolves_targets_and_creates_nothing() {
        let env = TestEnv::fixture();
        let project = workspace(&env);
        let wt = env.home.join("proj-b");
        let path = |dir: &Path, target: &str| run_in(&env, dir, &["worktree", "path", target]).0;
        assert_eq!(path(&env.home, "task:3"), "[ROOT]/home/proj-b\n");
        assert_eq!(
            path(&project, "b"),
            "[ROOT]/home/proj-b\n",
            "a branch git lists"
        );
        assert_eq!(
            path(&env.home, "b"),
            "[ROOT]/home/proj-b\n",
            "a branch a task declares"
        );
        assert_eq!(path(&wt, "^"), "[ROOT]/home/proj\n");
        assert_eq!(
            path(&env.home, wt.to_str().unwrap()),
            "[ROOT]/home/proj-b\n"
        );
        assert_eq!(path(&project, "main"), "[ROOT]/home/proj\n");
        assert_eq!(
            run_in(&env, &env.home, &["worktree", "path", "task:3", "--json"]).0,
            "{\n  \"path\": \"[ROOT]/home/proj-b\",\n  \"schema\": 1\n}\n"
        );
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "task:6"]),
            "tasq: task 6 has no worktree yet; tasq worktree switch task:6 creates it\n"
        );
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "d"]),
            "tasq: task 6 has no worktree yet; tasq worktree switch task:6 creates it\n"
        );
        assert!(!env.home.join("proj-d").exists());
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "nope"]),
            "tasq: no worktree on branch nope\n"
        );
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "^"]),
            "tasq: not in a git repository: [ROOT]/home\n"
        );
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "-"]),
            "tasq: no previous worktree: nothing was switched from yet\n"
        );
        // A state file that cannot be read is an error, not "nothing yet".
        let state = env.home.join(".local/state/tasq/previous-worktree");
        std::fs::create_dir_all(&state).unwrap();
        assert!(
            fails_in(&env, &env.home, &["worktree", "path", "-"]).starts_with(
                "tasq: [ROOT]/home/.local/state/tasq/previous-worktree: Is a directory"
            )
        );
        std::fs::remove_dir(&state).unwrap();
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "./gone"]),
            "tasq: directory not found: ./gone\n"
        );
        assert_eq!(
            fails_in(&env, &env.home, &["worktree", "path", "task:"]),
            "tasq: task: needs an id (task:<id>)\n"
        );
    }

    #[test]
    fn switch_hands_the_directory_to_the_shell_and_remembers_the_previous() {
        let env = TestEnv::fixture();
        let project = workspace(&env);
        let cd_file = env.home.join("cd");
        // Without the shell function: the path, and how to get the function.
        let (out, err) = run_in(&env, &project.join("."), &["worktree", "switch", "b"]);
        assert_eq!(out, "[ROOT]/home/proj-b\n");
        assert!(err.contains("shell-init"), "{err}");
        // With it: the file gets the path, stdout stays quiet.
        let out = env
            .tasq_wt()
            .current_dir(env.home.join("proj-b"))
            .env("TASQ_CD_FILE", &cd_file)
            .args(["worktree", "switch", "task:6"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[6] created worktree [ROOT]/home/proj-d on d (new from HEAD)\n",
            "a task's worktree is created first"
        );
        assert_eq!(
            env.normalize(&std::fs::read_to_string(&cd_file).unwrap()),
            "[ROOT]/home/proj-d"
        );
        // `-` is where the last switch came from.
        let (back, _) = run_in(&env, &env.home, &["worktree", "path", "-"]);
        assert_eq!(back, "[ROOT]/home/proj-b\n");
        let (json, _) = run_in(&env, &env.home, &["worktree", "switch", "-", "--json"]);
        assert_eq!(
            json,
            "{\n  \"path\": \"[ROOT]/home/proj-b\",\n  \"schema\": 1\n}\n"
        );
        let (back, _) = run_in(&env, &env.home, &["worktree", "path", "-"]);
        assert_eq!(
            back, "[ROOT]/home\n",
            "home is not in a repository: the directory itself"
        );
    }

    #[test]
    fn switch_without_a_target_offers_a_list() {
        let env = TestEnv::fixture();
        let project = workspace(&env);
        // No fzf: a numbered list on stderr, the number on stdin.
        let out = env
            .tasq_wt()
            .current_dir(&project)
            .args(["worktree", "switch"])
            .write_stdin("2\n")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(env.normalize(&stdout(&out)), "[ROOT]/home/proj-b\n");
        // Task 3's `c` is not offered: `b` exists, so `c` would not be made.
        assert_eq!(
            env.normalize(&stderr(&out)),
            " 1  ^  main checkout  [ROOT]/home/proj\n \
             2  [3]  ready  b  clean  [ROOT]/home/proj-b\n \
             3  [6]  waiting  d  not created  [ROOT]/home/proj-d\n\
             switch to [1-3]: tasq: warning: the shell function changes directory: add eval \
             \"$(tasq worktree shell-init zsh)\" (or bash, fish) to the shell's rc file\n"
        );
        // fzf, when installed, picks instead (here: always the third line),
        // and a task's missing worktree is created.
        env.fake_tool(
            "fzf",
            "n=0\nwhile IFS= read -r line || [ -n \"$line\" ]; do\n\
             n=$((n+1)); printf '%s\\n' \"$line\" >> \"$HOME/fzf.in\"\n\
             [ $n = 3 ] && picked=$line\ndone\nprintf '%s\\n' \"$picked\"",
        );
        let out = env
            .tasq_wt()
            .current_dir(&project)
            .args(["worktree", "switch"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[6] created worktree [ROOT]/home/proj-d on d (new from HEAD)\n\
             [ROOT]/home/proj-d\n"
        );
        assert_eq!(
            env.normalize(&std::fs::read_to_string(env.home.join("fzf.in")).unwrap()),
            " 1  ^  main checkout  [ROOT]/home/proj\n \
             2  [3]  ready  b  clean  [ROOT]/home/proj-b\n \
             3  [6]  waiting  d  not created  [ROOT]/home/proj-d\n"
        );
        env.fake_tool("fzf", "exit 130");
        assert_eq!(
            fails_in(&env, &project, &["worktree", "switch"]),
            "tasq: nothing chosen\n"
        );
        env.fake_tool("fzf", "echo 99");
        assert_eq!(
            fails_in(&env, &project, &["worktree", "switch"]),
            "tasq: not a choice: \"99\"\n"
        );
    }

    /// Runs `script` in `shell` with the function from `shell-init`, a fake
    /// `tasq` that logs its arguments and, for a switch, writes `<home>/to`.
    fn in_shell(env: &TestEnv, shell: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let (init, _) = run_in(env, &env.home, &["worktree", "shell-init", shell]);
        std::fs::write(env.home.join("init.sh"), init).unwrap();
        let fake = env.home.join("fakebin");
        std::fs::create_dir_all(&fake).unwrap();
        let to = env.home.join("to");
        std::fs::create_dir_all(&to).unwrap();
        let tasq = fake.join("tasq");
        std::fs::write(
            &tasq,
            format!(
                "#!/bin/sh\necho \"args: $* cd=${{TASQ_CD_FILE:+set}}\"\n\
                 [ -n \"$TASQ_CD_FILE\" ] && printf %s '{}' > \"$TASQ_CD_FILE\"\n\
                 [ \"$1\" = fail ] && exit 7\nexit 0\n",
                to.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tasq, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = std::process::Command::new(shell)
            .args([
                "-c",
                &format!(". '{}/init.sh'\n{script}", env.home.display()),
            ])
            .current_dir(&env.home)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
            .env("HOME", &env.home)
            .env("TMPDIR", &env.home)
            .output()
            .unwrap();
        env.normalize(&format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }

    #[test]
    fn shell_init_changes_directory_on_switch_only() {
        let env = TestEnv::fixture();
        for shell in ["bash", "zsh"] {
            if !Path::new("/usr/bin").join(shell).exists()
                && !Path::new("/bin").join(shell).exists()
            {
                continue;
            }
            assert_eq!(
                in_shell(
                    &env,
                    shell,
                    "tasq ls; pwd; tasq --profile x worktree switch b; echo $?; pwd; \
                                       tasq fail worktree switch; echo $?; ls \"$TMPDIR\" | grep -c tasq-cd"
                ),
                "args: ls cd=\n\
                 [ROOT]/home\n\
                 args: --profile x worktree switch b cd=set\n\
                 0\n\
                 [ROOT]/home/to\n\
                 args: fail worktree switch cd=set\n\
                 7\n\
                 0\n",
                "{shell}"
            );
        }
        let (fish, _) = run_in(&env, &env.home, &["worktree", "shell-init", "fish"]);
        assert!(fish.contains("function tasq --wraps tasq"), "{fish}");
        assert!(
            fish.contains("env TASQ_CD_FILE=$tasq_cd_file tasq $argv"),
            "{fish}"
        );
        let (bash, _) = run_in(&env, &env.home, &["worktree", "shell-init", "bash"]);
        let (zsh, _) = run_in(&env, &env.home, &["worktree", "shell-init", "zsh"]);
        assert_eq!(bash, zsh);
        let (json, _) = run_in(
            &env,
            &env.home,
            &["worktree", "shell-init", "bash", "--json"],
        );
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(doc["script"].as_str(), Some(bash.as_str()));
        env.tasq()
            .args(["worktree", "shell-init", "bash"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with("tasq: worktrees are disabled"));
    }
}
