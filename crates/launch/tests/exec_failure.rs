//! A failed `exec` from the Claude and shell launchers, in a test binary of
//! its own with a single test.
//!
//! `Command::exec` with a custom environment points the process-wide
//! `environ` at a temporary array, and on failure restores it and frees the
//! array, all under the environment *read* lock. A `Command::spawn` on
//! another test thread can read that array meanwhile and fail with `Bad
//! address (os error 14)` (it broke insta's `cargo metadata` in CI), and two
//! failed execs at once can leave `environ` dangling. `exec` also changes
//! the process's directory before `execvp`. Cargo runs test binaries one at
//! a time, so here nothing else runs in the process.

use tasq_core::config::EnvStrategy;
use tasq_core::launch::{LaunchContext, LaunchError, Launcher};
use tasq_core::model::{Task, TaskId};
use tasq_launch::prompt::DEFAULT_TEMPLATE;
use tasq_launch::{ClaudeLauncher, ShellLauncher};

#[test]
fn failed_exec_is_reported_and_restores_the_directory() {
    let root = tempfile::tempdir().unwrap();
    let ctx = LaunchContext {
        task: Task::new(TaskId::from(3), "T"),
        file: "/nb/3.todo.md".into(),
        markdown: "# [ ] T\n".into(),
        workdir: root.path().to_path_buf(),
        in_worktree: false,
        env: vec![("TASQ_TASK_ID".into(), "3".into())],
        statuses: vec!["ready".into()],
        focus: true,
    };
    let before = std::env::current_dir().unwrap();

    // No `claude` on an empty PATH: exec fails and returns, so the error
    // is observable (a real launch never returns here).
    let claude = ClaudeLauncher {
        env: vec![("PATH".into(), root.path().display().to_string())],
        strategy: EnvStrategy::Inherit,
        template: DEFAULT_TEMPLATE.to_owned(),
        in_herdr: false,
    };
    let err = claude.launch(&ctx).unwrap_err();
    assert!(
        matches!(err, LaunchError::Tool { ref tool, .. } if tool == "claude"),
        "{err}"
    );
    // The failed exec must not leave this process in the workdir, which
    // is deleted with the temp dir under every later child process.
    assert_eq!(std::env::current_dir().unwrap(), before);

    let shell = ShellLauncher {
        env: vec![("SHELL".into(), "/nonexistent/shell".into())],
    };
    let err = shell.launch(&ctx).unwrap_err();
    assert!(
        matches!(err, LaunchError::Tool { ref tool, .. } if tool == "/nonexistent/shell"),
        "{err}"
    );
    assert_eq!(std::env::current_dir().unwrap(), before);
}
