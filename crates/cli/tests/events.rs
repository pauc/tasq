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
        let opening: &[(&[&str], &[&str])] = &[
            (&[], &["store-open"]),
            (&["list"], &["store-open"]),
            (&["view", "3"], &["store-open"]),
            (&["store", "info"], &["store-open"]),
            (&["log", "3", "a note"], &["store-open", "post-write"]),
            (&["create", "Fresh"], &["store-open", "post-write"]),
            (&["done", "6"], &["store-open", "post-write"]),
        ];
        for (args, events) in opening {
            let out = env
                .tasq()
                .env("TASQ_ECHO_LOG", &log)
                .args(*args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
            assert_eq!(take_events(&log), *events, "{args:?}");
        }
        let not_opening: &[&[&str]] = &[
            &["config", "show"],
            &["plugins", "list"],
            &["completions", "bash"],
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
        // `doctor` opens the store quietly and sends its own event.
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .arg("doctor")
            .output()
            .unwrap();
        assert_eq!(take_events(&log), ["doctor"]);
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
        env.write_project_config("[plugins]\nenabled = [\"envy\"]\n");
        env.write_profile("p", "");
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

/// One `post-write` as the plugin read it: the command, then each write
/// as `<kind> <id> <file name>`.
#[derive(Debug, PartialEq, Eq)]
struct PostWrite {
    command: String,
    writes: Vec<String>,
}

/// The `post-write` events logged by `tasq-echo` since the log was last
/// cleared (other events are dropped), and clears it. Every file must be in
/// `store`.
fn take_post_writes(log: &Path, store: &Path) -> Vec<PostWrite> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let _ = std::fs::remove_file(log);
    text.lines()
        .filter_map(|line| line.strip_prefix("post-write "))
        .map(|json| {
            let payload: serde_json::Value = serde_json::from_str(json).unwrap();
            let writes = payload["writes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| {
                    let file = Path::new(w["file"].as_str().unwrap());
                    assert_eq!(file.parent(), Some(store), "{w}");
                    format!(
                        "{} {} {}",
                        w["kind"].as_str().unwrap(),
                        w["id"].as_str().unwrap(),
                        file.file_name().unwrap().to_string_lossy()
                    )
                })
                .collect();
            PostWrite {
                command: payload["command"].as_str().unwrap().to_owned(),
                writes,
            }
        })
        .collect()
}

fn post_write(command: &str, writes: &[String]) -> PostWrite {
    PostWrite {
        command: command.to_owned(),
        writes: writes.to_vec(),
    }
}

mod post_write {
    use super::*;

    /// `update <id> <file name>` of a fixture task.
    fn update(env: &TestEnv, id: u64) -> String {
        let file = env.file_of(id);
        format!(
            "update {id} {}",
            file.file_name().unwrap().to_string_lossy()
        )
    }

    /// `create <id> <file name>` of a task the command created.
    fn create(env: &TestEnv, id: u64) -> String {
        update(env, id).replacen("update", "create", 1)
    }

    /// Runs `tasq <args>` with the echo plugin logging to `log`; it must
    /// succeed.
    fn run(env: &TestEnv, log: &Path, args: &[&str]) {
        let out = env
            .tasq()
            .env("TASQ_ECHO_LOG", log)
            .env("TASQ_DEFAULT_PROJECT", &env.home)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    }

    /// Every one-task edit: one event, named after the command, with the
    /// task updated.
    #[test]
    fn every_edit_command_sends_its_one_update() {
        let home_dir = "@HOME@";
        let edits: &[(&[&str], &str)] = &[
            (&["set", "3", "A"], "set"),
            (&["log", "3", "a note"], "log"),
            (&["done", "3"], "done"),
            (&["reopen", "4"], "reopen"),
            (&["rename", "3", "Answer it"], "rename"),
            (&["tag", "3", "+extra"], "tag"),
            (&["due", "3", "2026-11-01"], "due"),
            (&["desc", "3", "Some words."], "desc"),
            (
                &["mr", "3", "https://gl.test/g/p/-/merge_requests/9", "Fix"],
                "mr",
            ),
            (&["session", "3", "sess-1"], "session"),
            (&["project", "3", home_dir], "project"),
        ];
        for (args, command) in edits {
            let env = TestEnv::fixture();
            let Some(log) = echo(&env) else { return };
            let home = env.home.display().to_string();
            let args: Vec<&str> = args
                .iter()
                .map(|a| if *a == home_dir { home.as_str() } else { a })
                .collect();
            run(&env, &log, &args);
            let id = if args[1] == "4" { 4 } else { 3 };
            assert_eq!(
                take_post_writes(&log, &env.store()),
                vec![post_write(command, &[update(&env, id)])],
                "{args:?}"
            );
        }
    }

