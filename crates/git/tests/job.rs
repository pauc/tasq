//! The background job that commits a store's writes (GS-003), driven the
//! way tasq drives it: `tasq-git hook` gets the `post-write` payload and
//! detaches `tasq-git job --store <dir>`. Most tests run that job in the
//! foreground and wait for it; one proves the hook does not wait for it.
//!
//! The store is a real repository under a `HOME` the test owns. The `tasq`
//! the job asks for titles is a fake answering `view <id> --json` from
//! `titles/<id>`; before answering it runs `on-view` once, if present,
//! which is how a test makes a write happen while the job runs.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

use tempfile::TempDir;

mod support;

const TASK_3: &str = "01K6Q8ZT3D0000000000000003.md";
const TASK_4: &str = "01K6Q8ZT3D0000000000000004.md";

/// A temp root holding `home`, `bin` (the fake `tasq`), the titles and a
/// store repository with tasks 3 and 4 in its first commit.
struct Env {
    root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    store: PathBuf,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let home = base.join("home");
        let bin = base.join("bin");
        let store = base.join("data").join("stores").join("default");
        for dir in [&home, &bin, &store, &base.join("titles")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let env = Self {
            root,
            home,
            bin,
            store,
        };
        let script = format!(
            "#!/bin/sh\n\
             base='{base}'\n\
             [ \"$1 $3\" = 'view --json' ] || {{ echo \"fake tasq: $*\" >&2; exit 2; }}\n\
             printf '%s|%s|%s\\n' \"$(pwd)\" \"$TASQ_DATA_DIR\" \"$TASQ_STORE_NAME\" >> \"$base/calls\"\n\
             if [ -f \"$base/on-view\" ]; then\n\
               mv \"$base/on-view\" \"$base/on-view.run\"\n\
               sh \"$base/on-view.run\"\n\
             fi\n\
             [ -f \"$base/titles/$2\" ] || {{ echo \"no task $2\" >&2; exit 1; }}\n\
             IFS= read -r title < \"$base/titles/$2\"\n\
             printf '{{\"schema\":1,\"task\":{{\"id\":\"%s\",\"title\":\"%s\"}}}}\\n' \"$2\" \"$title\"\n",
            base = base.display()
        );
        install(&env.bin.join("tasq"), &script);
        env.title("3", "Three");
        env.title("4", "Four");
        env.write(TASK_3, "---\nid: 3\n---\n# Three\n");
        env.write(TASK_4, "---\nid: 4\n---\n# Four\n");
        env.git(&["init", "-q", "-b", "default"]);
        env.git(&["config", "user.name", "Me Personal"]);
        env.git(&["config", "user.email", "me@example.invalid"]);
        env.git(&["add", "-A"]);
        env.git(&["commit", "-q", "-m", "start"]);
        env
    }

