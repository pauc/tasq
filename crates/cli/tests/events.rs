//! Plugin events (ADR 0030): a real `tasq` against the fixture store with a
//! fake plugin on `PATH`, asserting on what the plugin received and on what
//! the user saw. `examples/plugins/tasq-echo` (bash builtins only) logs
//! every event; one-off behaviours (a failure, a hang) are `fake_tool`s.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::path::{Path, PathBuf};

use support::{TestEnv, stderr, stdout};

const PROBE: &str = "[ -n \"${FAKE_PROBE:-}\" ] && exit 0";

/// Installs `examples/plugins/tasq-echo` (and `bash`, which it runs on) in
/// `env.bin`, enables it, and returns the log it writes to (what
/// `TASQ_ECHO_LOG` must point at). `None`, with a note, without bash.
#[allow(
    clippy::print_stderr,
    reason = "the note says why the test did nothing"
)]
fn echo(env: &TestEnv) -> Option<PathBuf> {
    let Some(bash) = support::find_on_path("bash") else {
        eprintln!("skipping: bash not installed");
        return None;
    };
    std::os::unix::fs::symlink(bash, env.bin.join("bash")).unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/tasq-echo")
        .canonicalize()
        .unwrap();
    std::os::unix::fs::symlink(plugin, env.bin.join("tasq-echo")).unwrap();
    env.write_project_config("[plugins]\nenabled = [\"echo\"]\n");
    Some(env.home.join("echo.log"))
}

/// The events logged by `tasq-echo` since the log was last cleared, and
/// clears it.
fn take_events(log: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    text.lines()
        .map(|line| {
            line.split_once(' ')
                .map_or(line, |(event, _)| event)
                .to_owned()
        })
        .collect()
}

mod store_open {
    use super::*;

    #[test]
    fn fires_once_for_each_command_that_opens_the_store() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        let opening: &[&[&str]] = &[
            &[],
            &["list"],
            &["view", "3"],
            &["store", "info"],
            &["log", "3", "a note"],
            &["create", "Fresh"],
            &["done", "6"],
        ];
        for args in opening {
            let out = env
                .tasq()
                .env("TASQ_ECHO_LOG", &log)
                .args(*args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert_eq!(take_events(&log), vec!["store-open"], "{args:?}");
        }
        let not_opening: &[&[&str]] = &[
            &["config", "show"],
            &["plugins", "list"],
            &["completions", "bash"],
            &["doctor"],
            &["echo", "hook"],
        ];
        for args in not_opening {
            env.tasq()
                .env("TASQ_ECHO_LOG", &log)
                .args(*args)
                .output()
                .unwrap();
            // `tasq echo hook` dispatches to the plugin itself, which logs
            // an empty event name: no event was sent.
            let events = take_events(&log);
            assert!(events.iter().all(String::is_empty), "{args:?}: {events:?}");
        }
    }
}

mod runner {
    use super::*;

