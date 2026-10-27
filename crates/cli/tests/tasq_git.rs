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
    env.write_project_config("[plugins]\nenabled = [\"git\"]\n");
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
    env.write_global_config("[plugin.git]\nname = \"Me Personal\"\nssh_key = \"/keys/me\"\n");
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