    #[test]
    fn create_sends_the_new_task_once_even_when_it_also_updates_it() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        run(&env, &log, &["create", "Fresh"]);
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("create", &[create(&env, 8)])]
        );
        // `--worktree` writes the task twice: still one create.
        run(
            &env,
            &log,
            &[
                "--set",
                "worktree.enabled=true",
                "create",
                "Again",
                "--worktree",
                "b",
            ],
        );
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("create", &[create(&env, 9)])]
        );
        assert!(env.read_task(9).contains("## Worktrees"), "written twice");
    }

    /// A move that ranks the other tasks of the group lists them all.
    #[test]
    fn move_sends_every_task_it_ranked() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        for id in [9, 10] {
            env.write_task(
                &support::ulid_name("2026-10-05 09:00", id),
                &support::task_text(id, "status: ready\n", "# Another ready one\n"),
            );
        }
        // Below an unranked task: that one is ranked too.
        run(&env, &log, &["move", "10", "--after", "3"]);
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("move", &[update(&env, 3), update(&env, 10)])]
        );
        // Where it already is: nothing written, no event.
        run(&env, &log, &["move", "10", "--after", "3"]);
        assert_eq!(take_post_writes(&log, &env.store()), Vec::new());
    }

    #[test]
    fn apply_sends_the_task_it_wrote() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        let out = env.tasq().args(["--json", "view", "3"]).output().unwrap();
        let document = stdout(&out).replace("Answer the support ticket", "Answered");
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .arg("apply")
            .write_stdin(document)
            .assert()
            .success();
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("apply", &[update(&env, 3)])]
        );
    }

    /// One event for the whole sync, however many tasks it wrote.
    #[test]
    fn sync_sends_one_event_for_every_task_it_wrote() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        std::fs::write(env.home.join("prompt.md"), "triage\n").unwrap();
        env.fake_tool(
            "inbox-bridge",
            &format!(
                "{PROBE}\nread -r prompt\necho '[{{\"external_id\":\"a\",\"title\":\"One\"}},\
                 {{\"external_id\":\"b\",\"title\":\"Two\"}}]'"
            ),
        );
        env.write_project_config(&format!(
            "[plugins]\nenabled = [\"echo\"]\n\n[[source]]\nname = \"inbox\"\n\
             kind = \"llm-bridge\"\ncommand = \"inbox-bridge\"\nprompt_file = \"{}\"\n",
            env.home.join("prompt.md").display()
        ));
        run(&env, &log, &["sync"]);
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("sync", &[create(&env, 8), create(&env, 9)])]
        );
    }

    /// `pick` and `next` write the task they set in progress.
    #[test]
    fn launching_sends_the_status_change() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        env.fake_tool("claude", "exit 0");
        run(&env, &log, &["pick", "3"]);
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("pick", &[update(&env, 3)])]
        );
        // Already in progress: nothing written, no event.
        run(&env, &log, &["pick", "3"]);
        run(&env, &log, &["next"]);
        assert_eq!(take_post_writes(&log, &env.store()), Vec::new());
        run(&env, &log, &["set", "3", "ready"]);
        run(&env, &log, &["set", "1", "ready"]);
        take_post_writes(&log, &env.store());
        run(&env, &log, &["next"]);
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("next", &[update(&env, 1)])]
        );
    }

    #[test]
    fn worktree_set_sends_the_task_it_declared_on() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        run(&env, &log, &["project", "3", env.home.to_str().unwrap()]);
        take_post_writes(&log, &env.store());
        run(
            &env,
            &log,
            &[
                "--set",
                "worktree.enabled=true",
                "worktree",
                "set",
                "3",
                "b",
            ],
        );
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("worktree", &[update(&env, 3)])]
        );
    }

    /// Reads, edits that change nothing (whether or not the command calls
    /// the store) and dry runs send no event.
    #[test]
    fn nothing_written_sends_nothing() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        let home = env.home.display().to_string();
        run(&env, &log, &["project", "3", &home]);
        run(&env, &log, &["tag", "3", "+extra"]);
        take_post_writes(&log, &env.store());
        let out = env.tasq().args(["--json", "view", "3"]).output().unwrap();
        let unchanged = stdout(&out);
        let quiet: &[&[&str]] = &[
            &[],
            &["list"],
            &["view", "3"],
            &["store", "info"],
            &["set", "3", "ready"],
            &["tag", "3", "+extra"],
            &["rename", "3", "Answer the support ticket"],
            // Written through the store, but the file already says so.
            &["project", "3", &home],
            &["pick", "3", "--dry-run"],
            &["move", "3", "--top"],
        ];
        for args in quiet {
            run(&env, &log, args);
            assert_eq!(take_post_writes(&log, &env.store()), Vec::new(), "{args:?}");
        }
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .arg("apply")
            .write_stdin(unchanged)
            .assert()
            .success();
        assert_eq!(take_post_writes(&log, &env.store()), Vec::new(), "apply");
        // Refused before any write.
        env.tasq()
            .env("TASQ_ECHO_LOG", &log)
            .args(["log", "99", "a note"])
            .assert()
            .failure();
        assert_eq!(take_post_writes(&log, &env.store()), Vec::new(), "log 99");
        assert!(
            std::fs::read_to_string(&log).is_err(),
            "every other event was taken"
        );
    }

    #[test]
    fn a_dry_run_sync_sends_nothing() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        std::fs::write(env.home.join("prompt.md"), "triage\n").unwrap();
        env.fake_tool(
            "inbox-bridge",
            &format!(
                "{PROBE}\nread -r prompt\necho '[{{\"external_id\":\"a\",\"title\":\"One\"}}]'"
            ),
        );
        env.write_project_config(&format!(
            "[plugins]\nenabled = [\"echo\"]\n\n[[source]]\nname = \"inbox\"\n\
             kind = \"llm-bridge\"\ncommand = \"inbox-bridge\"\nprompt_file = \"{}\"\n",
            env.home.join("prompt.md").display()
        ));
        run(&env, &log, &["sync", "--dry-run"]);
        assert_eq!(take_post_writes(&log, &env.store()), Vec::new());
    }

    /// `pick` sets the task in progress, then a blocking `pre-launch` hook
    /// fails: the command fails, and the write it made is still sent.
    #[test]
    fn a_command_that_fails_after_a_write_still_sends_it() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        env.fake_tool("claude", "exit 0");
        env.write_project_config(
            "[plugins]\nenabled = [\"echo\"]\n\n[hooks]\npre-launch = [\"/bin/sh -c 'exit 5'\"]\n",
        );
        let out = env
            .tasq()
            .env("TASQ_ECHO_LOG", &log)
            .env("TASQ_DEFAULT_PROJECT", &env.home)
            .args(["pick", "3"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("pre-launch hook \"/bin/sh -c 'exit 5'\" failed"),
            "{}",
            stderr(&out)
        );
        assert_eq!(
            take_post_writes(&log, &env.store()),
            vec![post_write("pick", &[update(&env, 3)])]
        );
    }

    #[test]
    fn a_failing_plugin_is_a_warning_and_the_write_stands() {
        let env = TestEnv::fixture();
        env.fake_tool(
            "tasq-bad",
            &format!("{PROBE}\n[ \"$TASQ_EVENT\" = post-write ] || exit 0\necho nope >&2\nexit 3"),
        );
        env.write_project_config("[plugins]\nenabled = [\"bad\"]\n");
        env.tasq()
            .args(["log", "3", "a note"])
            .assert()
            .success()
            .stdout("[3] logged: a note\n")
            .stderr("tasq: warning: plugin bad: post-write event failed: exit status 3: nope\n");
        assert!(env.read_task(3).contains("a note"));
    }
}

