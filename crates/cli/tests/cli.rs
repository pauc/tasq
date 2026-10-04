//! Integration tests: the `tasq` binary against a temporary fixture
//! notebook. Output is snapshotted with `insta` (`cargo insta review`, or
//! `INSTA_UPDATE=always cargo test -p tasq` to accept everything).

mod support;

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
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert_eq!(
            file,
            "# [ ] Minimal\n\n## Tags\n\n#B #ready\n\n## Progress\n\n- 2026-10-07 09:30: created via tasq create\n"
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
        assert_eq!(
            stderr(&out),
            format!(
                "tasq: warning: no title lookup for {url} yet; tracked as \"group/project!77\" (fix it with tasq mr <id> {url} \"<title>\")\n"
            )
        );
        let file = std::fs::read_to_string(env.notebook().join(NEW_FILE)).unwrap();
        assert_snapshot!(file);
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
    fn create_with_gwm_manager_uses_the_workspace_above_the_project() {
        let env = TestEnv::fixture();
        let project = env.home.join("ws").join("proj");
        env.git_repo(&project);
        std::fs::write(env.home.join("ws").join("gwm.yml"), "name: ws\n").unwrap();
        let log = env.home.join("gwm.log");
        let target = env.home.join("ws").join("wt-z");
        env.fake_tool(
            "gwm",
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
            "create -b feature/z --no-tmux -s\n"
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
        env.tasq()
            .env("TASQ_DEFAULT_PROJECT", project.to_str().unwrap())
            .args(["worktree", "3", "--create", "b"])
            .assert()
            .code(1)
            .stderr(predicate::str::starts_with(
                "tasq: no gwm workspace found above",
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
