//! The `tasq-git` plugin (ADR 0030, `ruli/features/git-sync/PLAN.md`) run by
//! a real `tasq` against the fixture store. The plugin binary is the one
//! `cargo test --workspace` builds next to `tasq`. This crate cannot make
//! cargo build it, so `cargo test -p tasq` alone (and a cargo-mutants
//! baseline of this package) skips these tests with a note.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use std::path::Path;

use support::{TestEnv, stderr, stdout};

/// Puts the built `tasq-git` on the test's `PATH` and enables it. `false`,
/// with a note, when it was not built.
#[allow(
    clippy::print_stderr,
    reason = "the note says why the test did nothing"
)]
fn enable_tasq_git(env: &TestEnv) -> bool {
    let plugin = Path::new(env!("CARGO_BIN_EXE_tasq")).with_file_name("tasq-git");
    if !plugin.is_file() {
        eprintln!(
            "skipping: {} is not built (cargo test --workspace builds it)",
            plugin.display()
        );
        return false;
    }
    std::os::unix::fs::symlink(plugin, env.bin.join("tasq-git")).unwrap();
    env.write_global_config("[plugins]\nenabled = [\"git\"]\n");
    true
}

#[test]
fn doctor_says_store_sync_is_off_for_a_plain_store() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    let out = env.tasq().arg("doctor").output().unwrap();
    assert_eq!(stderr(&out), "");
    let text = env.normalize(&stdout(&out));
    assert!(
        text.contains(
            "\nOK    git/sync     store sync off: [ROOT]/home/.local/share/tasq/stores/default \
             is not a git repository\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("\nOK    tasq-git     plugin git is "),
        "{text}"
    );
}

#[test]
fn commands_on_a_plain_store_are_unchanged() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    for args in [&["list"][..], &["log", "3", "a note"]] {
        let out = env.tasq().args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stderr(&out), "", "{args:?}");
    }
    assert!(
        !env.store().join(".tasq").join("plugins").exists(),
        "the plugin kept state for a plain store"
    );
}

#[test]
fn git_init_makes_the_fixture_store_a_repository() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    env.write_global_config(
        "[plugins]\nenabled = [\"git\"]\n\n[plugin.git]\nname = \"Me Personal\"\nssh_key = \"/keys/me\"\n",
    );
    let out = env
        .tasq()
        .args(["git", "init", "--email", "me@example.invalid"])
        .output()
        .unwrap();
    assert_eq!(stderr(&out), "");
    assert!(out.status.success());
    assert_eq!(
        env.normalize(&stdout(&out)),
        "initialized [ROOT]/home/.local/share/tasq/stores/default as a git repository on \
         branch default: 5 task files committed\n"
    );
    let store = env.store();
    let files = env.git(&store, &["ls-tree", "-r", "--name-only", "HEAD"]);
    let expected: Vec<String> = env
        .task_files()
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.lines().collect::<Vec<_>>(), expected);
    assert_eq!(
        env.git(&store, &["log", "--format=%an <%ae>"]),
        "Me Personal <me@example.invalid>\n"
    );
    assert_eq!(
        env.git(&store, &["config", "--local", "core.sshCommand"]),
        "ssh -i '/keys/me' -o IdentitiesOnly=yes\n"
    );
}