/// Writes `text` as `plugin`'s lasting warnings in `env`'s store.
fn write_warnings(env: &TestEnv, plugin: &str, text: &str) -> PathBuf {
    let file = env
        .store()
        .join(".tasq")
        .join("plugins")
        .join(plugin)
        .join("warnings");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    file
}

mod lasting_warnings {
    use super::*;

    /// Every command that opens the store prints them, after the
    /// missing-plugin warnings and the `store-open` ones, until the plugin
    /// removes them; even a plugin that is not installed has them read.
    #[test]
    fn every_command_prints_them_until_the_plugin_clears_them() {
        let env = TestEnv::fixture();
        env.fake_tool(
            "tasq-bad",
            &format!("{PROBE}\n[ \"$TASQ_EVENT\" = store-open ] || exit 0\nexit 3"),
        );
        env.fake_tool("tasq-quiet", &format!("{PROBE}\nexit 0"));
        env.write_project_config("[plugins]\nenabled = [\"gone\", \"bad\", \"quiet\"]\n");
        let quiet = write_warnings(
            &env,
            "quiet",
            "push failed: no network\n\n  \nrebase stopped\n",
        );
        write_warnings(&env, "gone", "left behind\n");
        write_warnings(&env, "other", "not enabled\n");
        let expected = "tasq: warning: plugin gone is enabled but tasq-gone is not on PATH\n\
                        tasq: warning: plugin bad: store-open event failed: exit status 3\n\
                        tasq: warning: gone: left behind\n\
                        tasq: warning: quiet: push failed: no network\n\
                        tasq: warning: quiet: rebase stopped\n";
        for args in [&["list"][..], &["log", "3", "a note"], &["store", "info"]] {
            let out = env.tasq().args(args).output().unwrap();
            assert_eq!(out.status.code(), Some(0), "{args:?}");
            assert_eq!(stderr(&out), expected, "{args:?}");
        }
        // A failing command keeps its exit code and still warns.
        let out = env.tasq().args(["view", "99"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).starts_with(expected), "{}", stderr(&out));
        // Commands that never open the store say nothing.
        let out = env.tasq().args(["config", "show"]).output().unwrap();
        assert_eq!(stderr(&out), "");

        std::fs::remove_file(&quiet).unwrap();
        std::fs::remove_dir_all(env.store().join(".tasq/plugins/gone")).unwrap();
        let out = env.tasq().arg("list").output().unwrap();
        assert_eq!(
            stderr(&out),
            "tasq: warning: plugin gone is enabled but tasq-gone is not on PATH\n\
             tasq: warning: plugin bad: store-open event failed: exit status 3\n"
        );
    }

