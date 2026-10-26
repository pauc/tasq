//! `tasq store lock -- <cmd>` (ADR 0030): a real `tasq` against the fixture
//! store running a fake command, asserting on what the command saw, on the
//! lock file, and on what the user saw.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use support::{TestEnv, stderr, stdout};

/// Every fake command starts with this, so `fake_tool`'s probe runs
/// nothing else.
const PROBE: &str = "[ -n \"${FAKE_PROBE:-}\" ] && exit 0";

/// The lock file of the default store.
fn lock_file(env: &TestEnv) -> PathBuf {
    env.store().join(".tasq").join("lock")
}

/// The instant `TASQ_NOW` (2026-10-06 09:00) names in `TZ=UTC`.
fn tasq_now() -> SystemTime {
    let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 6)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()
        .and_utc();
    SystemTime::UNIX_EPOCH + Duration::from_secs(at.timestamp().try_into().unwrap())
}

/// A lock held by somebody else, last touched at `at`.
fn hold_lock(env: &TestEnv, at: SystemTime) {
    let path = lock_file(env);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// A fake command that records it ran in `<home>/ran`.
fn marker(env: &TestEnv) -> PathBuf {
    env.fake_tool("marker", &format!("{PROBE}\necho ran > \"$HOME/ran\""));
    env.home.join("ran")
}

/// A background `tasq`, killed if the test ends before it does.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

#[test]
fn runs_the_command_holding_the_lock_with_its_arguments_and_environment() {
    let env = TestEnv::fixture();
    env.fake_tool(
        "locked",
        &format!(
            "{PROBE}\n\
             [ -f \"$TASQ_STORE_DIR/.tasq/lock\" ] && echo held\n\
             echo \"locked=$TASQ_LOCKED dir=$TASQ_STORE_DIR\"\n\
             echo \"$#: $1|$2\"\n\
             [ -n \"$TASQ_BIN\" ] && echo bin\n\
             echo to stderr >&2"
        ),
    );
    let out = env
        .tasq()
        .args(["store", "lock", "--", "locked", "a", "b c"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!(
            "held\nlocked=1 dir={}\n2: a|b c\nbin\n",
            env.store().display()
        )
    );
    assert_eq!(stderr(&out), "to stderr\n");
    assert!(!lock_file(&env).exists(), "released");
}

#[test]
fn creates_a_store_that_does_not_exist_yet() {
    let env = TestEnv::empty();
    let ran = marker(&env);
    let out = env
        .tasq()
        .args(["store", "lock", "--", "marker"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(ran.exists());
    assert!(env.store().is_dir());
    assert!(!lock_file(&env).exists());
}

#[test]
fn passes_the_exit_code_through() {
    let env = TestEnv::fixture();
    env.fake_tool("fails", &format!("{PROBE}\nexit 3"));
    let out = env
        .tasq()
        .args(["store", "lock", "--", "fails"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(stderr(&out), "");
    assert!(!lock_file(&env).exists(), "released after a failure too");
}

#[test]
fn a_command_killed_by_a_signal_exits_128_plus_the_signal() {
    let env = TestEnv::fixture();
    env.fake_tool("killed", &format!("{PROBE}\nkill -TERM $$"));
    let out = env
        .tasq()
        .args(["store", "lock", "--", "killed"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(128 + 15));
    assert!(!lock_file(&env).exists());
}

#[test]
fn a_command_that_cannot_start_is_an_error() {
    let env = TestEnv::fixture();
    let out = env
        .tasq()
        .args(["store", "lock", "--", "no-such-command"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with("tasq: could not run no-such-command: "),
        "{}",
        stderr(&out)
    );
    assert!(!lock_file(&env).exists());
}

#[test]
fn a_lock_still_held_at_the_timeout_runs_nothing() {
    let env = TestEnv::fixture();
    let ran = marker(&env);
    hold_lock(&env, tasq_now());
    let out = env
        .tasq()
        .args(["store", "lock", "--timeout", "0", "--", "marker", "a b"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        format!(
            "tasq: the store lock {} is still held after 0s; marker 'a b' was not run\n",
            lock_file(&env).display()
        )
    );
    assert!(!ran.exists());
    assert!(
        lock_file(&env).exists(),
        "somebody else's lock is left alone"
    );
}

#[test]
fn waits_for_a_held_lock_until_the_timeout() {
    let env = TestEnv::fixture();
    let ran = marker(&env);
    hold_lock(&env, tasq_now());
    let started = Instant::now();
    let out = env
        .tasq()
        .args(["store", "lock", "--timeout", "1", "--", "marker"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert!(!ran.exists());
}

#[test]
fn a_lock_released_within_the_timeout_is_taken() {
    let env = TestEnv::fixture();
    let ran = marker(&env);
    hold_lock(&env, tasq_now());
    let mut child = Running(
        env.tasq_std()
            .args(["store", "lock", "--timeout", "30", "--", "marker"])
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(!ran.exists(), "waiting while the lock is held");
    std::fs::remove_file(lock_file(&env)).unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(ran.exists());
}

#[test]
fn a_stale_lock_is_broken_by_the_configured_clock() {
    let env = TestEnv::fixture();
    let ran = marker(&env);
    hold_lock(&env, tasq_now() - Duration::from_secs(120));
    let out = env
        .tasq()
        .args(["store", "lock", "--timeout", "0", "--", "marker"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(ran.exists());
    assert!(!lock_file(&env).exists());
}

#[test]
fn a_lock_that_cannot_be_taken_runs_nothing() {
    let env = TestEnv::fixture();
    let ran = marker(&env);
    std::fs::write(env.store().join(".tasq"), "a file where the directory goes").unwrap();
    let out = env
        .tasq()
        .args(["store", "lock", "--", "marker"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).starts_with(&format!(
            "tasq: could not take the store lock {}: ",
            lock_file(&env).display()
        )),
        "{}",
        stderr(&out)
    );
    assert!(!ran.exists());
}

#[test]
fn a_write_started_while_the_command_runs_waits_for_it() {
    let env = TestEnv::fixture();
    let Some(sleep) = support::find_on_path("sleep") else {
        return;
    };
    std::os::unix::fs::symlink(sleep, env.bin.join("sleep")).unwrap();
    env.fake_tool(
        "hold",
        &format!(
            "{PROBE}\n\
             echo > \"$HOME/started\"\n\
             i=0\n\
             while [ ! -f \"$HOME/release\" ] && [ $i -lt 1000 ]; do sleep 0.01; i=$((i+1)); done"
        ),
    );
    let mut locked = Running(
        env.tasq_std()
            .args(["store", "lock", "--", "hold"])
            .spawn()
            .unwrap(),
    );
    wait_for(&env.home.join("started"));
    let mut write = Running(
        env.tasq_std()
            .args(["log", "3", "written while locked"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(write.0.try_wait().unwrap().is_none(), "the write waits");
    assert!(!env.read_task(3).contains("written while locked"));
    std::fs::write(env.home.join("release"), "").unwrap();
    assert!(locked.0.wait().unwrap().success());
    assert!(write.0.wait().unwrap().success());
    assert!(env.read_task(3).contains("written while locked"));
}

#[test]
fn opens_no_store_and_tells_no_plugin() {
    let env = TestEnv::fixture();
    env.fake_tool(
        "tasq-echo",
        &format!("{PROBE}\necho \"$TASQ_EVENT\" >> \"$HOME/events\""),
    );
    env.write_project_config("[plugins]\nenabled = [\"echo\", \"missing\"]\n");
    let ran = marker(&env);
    let out = env
        .tasq()
        .args(["store", "lock", "--", "marker"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    assert!(ran.exists());
    assert!(!env.home.join("events").exists());
}
