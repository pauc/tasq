//! Integration tests: the `tasq` binary against a temporary fixture
//! notebook. Output is snapshotted with `insta` (`cargo insta review`, or
//! `INSTA_UPDATE=always cargo test -p tasq` to accept everything).

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

    #[test]
    fn sync_without_a_repository_is_skipped() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["store", "sync"]).output().unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_snapshot!(env.normalize(&stdout(&out)));
        let json = env
            .tasq()
            .args(["--json", "store", "sync"])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["schema"], 1);
        assert_eq!(value["synced"], false);
    }

    #[test]
    fn verbose_prints_the_notebook_path() {
        let env = TestEnv::fixture();
        let out = env.tasq().args(["-v", "store", "info"]).output().unwrap();
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: notebook: [ROOT]/nb/home\n"
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
    fn done_on_a_done_task_changes_nothing() {
        let env = TestEnv::fixture();
        let before =
            std::fs::read_to_string(env.notebook().join("20260903110000.todo.md")).unwrap();
        env.tasq()
            .args(["done", "4"])
            .assert()
            .success()
            .stdout("[4] done: Ship the release notes\n");
        let after = std::fs::read_to_string(env.notebook().join("20260903110000.todo.md")).unwrap();
        assert_eq!(after, before);
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
            .tasq()
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
            .tasq()
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
            .tasq()
            .args(["worktree", "3", plain.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] worktree: [ROOT]/home/plain\n"
        );
        env.tasq()
            .args(["worktree", "3", "/definitely/not/here"])
            .assert()
            .code(1)
            .stderr("tasq: worktree path not found: /definitely/not/here\n");
        env.tasq().args(["worktree", "3"]).assert().code(2);
    }

    #[test]
    fn create_with_git_manager() {
        let env = TestEnv::fixture();
        let project = env.home.join("proj");
        env.git_repo(&project);
        env.tasq()
            .args(["project", "3", project.to_str().unwrap()])
            .assert()
            .success();
        let out = env
            .tasq()
            .env("TASQ_WORKTREE_MANAGER", "git")
            .args(["worktree", "3", "--create", "feature/y"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "[3] worktree: [ROOT]/home/proj-feature-y (feature/y)\n"
        );
        assert!(env.home.join("proj-feature-y").join("README").is_file());
    }

    #[test]
    fn create_with_a_command_manager() {
        let env = TestEnv::fixture();
        let project = env.home.join("ws").join("proj");
        env.git_repo(&project);
        let log = env.home.join("mkwt.log");
        let target = env.home.join("ws").join("wt-z");
        env.fake_tool(
            "mkwt",
            &format!(
                "[ -n \"${{FAKE_PROBE:-}}\" ] && exit 0\nprintf '%s\\n' \"$*\" > '{}'\n/bin/mkdir -p '{}'\necho 'Linked .envrc'\necho '{}'",
                log.display(),
                target.display(),
                target.display()
            ),
        );
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .env("TASQ_WORKTREE_MANAGER", "command")
            .env("TASQ_WORKTREE_COMMAND", "mkwt {new} {branch} --no-tmux -s")
            .args(["worktree", "3", "--create", "feature/z"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(
            env.normalize(&stdout(&out)),
            "Linked .envrc\n[3] worktree: [ROOT]/home/ws/wt-z (feature/z)\n"
        );
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "-b feature/z --no-tmux -s\n"
        );
    }

    #[test]
    fn create_errors() {
        let env = TestEnv::fixture();
        env.tasq()
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr("tasq: task 3 tracks no project and work.default_project is unset; set one with tasq project 3 <path>\n");
        let project = env.home.join("proj");
        std::fs::create_dir(&project).unwrap();
        // The command manager without a command is a config error.
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .args(["worktree", "3", "--create", "b", "--set", "work.worktree_manager=command"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: work.worktree_manager = \"command\" (set by --set) requires work.worktree_command",
            ));
        // A failing command surfaces its message.
        env.fake_tool("mkwt", "echo 'boom' >&2\nexit 3");
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .env("TASQ_WORKTREE_MANAGER", "command")
            .env("TASQ_WORKTREE_COMMAND", "mkwt {branch}")
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr("tasq: mkwt failed for branch 'b': boom\n");
        // git is the default manager and needs a repository.
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: git failed for branch 'b':",
            ));
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
        value["task"]["title"] = "Renamed".into();
        let before = env.read_task("20260902100000.todo.md");
        env.tasq()
            .arg("apply")
            .write_stdin(value.to_string())
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: unsupported: changing title of task 3",
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
            .tasq()
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
        // Task 1 tracks a project and a worktree that do not exist here.
        assert_eq!(
            env.normalize(&stderr(&out)),
            "tasq: warning: tracked project not found: /home/pau/code/tasks; using work.default_project\n\
             tasq: warning: tracked worktree is gone: /home/pau/code/tasks-wt/feature-a\n\
             tasq: warning: dry run: would offer to recreate it on branch feature-a; starting in the project instead\n"
        );
        assert_eq!(
            env.normalize(&stdout(&out)),
            "Task: [1] Rewrite the tasks script in Rust\nStarting in project: [ROOT]/home\nLauncher: shell (dry run)\ncd [ROOT]/home\nTASQ_TASK_ID=1 TASQ_NOTEBOOK=home exec sh\n"
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

    #[test]
    fn missing_worktree_is_not_asked_about_without_a_terminal() {
        let env = TestEnv::fixture();
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .args(["pick", "1", "--launcher", "shell", "--dry-run"])
            .output()
            .unwrap();
        assert!(out.status.success());
        // stdin is a pipe here, so even without --dry-run no prompt appears.
        let out = env
            .tasq()
            .env("TASQ_DEFAULT_PROJECT", env.home.to_str().unwrap())
            .env("SHELL", "/nonexistent/shell")
            .args(["pick", "1", "--launcher", "shell"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let err = stderr(&out);
        assert!(
            err.contains("no terminal to ask on, starting in the project instead"),
            "{err}"
        );
        assert!(err.contains("tasq: /nonexistent/shell failed:"), "{err}");
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
            .stderr("tasq: task 4 is done; reopen it first (tasq set 4 <status>)\n");
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
            .tasq()
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
            .tasq()
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
            .tasq()
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
        env.tasq()
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
            .tasq()
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
            .tasq()
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
            .args(["tlogs", "2026-10-02", "2026-10-04"])
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
                "--json",
                "2026-10-02",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(doc["schema"], 1);
        assert_eq!(doc["days"][0]["tasks"][0]["hours"], 6);
        let out = env
            .tasq()
            .env("PATH", &path)
            .args(["plugins", "list"])
            .output()
            .unwrap();
        assert!(stdout(&out).contains("  tlogs  "), "{}", stdout(&out));
    }
}