    /// A `tasq` a plugin runs during an event (`TASQ_EVENT` set) keeps
    /// them to itself: they would only clutter the plugin's stderr.
    #[test]
    fn are_not_printed_by_a_tasq_run_during_an_event() {
        let env = TestEnv::fixture();
        env.fake_tool("tasq-quiet", &format!("{PROBE}\nexit 0"));
        env.write_project_config("[plugins]\nenabled = [\"quiet\"]\n");
        write_warnings(&env, "quiet", "push failed\n");
        env.tasq()
            .env("TASQ_EVENT", "post-write")
            .arg("list")
            .assert()
            .success()
            .stderr("");
        env.tasq()
            .env("TASQ_EVENT", "")
            .arg("list")
            .assert()
            .success()
            .stderr("tasq: warning: quiet: push failed\n");
    }

    /// A plugin that leaves a warning while it handles `store-open` (its
    /// background job failed last time) is heard on that same command.
    #[test]
    fn are_read_after_store_open() {
        let env = TestEnv::fixture();
        let file = write_warnings(&env, "mirror", "");
        env.fake_tool(
            "tasq-mirror",
            &format!("{PROBE}\necho 'fetch failed' > '{}'", file.display()),
        );
        env.write_project_config("[plugins]\nenabled = [\"mirror\"]\n");
        env.tasq()
            .arg("list")
            .assert()
            .success()
            .stderr("tasq: warning: mirror: fetch failed\n");
    }
}

mod doctor {
    use super::*;