#[test]
fn a_write_is_committed_in_the_background() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    let out = env.tasq().args(["git", "init"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let out = env.tasq().args(["log", "3", "a note"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");

    let store = env.store();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let subject = env.git(&store, &["log", "-1", "--format=%s"]);
        if subject == "log 3: Answer the support ticket\n" {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no commit for the write; last subject: {subject}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        env.git(&store, &["show", "--name-only", "--format=", "HEAD"]),
        format!(
            "{}\n",
            env.file_of(3).file_name().unwrap().to_string_lossy()
        )
    );
}

/// `tasq git init <url>` on the fixture store, with a bare remote, and a
/// clone of it standing for another machine; the clone's directory.
fn with_another_machine(env: &TestEnv) -> std::path::PathBuf {
    let root = env.root.path().canonicalize().unwrap();
    env.git(
        &root,
        &["init", "-q", "--bare", "-b", "default", "remote.git"],
    );
    let url = format!("file://{}", root.join("remote.git").display());
    let out = env
        .tasq()
        .args([
            "git",
            "init",
            &url,
            "--name",
            "Me",
            "--email",
            "me@example.invalid",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let other = root.join("other");
    env.git(
        &root,
        &[
            "clone",
            "-q",
            "--branch",
            "default",
            &url,
            other.to_str().unwrap(),
        ],
    );
    env.git(&other, &["config", "user.name", "Me"]);
    env.git(&other, &["config", "user.email", "me@example.invalid"]);
    other
}

/// Polls `check` every 20 ms until it holds, for at most ten seconds.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !check() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn another_machines_write_is_pulled_when_the_store_opens() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    let other = with_another_machine(&env);

    // Another machine edits task 3 and pushes.
    let name = env.file_of(3).file_name().unwrap().to_owned();
    let text = std::fs::read_to_string(other.join(&name)).unwrap();
    std::fs::write(
        other.join(&name),
        format!("{text}- from the other machine\n"),
    )
    .unwrap();
    env.git(&other, &["commit", "-q", "-am", "elsewhere"]);
    env.git(&other, &["push", "-q"]);
    let pushed = env.git(&other, &["rev-parse", "HEAD"]);

    // Never fetched: opening the store starts the job, which rebases under
    // the real `tasq store lock`.
    let out = env.tasq().arg("list").output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    let store = env.store();
    eventually("the other machine's commit to arrive", || {
        env.git(&store, &["rev-parse", "HEAD"]) == pushed
    });
    assert!(
        std::fs::read_to_string(env.file_of(3))
            .unwrap()
            .ends_with("- from the other machine\n")
    );
}

#[test]
fn an_id_pulled_in_twice_is_a_lasting_warning_until_fixed_by_hand() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    let other = with_another_machine(&env);

    // Another machine created a task offline and gave it id 3 too.
    let name = "01MA000000000000000000000Z.md";
    std::fs::write(other.join(name), "---\nid: 3\n---\n# Elsewhere\n").unwrap();
    env.git(&other, &["add", name]);
    env.git(&other, &["commit", "-q", "-m", "create 3: Elsewhere"]);
    env.git(&other, &["push", "-q"]);

    let out = env.tasq().arg("list").output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let warnings = env.store().join(".tasq/plugins/git/warnings");
    eventually("the duplicate id warning", || warnings.exists());
    let text = std::fs::read_to_string(&warnings).unwrap();
    let ours = env.store().join(name);
    assert!(
        text.starts_with("duplicate id: id 3 is claimed by more than one file: "),
        "{text}"
    );
    assert!(
        text.ends_with(&format!(
            "(created 2026-12-16 17:56, \"Elsewhere\", newer); fix: give the newer file the \
             next free id: write 'id: 8' in {} (or remove it if it is a copy)\n",
            ours.display()
        )),
        "{text}"
    );
    let out = env.tasq().arg("list").output().unwrap();
    assert!(
        stderr(&out).contains("tasq: warning: git: duplicate id: id 3 is claimed"),
        "{}",
        stderr(&out)
    );

    // The fix the warning gives, then any write: its job clears the warning.
    std::fs::write(&ours, "---\nid: 8\n---\n# Elsewhere\n").unwrap();
    let out = env
        .tasq()
        .args(["log", "1", "renumbered 8"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    eventually("the warning to clear", || !warnings.exists());
}

#[test]
fn git_sync_prints_each_step_and_doctor_explains_the_repository() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    let other = with_another_machine(&env);
    let name = env.file_of(4).file_name().unwrap().to_owned();
    let text = std::fs::read_to_string(other.join(&name)).unwrap();
    std::fs::write(other.join(&name), format!("{text}- elsewhere\n")).unwrap();
    env.git(&other, &["commit", "-q", "-am", "elsewhere"]);
    env.git(&other, &["push", "-q"]);

    let out = env.tasq().args(["git", "sync"]).output().unwrap();
    assert_eq!(stderr(&out), "");
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "nothing to commit\nfetched origin/default\npulled 1 commit from origin/default\n"
    );
    assert_eq!(
        env.git(&env.store(), &["rev-parse", "HEAD"]),
        env.git(&other, &["rev-parse", "HEAD"])
    );

    let out = env.tasq().args(["doctor", "--json"]).output().unwrap();
    let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let checks: Vec<(String, String)> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"].as_str().unwrap().starts_with("git/"))
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_owned(),
                c["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected = ["identity", "remote", "push", "fetch", "rebase"]
        .map(|name| (format!("git/{name}"), "ok".to_owned()));
    assert_eq!(checks, expected, "{doc}");
}

#[test]
fn git_clone_fills_another_store() {
    let env = TestEnv::fixture();
    if !enable_tasq_git(&env) {
        return;
    }
    with_another_machine(&env);
    let root = env.root.path().canonicalize().unwrap();
    let url = format!("file://{}", root.join("remote.git").display());
    let out = env
        .tasq()
        .args([
            "git", "clone", &url, "--store", "work", "--branch", "default",
        ])
        .output()
        .unwrap();
    assert_eq!(stderr(&out), "");
    assert!(out.status.success());
    assert_eq!(
        env.normalize(&stdout(&out)),
        format!(
            "cloned branch default of {} into [ROOT]/home/.local/share/tasq/stores/work: 5 \
             task files\n",
            env.normalize(&url)
        )
    );
    let mine = env.tasq().args(["list", "--json"]).output().unwrap();
    let work = env
        .tasq()
        .args(["--set", "store.name=work", "list", "--json"])
        .output()
        .unwrap();
    assert!(work.status.success(), "{}", stderr(&work));
    assert_eq!(stdout(&work), stdout(&mine));

    // A second clone into the filled store is refused.
    let out = env
        .tasq()
        .args([
            "git", "clone", &url, "--store", "work", "--branch", "default",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("is already a git repository"),
        "{}",
        stderr(&out)
    );
}
