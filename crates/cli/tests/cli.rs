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