    fn base(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    fn title(&self, id: &str, title: &str) {
        std::fs::write(self.base().join("titles").join(id), format!("{title}\n")).unwrap();
    }

    /// Writes `text` to `path` under the store, creating directories.
    fn write(&self, path: &str, text: &str) {
        let path = self.store.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Appends a progress line to a task file: a write tasq would make.
    fn touch_task(&self, file: &str) {
        let path = self.store.join(file);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("- a note\n");
        std::fs::write(path, text).unwrap();
    }

    fn state(&self, file: &str) -> PathBuf {
        self.store.join(".tasq/plugins/git").join(file)
    }

    /// `job.lock` held as a running job holds it, until the file is
    /// dropped.
    fn hold_job_lock(&self) -> std::fs::File {
        let path = self.state("job.lock");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        file.lock().unwrap();
        file
    }

    /// Whether a job holds `job.lock`.
    fn job_running(&self) -> bool {
        std::fs::File::open(self.state("job.lock")).is_ok_and(|file| file.try_lock().is_err())
    }

    /// A `git` first on `PATH` that runs `body` for a command line
    /// matching the `case` pattern `when`, then the real git.
    fn wrap_git(&self, when: &str, body: &str) {
        install(
            &self.bin.join("git"),
            &format!(
                "#!/bin/sh\ncase \" $* \" in\n  {when}) {body} ;;\nesac\nexec '{}' \"$@\"\n",
                real_git().display()
            ),
        );
    }

    fn unwrap_git(&self) {
        std::fs::remove_file(self.bin.join("git")).unwrap();
    }

    /// The `post-write` payload for `command` writing `ids`.
    fn payload(&self, command: &str, ids: &[&str]) -> String {
        let writes: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({"id": id, "file": self.store.join("x.md"), "kind": "update"}))
            .collect();
        serde_json::json!({
            "schema": 1,
            "event": "post-write",
            "store": {"name": "default", "location": self.store},
            "config": {},
            "command": command,
            "writes": writes,
        })
        .to_string()
    }

    /// `tasq-git hook` with `payload` on stdin, as tasq runs it.
    fn hook(&self, payload: &str) -> Output {
        let mut child = self
            .command(&["hook"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// Tells the plugin about writes while a job holds `job.lock`, so that
    /// the hook records them and detaches nothing; the lock is gone after.
    fn record(&self, writes: &[(&str, &[&str])]) {
        let lock = self.hold_job_lock();
        for (command, ids) in writes {
            assert_silent(&self.hook(&self.payload(command, ids)));
        }
        drop(lock);
    }

    /// `tasq-git job --store <store>` in the foreground.
    fn job(&self) -> Output {
        self.job_with(&[])
    }

    fn job_with(&self, vars: &[(&str, &str)]) -> Output {
        let store = self.store.to_str().unwrap().to_owned();
        let mut cmd = self.command(&["job", "--store", &store]);
        cmd.envs(vars.iter().copied());
        cmd.output().unwrap()
    }

    /// `tasq-git <args>` with `TASQ_BIN`, `HOME` and `PATH` only.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tasq-git"));
        cmd.args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", self.path())
            .env("TASQ_BIN", self.bin.join("tasq"))
            .current_dir(&self.home);
        support::forward_profile(&mut cmd);
        cmd
    }

    fn path(&self) -> OsString {
        let mut dirs = vec![self.bin.clone()];
        dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        std::env::join_paths(dirs).unwrap()
    }

    /// `git <args>` in the store; panics on failure.
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.store)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", self.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// The full messages of the commits after `start`, oldest first.
    fn messages(&self) -> Vec<String> {
        let log = self.git(&["log", "--reverse", "--format=%B%x00", "HEAD"]);
        log.split('\0')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .skip(1)
            .map(str::to_owned)
            .collect()
    }

    /// `git show --name-status` of `rev`: one `<status>\t<file>` per line.
    fn changed(&self, rev: &str) -> String {
        self.git(&["show", "--name-status", "--format=", rev])
    }

    fn warnings(&self) -> Option<String> {
        std::fs::read_to_string(self.state("warnings")).ok()
    }
}

/// The real `git`: the first on the test's own `PATH`.
fn real_git() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .unwrap()
}

/// Writes an executable script, waiting until it can run (ETXTBSY while
/// another test thread forks with the file still open for writing).
fn install(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..200 {
        match Command::new(path).arg("probe").output() {
            Err(err) if err.raw_os_error() == Some(26) => {
                std::thread::sleep(Duration::from_millis(2));
            }
            _ => return,
        }
    }
    panic!("{} never became runnable", path.display());
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// Exit 0, nothing printed.
fn assert_silent(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{}", stderr(out));
    assert_eq!(stdout(out), "");
    assert_eq!(stderr(out), "");
}

/// Polls `check` every 20 ms until it holds, for at most ten seconds.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

mod commit {
    use super::*;

    #[test]
    fn one_write_is_one_commit_with_its_subject() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["log 3: Three"]);
        assert_eq!(env.changed("HEAD"), format!("M\t{TASK_3}\n"));
        assert_eq!(env.git(&["status", "--porcelain", "--", "*.md"]), "");
        assert!(!env.job_running(), "the job left its lock held");
    }