    /// A plugin answering `doctor` with checks, one whose answer does not
    /// parse, one that fails, one with nothing to say, and lasting
    /// warnings: each plugin's checks follow its `tasq-<name>` check,
    /// after the store's and before the tools'.
    fn plugins(env: &TestEnv) {
        let answer = r#"[{"name": "remote", "status": "ok", "message": "origin reachable"},
            {"name": "push", "status": "warn", "message": "3 commits not pushed", "fix": "tasq git push"}]"#;
        let only_doctor = "[ \"$TASQ_EVENT\" = doctor ] || exit 0";
        env.fake_tool(
            "tasq-git",
            &format!("{PROBE}\n{only_doctor}\nread -r payload\necho '{answer}'"),
        );
        env.fake_tool(
            "tasq-broken",
            &format!("{PROBE}\n{only_doctor}\necho 'all good'"),
        );
        env.fake_tool(
            "tasq-bad",
            &format!("{PROBE}\n{only_doctor}\necho nope >&2\nexit 3"),
        );
        env.fake_tool("tasq-quiet", &format!("{PROBE}\nexit 0"));
        env.write_project_config(
            "[plugins]\nenabled = [\"git\", \"broken\", \"bad\", \"quiet\"]\n",
        );
        write_warnings(env, "git", "push failed: no network\n");
        write_warnings(env, "quiet", "one\ntwo\n");
    }

    #[test]
    fn lists_the_plugins_checks_and_lasting_warnings() {
        let env = TestEnv::fixture();
        plugins(&env);
        let out = env.tasq().arg("doctor").output().unwrap();
        assert_eq!(out.status.code(), Some(1), "an unparseable answer fails");
        assert_eq!(stderr(&out), "", "doctor reports them as checks only");
        insta::assert_snapshot!(env.normalize(&stdout(&out)));
    }

    #[test]
    fn json() {
        let env = TestEnv::fixture();
        plugins(&env);
        let out = env.tasq().args(["--json", "doctor"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let checks: Vec<&serde_json::Value> = doc["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| {
                let name = c["name"].as_str().unwrap();
                ["git", "broken", "bad", "quiet"]
                    .iter()
                    .any(|p| name.starts_with(&format!("{p}/")))
            })
            .collect();
        insta::assert_snapshot!(env.normalize(&serde_json::to_string_pretty(&checks).unwrap()));
    }

    /// The plugin gets the `doctor` payload, like any other event.
    #[test]
    fn the_plugin_is_told_doctor() {
        let env = TestEnv::fixture();
        let Some(log) = echo(&env) else { return };
        let out = env
            .tasq()
            .env("TASQ_ECHO_LOG", &log)
            .arg("doctor")
            .output()
            .unwrap();
        assert!(stdout(&out).contains("OK    tasq-echo"), "{}", stdout(&out));
        let line = std::fs::read_to_string(&log).unwrap();
        assert!(
            env.normalize(&line).starts_with(
                "doctor {\"schema\":1,\"event\":\"doctor\",\"store\":{\"name\":\"default\",\
                 \"location\":\"[ROOT]/home/.local/share/tasq/stores/default\"},\"config\":{}}"
            ),
            "{line}"
        );
        assert_eq!(take_events(&log), ["doctor"], "and no store-open");
    }
}