    /// The payload exactly as the plugin read it, `[plugin.echo]` from a
    /// file and `--set`, the store from `--set store.name`.
    #[test]
    fn the_payload_carries_the_schema_the_store_and_the_plugin_config() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        env.write_project_config(
            "[plugins]\nenabled = [\"echo\"]\n\n[plugin.echo]\ninterval = 60\nsince = 2026-10-26\n\
             [plugin.echo.deep]\nlist = [1, \"two\"]\n\n[plugin.other]\nsecret = \"not yours\"\n",
        );
        support::copy_dir(&env.store(), &env.data_dir().join("work"));
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .args([
                "--set",
                "store.name=work",
                "--set",
                "plugin.echo.interval=5",
                "list",
            ])
            .assert()
            .success()
            .stderr("");
        insta::assert_snapshot!(env.normalize(&std::fs::read_to_string(&log).unwrap()));
        // No table: an empty config.
        std::fs::remove_file(&log).unwrap();
        env.write_project_config("[plugins]\nenabled = [\"echo\"]\n");
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .arg("list")
            .assert()
            .success();
        let line = std::fs::read_to_string(&log).unwrap();
        assert!(line.ends_with(",\"config\":{}}\n"), "{line}");
    }

    #[test]
    fn the_plugin_runs_as_tasq_dash_name_hook_with_the_event_and_passthrough_env() {
        let env = TestEnv::fixture();
        let log = env.home.join("env.log");
        env.fake_tool(
            "tasq-envy",
            &format!(
                "{PROBE}\necho \"args=$* event=$TASQ_EVENT bin=${{TASQ_BIN:+set}} \
                 profile=${{TASQ_PROFILE-unset}} set=${{TASQ_SET-unset}}\" >> '{}'",
                log.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"envy\"]\n\n[profile.p]\n");
        env.tasq()
            .args(["--profile", "p", "--set", "ui.no_osc8=true", "list"])
            .assert()
            .success()
            .stderr("");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "args=hook event=store-open bin=set profile=p set=ui.no_osc8=true\n"
        );
    }

    #[test]
    fn plugins_run_in_enabled_order_and_their_output_shows_at_verbose() {
        let env = TestEnv::fixture();
        env.fake_tool("tasq-one", &format!("{PROBE}\necho first"));
        env.fake_tool("tasq-two", &format!("{PROBE}\necho second"));
        env.fake_tool("tasq-mute", &format!("{PROBE}\necho ' '"));
        // A plugin that prints only blanks shows nothing, even at -v.
        env.write_project_config("[plugins]\nenabled = [\"two\", \"one\", \"mute\"]\n");
        env.tasq().arg("list").assert().success().stderr("");
        let out = env.tasq().args(["-v", "list"]).output().unwrap();
        let err = env.normalize(&stderr(&out));
        assert!(
            err.ends_with(
                "tasq: plugin two: store-open: second\ntasq: plugin one: store-open: first\n"
            ),
            "{err}"
        );
    }

    #[test]
    fn a_failing_plugin_is_a_warning_and_the_exit_code_stands() {
        let env = TestEnv::fixture();
        env.fake_tool("tasq-bad", &format!("{PROBE}\necho nope >&2\nexit 3"));
        env.fake_tool("tasq-quiet", &format!("{PROBE}\nexit 4"));
        env.write_project_config("[plugins]\nenabled = [\"bad\", \"quiet\"]\n");
        let out = env.tasq().arg("list").output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert!(
            stdout(&out).starts_with("IN PROGRESS\n"),
            "{}",
            stdout(&out)
        );
        assert_eq!(
            stderr(&out),
            "tasq: warning: plugin bad: store-open event failed: exit status 3: nope\n\
             tasq: warning: plugin quiet: store-open event failed: exit status 4\n"
        );
        // A timeout too large for the clock waits as long as it takes.
        let out = env
            .tasq()
            .args(["--set", "plugins.timeout=9223372036854775807", "list"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            stderr(&out).ends_with("plugin quiet: store-open event failed: exit status 4\n"),
            "{}",
            stderr(&out)
        );
        // A command that fails keeps its own exit code.
        let out = env.tasq().args(["view", "99"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq: warning: plugin bad: store-open event failed: "),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn a_plugin_that_hangs_is_cut_off_after_the_timeout() {
        let env = TestEnv::fixture();
        // `exec`, so the shell itself is what gets killed.
        env.fake_tool("tasq-slow", &format!("{PROBE}\nexec /bin/sleep 30"));
        env.write_project_config("[plugins]\nenabled = [\"slow\"]\ntimeout = 1\n");
        let started = std::time::Instant::now();
        let out = env.tasq().arg("list").output().unwrap();
        let took = started.elapsed();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            stderr(&out),
            "tasq: warning: plugin slow: store-open event failed: timed out after 1s\n"
        );
        assert!(
            took >= std::time::Duration::from_secs(1) && took < std::time::Duration::from_secs(10),
            "{took:?}"
        );
    }

    /// The event is over when the plugin's output closes: a background job
    /// holding it is cut off at the timeout, with the rest of the
    /// plugin's process group.
    #[test]
    fn a_background_job_holding_the_output_is_cut_off_with_its_group() {
        let env = TestEnv::fixture();
        let marker = env.home.join("late");
        env.fake_tool(
            "tasq-leaky",
            &format!(
                "{PROBE}\n{{ /bin/sleep 3; echo late > '{}'; }} &\nexit 0",
                marker.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"leaky\"]\ntimeout = 1\n");
        let started = std::time::Instant::now();
        let out = env.tasq().arg("list").output().unwrap();
        let took = started.elapsed();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            stderr(&out),
            "tasq: warning: plugin leaky: store-open event failed: timed out after 1s\n"
        );
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
        std::thread::sleep(std::time::Duration::from_millis(3500).saturating_sub(took));
        assert!(!marker.exists(), "the background job was killed");
    }

    /// A `tasq` run by a plugin during an event tells no plugin: a plugin
    /// that reads the store through `$TASQ_BIN` would otherwise wake itself
    /// without end.
    #[test]
    fn tasq_called_by_a_plugin_fires_no_event() {
        let env = TestEnv::fixture();
        let log = env.home.join("calls.log");
        env.fake_tool(
            "tasq-reader",
            &format!(
                "{PROBE}\necho \"$TASQ_EVENT\" >> '{}'\n\"$TASQ_BIN\" list > /dev/null",
                log.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"reader\"]\ntimeout = 5\n");
        env.tasq().arg("list").assert().success().stderr("");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "store-open\n");
    }
}