    #[test]
    fn batched_writes_are_one_commit_listing_each() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.touch_task(TASK_4);
        env.record(&[("log", &["3"]), ("set", &["4"]), ("move", &["4", "3"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["log 3: Three\n\nlog 3\nset 4\nmove 4 3"]);
    }

    #[test]
    fn the_subject_is_the_first_write_and_its_first_id() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.touch_task(TASK_4);
        env.record(&[("move", &["4", "3"]), ("log", &["3"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["move 4 3: Four\n\nmove 4 3\nlog 3"]);
    }

    #[test]
    fn a_title_tasq_cannot_give_leaves_the_subject_bare() {
        let env = Env::new();
        env.write("01K6Q8ZT3D0000000000000009.md", "---\nid: 9\n---\n# Nine\n");
        env.record(&[("create", &["9"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["create 9"]);
    }

    #[test]
    fn a_change_nobody_reported_is_committed_too() {
        let env = Env::new();
        env.touch_task(TASK_3);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["update task files"]);
    }

    #[test]
    fn nothing_changed_is_no_commit_and_forgets_the_writes() {
        let env = Env::new();
        env.record(&[("log", &["3"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), Vec::<String>::new());

        env.touch_task(TASK_4);
        env.record(&[("set", &["4"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["set 4: Four"]);
    }

    #[test]
    fn a_second_job_commits_only_the_new_writes() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        assert_silent(&env.job());
        env.touch_task(TASK_4);
        env.record(&[("set", &["4"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["log 3: Three", "set 4: Four"]);
    }

    #[test]
    fn only_task_files_are_committed() {
        let env = Env::new();
        env.touch_task(TASK_3);
        std::fs::remove_file(env.store.join(TASK_4)).unwrap();
        env.write("01K6Q8ZT3D0000000000000006.MD", "---\nid: 6\n---\n# Six\n");
        env.write(".tasq/lock", "");
        env.write("notes.txt", "mine\n");
        env.write("sub/01K6Q8ZT3D0000000000000005.md", "---\nid: 5\n---\n");
        env.write(".#01K6Q8ZT3D0000000000000003.md", "an editor lock\n");
        env.record(&[("log", &["3"])]);
        assert_silent(&env.job());
        assert_eq!(
            env.changed("HEAD"),
            format!("M\t{TASK_3}\nD\t{TASK_4}\nA\t01K6Q8ZT3D0000000000000006.MD\n")
        );
        let status = env.git(&["status", "--porcelain", "--untracked-files=all"]);
        let left: Vec<_> = status
            .lines()
            .filter(|line| !line.starts_with("?? .tasq/plugins/"))
            .collect();
        assert_eq!(
            left,
            [
                "?? .#01K6Q8ZT3D0000000000000003.md",
                "?? .tasq/lock",
                "?? notes.txt",
                "?? sub/01K6Q8ZT3D0000000000000005.md",
            ]
        );
    }

    #[test]
    fn git_variables_of_the_environment_are_ignored() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        let elsewhere = env.base().join("elsewhere");
        let out = env.job_with(&[
            ("GIT_AUTHOR_NAME", "Work Me"),
            ("GIT_AUTHOR_EMAIL", "me@work.invalid"),
            ("GIT_COMMITTER_NAME", "Work Me"),
            ("GIT_COMMITTER_EMAIL", "me@work.invalid"),
            ("GIT_DIR", elsewhere.to_str().unwrap()),
            ("GIT_INDEX_FILE", elsewhere.to_str().unwrap()),
        ]);
        assert_silent(&out);
        assert_eq!(
            env.git(&["log", "-1", "--format=%an <%ae>|%cn <%ce>|%s"]),
            "Me Personal <me@example.invalid>|Me Personal <me@example.invalid>|log 3: Three\n"
        );
        assert!(!elsewhere.exists());
    }

    #[test]
    fn the_users_signing_and_hooks_are_not_applied() {
        let env = Env::new();
        let hooks = env.home.join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        install(
            &hooks.join("pre-commit"),
            "#!/bin/sh\necho global hook >&2\nexit 1\n",
        );
        install(
            &env.bin.join("no-gpg"),
            "#!/bin/sh\necho no pinentry >&2\nexit 2\n",
        );
        std::fs::write(
            env.home.join(".gitconfig"),
            format!(
                "[commit]\n\tgpgSign = true\n[gpg]\n\tprogram = {}\n[core]\n\thooksPath = {}\n",
                env.bin.join("no-gpg").display(),
                hooks.display()
            ),
        )
        .unwrap();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["log 3: Three"]);
        assert_eq!(env.git(&["log", "-1", "--format=%G?"]), "N\n");
        assert_eq!(env.warnings(), None);
    }

    #[test]
    fn the_identity_may_come_from_xdg_config_home() {
        let env = Env::new();
        env.git(&["config", "--unset", "user.name"]);
        env.git(&["config", "--unset", "user.email"]);
        std::fs::write(
            env.home.join(".gitconfig"),
            "[user]\n\tuseConfigOnly = true\n",
        )
        .unwrap();
        let xdg = env.base().join("xdg");
        std::fs::create_dir_all(xdg.join("git")).unwrap();
        std::fs::write(
            xdg.join("git/config"),
            "[user]\n\tname = Xdg Me\n\temail = xdg@example.invalid\n",
        )
        .unwrap();
        env.touch_task(TASK_3);
        assert_silent(&env.job_with(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]));
        assert_eq!(
            env.git(&["log", "-1", "--format=%an <%ae>"]),
            "Xdg Me <xdg@example.invalid>\n"
        );
    }

    #[test]
    fn a_job_whose_directory_is_gone_asks_tasq_from_the_store() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        // `tasq done` in the worktree it removes: the hook, and the job it
        // detaches, start in a directory that no longer exists.
        let gone = env.base().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "cd \"$1\" && rmdir \"$1\" && exec \"$2\" job --store \"$3\"",
            "sh",
        ])
        .arg(&gone)
        .arg(env!("CARGO_BIN_EXE_tasq-git"))
        .arg(&env.store)
        .env_clear()
        .env("HOME", &env.home)
        .env("PATH", env.path())
        .env("TASQ_BIN", env.bin.join("tasq"))
        .current_dir(env.base());
        let out = support::forward_profile(&mut cmd).output().unwrap();
        assert_silent(&out);
        assert_eq!(env.messages(), ["log 3: Three"]);
        assert_eq!(
            std::fs::read_to_string(env.base().join("calls")).unwrap(),
            format!(
                "{}|{}|default\n",
                env.store.display(),
                env.store.parent().unwrap().display()
            )
        );
    }

    #[test]
    fn a_store_that_is_not_a_repository_is_left_alone() {
        let env = Env::new();
        std::fs::remove_dir_all(env.store.join(".git")).unwrap();
        env.touch_task(TASK_3);
        assert_silent(&env.job());
        assert!(!env.store.join(".git").exists());
        assert!(!env.state("job.lock").exists());
    }
}

mod running {
    use super::*;

    #[test]
    fn a_write_while_the_job_runs_is_committed_before_it_exits() {
        let env = Env::new();
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);
        // While the job asks for task 3's title, task 4 is written and the
        // plugin told, as tasq would: the hook finds the job running.
        let payload = env.base().join("payload.json");
        std::fs::write(&payload, env.payload("set", &["4"])).unwrap();
        let on_view = format!(
            "printf -- '- a note\\n' >> '{task}'\n\
             '{plugin}' hook < '{payload}' || exit 1\n",
            task = env.store.join(TASK_4).display(),
            plugin = env!("CARGO_BIN_EXE_tasq-git"),
            payload = payload.display(),
        );
        std::fs::write(env.base().join("on-view"), on_view).unwrap();
        assert_silent(&env.job());
        assert!(!env.base().join("on-view").exists(), "on-view never ran");
        assert_eq!(env.messages(), ["log 3: Three", "set 4: Four"]);
        assert_eq!(env.git(&["status", "--porcelain", "--", "*.md"]), "");
        assert!(!env.state("pending").exists());
        assert!(!env.job_running());
    }

    #[test]
    fn a_held_job_lock_means_another_job_is_running() {
        let env = Env::new();
        env.touch_task(TASK_3);
        let lock = env.hold_job_lock();
        // However old: a job holds the lock for as long as it lives.
        lock.set_modified(SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        assert_silent(&env.job());
        assert_eq!(env.messages(), Vec::<String>::new());
        drop(lock);
    }

    #[test]
    fn a_job_lock_nobody_holds_is_a_dead_jobs() {
        let env = Env::new();
        env.touch_task(TASK_3);
        drop(env.hold_job_lock());
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["update task files"]);
    }
}

mod failure {
    use super::*;

    /// A `git commit` that fails with "nope" and "twice".
    fn failing_commits(env: &Env) {
        env.wrap_git("*' commit '*", "echo nope >&2; echo twice >&2; exit 1");
    }

    #[test]
    fn a_failed_commit_is_a_lasting_warning_cleared_by_the_next_success() {
        let env = Env::new();
        std::fs::create_dir_all(env.state("")).unwrap();
        std::fs::write(env.state("warnings"), "push failed: offline\n").unwrap();
        failing_commits(&env);
        env.touch_task(TASK_3);
        env.record(&[("log", &["3"])]);

        let out = env.job();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(stderr(&out), "tasq-git: commit failed: nope; twice\n");
        assert_eq!(
            env.warnings().as_deref(),
            Some("push failed: offline\ncommit failed: nope; twice\n")
        );
        assert_eq!(env.messages(), Vec::<String>::new());
        assert!(!env.job_running());

        env.unwrap_git();
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["log 3: Three"]);
        assert_eq!(env.warnings().as_deref(), Some("push failed: offline\n"));
    }

    #[test]
    fn the_warnings_file_goes_when_its_last_line_does() {
        let env = Env::new();
        failing_commits(&env);
        env.touch_task(TASK_3);
        assert_eq!(env.job().status.code(), Some(1));
        assert_eq!(
            env.warnings().as_deref(),
            Some("commit failed: nope; twice\n")
        );
        env.unwrap_git();
        assert_silent(&env.job());
        assert_eq!(env.warnings(), None);
    }
}

mod hook {
    use super::*;

    #[test]
    fn post_write_detaches_the_job_and_returns() {
        let env = Env::new();
        env.touch_task(TASK_3);
        // The job waits in its title lookup until the test says go, so the
        // hook can only have returned without waiting for it.
        let go = env.base().join("go");
        let on_view = format!(
            "i=0\n\
             while [ ! -f '{go}' ] && [ $i -lt 500 ]; do sleep 0.02; i=$((i+1)); done\n",
            go = go.display()
        );
        std::fs::write(env.base().join("on-view"), on_view).unwrap();

        assert_silent(&env.hook(&env.payload("log", &["3"])));
        eventually("the job to start", || {
            env.base().join("on-view.run").exists()
        });
        assert_eq!(env.messages(), Vec::<String>::new());
        assert!(env.job_running());

        std::fs::write(&go, "").unwrap();
        eventually("the commit", || env.messages() == ["log 3: Three"]);
        eventually("the job to exit", || !env.job_running());
    }

    #[test]
    fn a_post_write_with_a_bad_interval_still_commits() {
        let env = Env::new();
        env.touch_task(TASK_3);
        let mut payload: serde_json::Value =
            serde_json::from_str(&env.payload("log", &["3"])).unwrap();
        payload["config"] = serde_json::json!({"interval": "10m"});
        let out = env.hook(&payload.to_string());
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "[plugin.git] interval must be a whole number of seconds, not \"10m\"\n"
        );
        eventually("the commit", || env.messages() == ["log 3: Three"]);
        eventually("the job to exit", || !env.job_running());
    }

    #[test]
    fn post_write_records_the_writes_while_a_job_runs() {
        let env = Env::new();
        env.touch_task(TASK_3);
        let lock = env.hold_job_lock();
        assert_silent(&env.hook(&env.payload("move", &["3", "4"])));
        assert_silent(&env.hook(&env.payload("log", &["3"])));
        assert!(env.state("pending").exists());
        // No job was started: nothing waits on the held lock to commit.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(env.messages(), Vec::<String>::new());
        drop(lock);
        assert_silent(&env.job());
        assert_eq!(env.messages(), ["move 3 4: Three\n\nmove 3 4\nlog 3"]);
    }

    #[test]
    fn a_post_write_without_a_command_is_an_error() {
        let env = Env::new();
        let mut payload: serde_json::Value =
            serde_json::from_str(&env.payload("log", &["3"])).unwrap();
        payload.as_object_mut().unwrap().remove("command");
        let out = env.hook(&payload.to_string());
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "the post-write event payload has no command\n"
        );
        assert!(!env.store.join(".tasq").exists());
    }

    #[test]
    fn a_post_write_without_writes_is_an_error() {
        let env = Env::new();
        let mut payload: serde_json::Value =
            serde_json::from_str(&env.payload("log", &["3"])).unwrap();
        payload.as_object_mut().unwrap().remove("writes");
        let out = env.hook(&payload.to_string());
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stderr(&out), "the post-write event payload has no writes\n");
    }
}