/// Ctrl-C of `tasq` (SIGINT to its process group, which the plugin's group
/// is not part of). Linux: the plugin's group is read from `/proc`.
#[cfg(target_os = "linux")]
mod interrupt {
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};

    use rustix::process::{Pid, Signal, kill_process};

    use super::*;

    /// A background `tasq`, killed if the test ends before it does.
    struct Running(std::process::Child);

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Running {
        /// Sends SIGINT to `tasq` alone, as the terminal does to its group.
        fn interrupt(&self) {
            let pid = Pid::from_raw(self.0.id().try_into().unwrap()).unwrap();
            kill_process(pid, Signal::INT).unwrap();
        }

        /// Waits up to ten seconds for `tasq` to exit.
        fn exit_status(&mut self) -> std::process::ExitStatus {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    return status;
                }
                assert!(Instant::now() < deadline, "tasq never exited");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// Waits up to five seconds for `path` to exist.
    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The processes still in the process group `group`, from `/proc`.
    fn group_members(group: &str) -> Vec<String> {
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path().join("stat")).ok())
            .filter(|stat| {
                // `pid (comm) state ppid pgrp ...`; comm may hold spaces.
                let rest = stat.rsplit_once(") ").map_or("", |(_, rest)| rest);
                rest.split(' ').nth(2) == Some(group)
            })
            .collect()
    }

    #[test]
    fn ends_the_running_plugin_and_exits_130() {
        let env = TestEnv::fixture();
        let started = env.home.join("plugin.pid");
        let got = env.home.join("got-int");
        // The trap runs once the foreground `sleep`, in the same group,
        // has died of the same SIGINT.
        env.fake_tool(
            "tasq-slow",
            &format!(
                "{PROBE}\ntrap 'echo int > \"{got}\"; exit 0' INT\necho $$ > \"{started}.tmp\"\n\
                 /bin/mv \"{started}.tmp\" \"{started}\"\n/bin/sleep 30",
                got = got.display(),
                started = started.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"slow\"]\ntimeout = 30\n");
        let mut tasq = Running(env.tasq_std().arg("list").spawn().unwrap());
        wait_for(&started);
        let group = std::fs::read_to_string(&started).unwrap().trim().to_owned();
        let interrupted = Instant::now();
        tasq.interrupt();
        let status = tasq.exit_status();
        assert_eq!(status.code(), Some(130), "{status:?}");
        assert!(interrupted.elapsed() < Duration::from_secs(5));
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "int\n");
        assert_eq!(group_members(&group), Vec::<String>::new());
    }

    /// A plugin that shrugs off the passed-on SIGINT is killed, with its
    /// group, on the second Ctrl-C, long before its timeout.
    #[test]
    fn a_second_ctrl_c_kills_a_plugin_that_keeps_running() {
        let env = TestEnv::fixture();
        let started = env.home.join("plugin.pid");
        let got = env.home.join("got-int");
        env.fake_tool(
            "tasq-stubborn",
            &format!(
                "{PROBE}\ntrap 'echo int >> \"{got}\"' INT\necho $$ > \"{started}.tmp\"\n\
                 /bin/mv \"{started}.tmp\" \"{started}\"\nwhile :; do /bin/sleep 1; done",
                got = got.display(),
                started = started.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"stubborn\"]\ntimeout = 30\n");
        let mut tasq = Running(env.tasq_std().arg("list").spawn().unwrap());
        wait_for(&started);
        let group = std::fs::read_to_string(&started).unwrap().trim().to_owned();
        let interrupted = Instant::now();
        tasq.interrupt();
        wait_for(&got);
        assert_eq!(
            tasq.0.try_wait().unwrap(),
            None,
            "still waiting on the plugin"
        );
        tasq.interrupt();
        let status = tasq.exit_status();
        assert_eq!(status.code(), Some(130), "{status:?}");
        assert!(interrupted.elapsed() < Duration::from_secs(5));
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "int\n");
        assert_eq!(group_members(&group), Vec::<String>::new());
    }

    /// The handler is in place only while a plugin runs: a Ctrl-C later in
    /// the command (here a write waiting on a held lock) kills `tasq` the
    /// default way.
    #[test]
    fn after_an_event_kills_tasq_the_default_way() {
        let env = TestEnv::fixture();
        let opened = env.home.join("opened");
        env.fake_tool(
            "tasq-quick",
            &format!(
                "{PROBE}\necho $$ > \"{opened}.tmp\"\n/bin/mv \"{opened}.tmp\" \"{opened}\"",
                opened = opened.display()
            ),
        );
        env.write_project_config("[plugins]\nenabled = [\"quick\"]\n");
        let lock = env.store().join(".tasq").join("lock");
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        // Wall time, ahead of `TASQ_NOW`: never stale to this `tasq`.
        std::fs::write(&lock, "").unwrap();
        let mut tasq = Running(env.tasq_std().args(["log", "3", "a note"]).spawn().unwrap());
        wait_for(&opened);
        // The event is over once tasq has reaped the plugin; the watch ends
        // right after, well within the margin.
        let plugin = Path::new("/proc").join(std::fs::read_to_string(&opened).unwrap().trim());
        let deadline = Instant::now() + Duration::from_secs(5);
        while plugin.exists() {
            assert!(Instant::now() < deadline, "the plugin was never reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));
        tasq.interrupt();
        let status = tasq.exit_status();
        assert_eq!(status.signal(), Some(2), "{status:?}");
    }
}
