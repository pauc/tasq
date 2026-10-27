//! Push, fetch and rebase in the background (GS-004): two machines, each a
//! store repository cloned from one bare remote (a `file://` path), driven
//! the way tasq drives the plugin: `tasq-git hook` with a payload, or the
//! job it detaches run in the foreground.
//!
//! The `tasq` the plugin calls is a fake: `view <id> --json` answers the
//! titles, and `store lock -- <cmd>` notes the command in `locked` and
//! runs it (after `on-lock`, once, if present). Every fetch of machine B
//! goes through an upload-pack wrapper that counts it in `fetches`.
//!
//! Conflicts (GS-005): both machines edit one task.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

use tempfile::TempDir;

const TASK_3: &str = "01K6Q8ZT3D0000000000000003.md";
const TASK_4: &str = "01K6Q8ZT3D0000000000000004.md";

/// A temp root holding `home`, `bin` (the fake `tasq`) and the bare
/// remote `remote.git`, with branch `default` holding tasks 3 and 4.
struct World {
    root: TempDir,
}

impl World {
    fn new() -> Self {
        let world = Self {
            root: tempfile::tempdir().unwrap(),
        };
        let base = world.base();
        for dir in ["home", "bin", "titles"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        let script = format!(
            "#!/bin/sh\n\
             base='{base}'\n\
             if [ \"$1 $2 $3\" = 'store lock --' ]; then\n\
               shift 3\n\
               printf '%s\\n' \"$*\" >> \"$base/locked\"\n\
               if [ -f \"$base/on-lock\" ]; then\n\
                 mv \"$base/on-lock\" \"$base/on-lock.run\"\n\
                 sh \"$base/on-lock.run\"\n\
               fi\n\
               exec \"$@\"\n\
             fi\n\
             [ \"$1 $3\" = 'view --json' ] || {{ echo \"fake tasq: $*\" >&2; exit 2; }}\n\
             [ -f \"$base/titles/$2\" ] || {{ echo \"no task $2\" >&2; exit 1; }}\n\
             IFS= read -r title < \"$base/titles/$2\"\n\
             printf '{{\"schema\":1,\"task\":{{\"id\":\"%s\",\"title\":\"%s\"}}}}\\n' \"$2\" \"$title\"\n",
            base = base.display()
        );
        install(&base.join("bin/tasq"), &script);
        install(
            &base.join("bin/counting-upload-pack"),
            &format!(
                "#!/bin/sh\necho fetch >> '{}'\nexec git-upload-pack \"$@\"\n",
                base.join("fetches").display()
            ),
        );
        std::fs::write(base.join("titles/3"), "Three\n").unwrap();
        std::fs::write(base.join("titles/4"), "Four\n").unwrap();
        world.run_git(
            &base,
            &["init", "-q", "--bare", "-b", "default", "remote.git"],
        );
        world
    }

    fn base(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    fn url(&self) -> String {
        format!("file://{}", self.base().join("remote.git").display())
    }

    /// Machine A: a store with tasks 3 and 4 in its first commit, pushed
    /// to the remote's `default` with an upstream.
    fn first(&self) -> Machine<'_> {
        let a = self.machine("a");
        std::fs::create_dir_all(&a.store).unwrap();
        a.write(TASK_3, "---\nid: 3\n---\n# Three\n");
        a.write(TASK_4, "---\nid: 4\n---\n# Four\n");
        a.git(&["init", "-q", "-b", "default"]);
        a.identity();
        a.git(&["add", "-A"]);
        a.git(&["commit", "-q", "-m", "start"]);
        a.git(&["remote", "add", "origin", &self.url()]);
        a.git(&["push", "-q", "--set-upstream", "origin", "default"]);
        a
    }

    /// Machine B: a clone of the remote's `default`, its fetches counted.
    fn second(&self) -> Machine<'_> {
        let b = self.machine("b");
        let store = b.store.to_str().unwrap();
        self.run_git(
            &self.base(),
            &["clone", "-q", "--branch", "default", &self.url(), store],
        );
        b.identity();
        let wrapper = self.base().join("bin/counting-upload-pack");
        b.git(&[
            "config",
            "remote.origin.uploadpack",
            wrapper.to_str().unwrap(),
        ]);
        b
    }

    fn machine(&self, name: &str) -> Machine<'_> {
        Machine {
            world: self,
            store: self.base().join(name).join("stores").join("default"),
        }
    }

    fn home(&self) -> PathBuf {
        self.base().join("home")
    }

    fn path(&self) -> OsString {
        let mut dirs = vec![self.base().join("bin")];
        dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        std::env::join_paths(dirs).unwrap()
    }

    /// `git <args>` in `dir`; panics on failure.
    fn run_git(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("HOME", self.home())
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

    /// The remote's `default`.
    fn remote_head(&self) -> String {
        let remote = self.base().join("remote.git");
        self.run_git(&remote, &["rev-parse", "default"])
    }

    /// How many fetches machine B made.
    fn fetches(&self) -> usize {
        std::fs::read_to_string(self.base().join("fetches")).map_or(0, |text| text.lines().count())
    }

    /// The commands run under `tasq store lock`, one per line.
    fn locked(&self) -> String {
        std::fs::read_to_string(self.base().join("locked")).unwrap_or_default()
    }

    /// A global `pre-push` hook running `script`.
    fn pre_push(&self, script: &str) {
        let hooks = self.home().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        install(&hooks.join("pre-push"), &format!("#!/bin/sh\n{script}"));
        std::fs::write(
            self.home().join(".gitconfig"),
            format!("[core]\n\thooksPath = {}\n", hooks.display()),
        )
        .unwrap();
    }
}

/// One store repository.
struct Machine<'w> {
    world: &'w World,
    store: PathBuf,
}

impl Machine<'_> {
    fn identity(&self) {
        self.git(&["config", "user.name", "Me Personal"]);
        self.git(&["config", "user.email", "me@example.invalid"]);
    }

    fn write(&self, file: &str, text: &str) {
        std::fs::write(self.store.join(file), text).unwrap();
    }

    fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.store.join(file)).unwrap()
    }

    /// Appends a progress line to a task file: a write tasq would make.
    fn note(&self, file: &str, note: &str) {
        let text = format!("{}- {note}\n", self.read(file));
        self.write(file, &text);
    }

    fn git(&self, args: &[&str]) -> String {
        self.world.run_git(&self.store, args)
    }

    /// `git <args>` in the store, whatever its outcome.
    fn try_git(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.store)
            .env_clear()
            .env("HOME", self.world.home())
            .env("PATH", self.world.path())
            .output()
            .unwrap()
    }

    /// Whether a rebase is stopped in the store repository.
    fn rebasing(&self) -> bool {
        self.store.join(".git/rebase-merge").exists()
            || self.store.join(".git/rebase-apply").exists()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// The subjects of the branch, newest first.
    fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn state(&self, file: &str) -> PathBuf {
        self.store.join(".tasq/plugins/git").join(file)
    }

    fn warnings(&self) -> Option<String> {
        std::fs::read_to_string(self.state("warnings")).ok()
    }

    /// `last-fetch` as the job writes it, `ago` before now.
    fn fetched(&self, ago: Duration) {
        let at = SystemTime::now() - ago;
        let secs = at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        std::fs::create_dir_all(self.state("")).unwrap();
        std::fs::write(self.state("last-fetch"), format!("{secs}\n")).unwrap();
    }

    /// `tasq-git job --store <store>` in the foreground, `args` after.
    fn job(&self, args: &[&str]) -> Output {
        let mut all = vec!["job", "--store", self.store.to_str().unwrap()];
        all.extend(args);
        self.command(&all).output().unwrap()
    }

    /// `tasq-git hook` with the `store-open` payload carrying `config`.
    fn store_open(&self, config: &serde_json::Value) -> Output {
        let payload = serde_json::json!({
            "schema": 1,
            "event": "store-open",
            "store": {"name": "default", "location": self.store},
            "config": config,
        });
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
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// `tasq-git <args>` with `TASQ_BIN`, `HOME` and `PATH` only.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tasq-git"));
        cmd.args(args)
            .env_clear()
            .env("HOME", self.world.home())
            .env("PATH", self.world.path())
            .env("TASQ_BIN", self.world.base().join("bin/tasq"))
            .current_dir(self.world.home());
        cmd
    }
}

/// Writes an executable script, waiting until it can run (ETXTBSY while
/// another test thread forks with the file still open for writing). The
/// probe run exits at once: a hook script must not act on it.
fn install(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let (shebang, body) = script.split_once('\n').unwrap();
    let script = format!("{shebang}\n[ \"$1\" = probe ] && exit 0\n{body}");
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

mod two_machines {
    use super::*;

    #[test]
    fn a_commit_is_pushed() {
        let world = World::new();
        let a = world.first();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        assert_eq!(a.subjects(), ["update task files", "start"]);
        assert_eq!(world.remote_head(), a.head());
        assert_eq!(world.locked(), "", "nothing to rebase, no lock");
        assert_eq!(a.warnings(), None);
    }

    #[test]
    fn writes_on_a_reach_b() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));

        // B wrote nothing; its first job fetches (no fetch yet) and rebases.
        assert_silent(&b.job(&[]));
        assert_eq!(b.head(), a.head());
        assert_eq!(b.read(TASK_3), a.read(TASK_3));
        assert_eq!(world.fetches(), 1);
        assert_eq!(
            world.locked(),
            format!(
                "{} rebase --store {}\n",
                env!("CARGO_BIN_EXE_tasq-git"),
                b.store.display()
            )
        );
        assert!(b.state("last-fetch").exists());
        assert!(!b.state("job.lock").exists());
    }

    #[test]
    fn interleaved_writes_on_different_tasks_converge() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        b.note(TASK_4, "on b");
        assert_silent(&a.job(&[]));
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), b.head());
        // A fetched a moment ago: only a due fetch brings B's write.
        assert_silent(&a.job(&[]));
        assert_ne!(a.head(), b.head());
        assert_silent(&a.job(&["--interval", "0"]));

        assert_eq!(a.head(), b.head());
        for task in [TASK_3, TASK_4] {
            assert_eq!(a.read(task), b.read(task), "{task}");
        }
        assert!(a.read(TASK_3).ends_with("- on a\n"));
        assert!(a.read(TASK_4).ends_with("- on b\n"));
        // Rebased, never merged.
        assert_eq!(b.git(&["rev-list", "--merges", "HEAD"]), "");
        assert_eq!(b.subjects().len(), 3);
        assert_eq!(a.warnings(), None);
        assert_eq!(b.warnings(), None);
    }

    #[test]
    fn a_rejected_push_is_fetched_rebased_and_pushed_again() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        a.git(&["commit", "-q", "-am", "from a"]);
        // B's first push races A's: A gets there between B's fetch and
        // B's push.
        world.pre_push(&format!(
            "[ -f '{done}' ] && exit 0\n: > '{done}'\ngit -C '{a}' push -q\n",
            done = world.base().join("raced").display(),
            a = a.store.display()
        ));
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));

        assert_eq!(world.remote_head(), b.head());
        assert_eq!(b.subjects(), ["update task files", "from a", "start"]);
        assert!(b.read(TASK_3).ends_with("- on a\n"));
        assert_eq!(world.fetches(), 2);
        assert_eq!(world.locked().lines().count(), 1);
        assert_eq!(b.warnings(), None);
    }

    #[test]
    fn a_write_made_before_the_store_lock_is_left_for_the_next_round() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        // B's write lands after its job committed and before the rebase
        // took the store lock; the rebase leaves it alone.
        std::fs::write(
            world.base().join("on-lock"),
            format!(
                "printf -- '- late\\n' >> '{}'\n",
                b.store.join(TASK_4).display()
            ),
        )
        .unwrap();
        b.note(TASK_4, "on b");
        let pushed = world.remote_head();
        assert_silent(&b.job(&[]));
        assert_eq!(world.locked().lines().count(), 1);
        assert_eq!(b.subjects(), ["update task files", "start"]);
        assert!(
            !b.read(TASK_3).contains("on a"),
            "rebased over an uncommitted write"
        );
        assert!(b.read(TASK_4).ends_with("- on b\n- late\n"));
        // Still behind: not pushed, and no push tried.
        assert_eq!(world.remote_head(), pushed);
        assert_eq!(b.warnings(), None);

        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), b.head());
        assert!(b.read(TASK_3).ends_with("- on a\n"));
        assert!(b.read(TASK_4).ends_with("- on b\n- late\n"));
        assert_eq!(b.git(&["status", "--porcelain", "--", "*.md"]), "");
    }

    #[test]
    fn a_long_round_keeps_its_job_lock() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        // By the rebase, B's round has run for three minutes; its push
        // must find the job lock fresh, not one another job would break.
        let stamp = |name: &str, ago: u64| {
            let path = world.base().join(name);
            let file = std::fs::File::create(&path).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(ago))
                .unwrap();
            path
        };
        let old = stamp("old", 180);
        let recent = stamp("recent", 60);
        let lock = b.state("job.lock");
        std::fs::write(
            world.base().join("on-lock"),
            format!("touch -r '{}' '{}'\n", old.display(), lock.display()),
        )
        .unwrap();
        world.pre_push(&format!(
            "[ -n \"$(find '{}' -newer '{}')\" ] || {{ echo 'the job lock went stale' >&2; exit 1; }}\n",
            lock.display(),
            recent.display()
        ));
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));
        assert!(
            world.base().join("on-lock.run").exists(),
            "on-lock never ran"
        );
        assert_eq!(b.warnings(), None);
        assert_eq!(world.remote_head(), b.head());
    }

    #[test]
    fn a_store_without_a_remote_only_commits() {
        let world = World::new();
        let a = world.first();
        a.git(&["remote", "remove", "origin"]);
        let pushed = world.remote_head();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        assert_eq!(a.subjects(), ["update task files", "start"]);
        assert_eq!(world.remote_head(), pushed);
        assert!(a.state("last-fetch").exists(), "the check is throttled too");
    }
}

mod interval {
    use super::*;

    #[test]
    fn a_job_that_committed_nothing_fetches_only_when_due() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.fetched(Duration::from_secs(50));
        let before = b.head();
        assert_silent(&b.job(&[]));
        assert_eq!(world.fetches(), 0);
        assert_eq!(b.head(), before);

        b.fetched(Duration::from_secs(60));
        assert_silent(&b.job(&[]));
        assert_eq!(world.fetches(), 1);
        assert_eq!(b.head(), a.head());
    }

    #[test]
    fn store_open_before_the_interval_spawns_nothing() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.fetched(Duration::from_secs(30));
        let before = b.head();
        assert_silent(&b.store_open(&serde_json::json!({})));
        assert_silent(&b.store_open(&serde_json::json!({"interval": 60})));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(world.fetches(), 0);
        assert_eq!(b.head(), before);
        assert!(!b.state("pending").exists());
    }

    #[test]
    fn store_open_after_the_interval_fetches_once() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.fetched(Duration::from_secs(10));
        assert_silent(&b.store_open(&serde_json::json!({"interval": 5})));
        eventually("the pull", || b.head() == a.head());
        eventually("the job to exit", || !b.state("job.lock").exists());
        assert_eq!(world.fetches(), 1);
        assert_eq!(b.read(TASK_3), a.read(TASK_3));
    }

    #[test]
    fn store_open_on_a_store_never_fetched_fetches() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        assert_silent(&b.store_open(&serde_json::json!({})));
        eventually("the pull", || b.head() == a.head());
        eventually("the job to exit", || !b.state("job.lock").exists());
    }

    #[test]
    fn store_open_on_a_plain_store_does_nothing() {
        let world = World::new();
        let a = world.first();
        std::fs::remove_dir_all(a.store.join(".git")).unwrap();
        assert_silent(&a.store_open(&serde_json::json!({})));
        assert!(!a.store.join(".tasq").exists());
    }

    #[test]
    fn an_interval_that_is_not_whole_seconds_is_an_error() {
        let world = World::new();
        let a = world.first();
        for (value, shown) in [
            (serde_json::json!("soon"), "\"soon\""),
            (serde_json::json!(-1), "-1"),
            (serde_json::json!(1.5), "1.5"),
        ] {
            let out = a.store_open(&serde_json::json!({"interval": value}));
            assert_eq!(out.status.code(), Some(1), "{shown}");
            assert_eq!(
                stderr(&out),
                format!("[plugin.git] interval must be a whole number of seconds, not {shown}\n")
            );
        }
        assert!(!a.store.join(".tasq").exists());
    }
}

mod failure {
    use super::*;

    #[test]
    fn an_unreachable_remote_is_a_warning_cleared_by_the_next_success() {
        let world = World::new();
        let a = world.first();
        let remote = world.base().join("remote.git");
        let gone = world.base().join("gone.git");
        std::fs::rename(&remote, &gone).unwrap();
        // A push warning from before stands: the push was not tried.
        std::fs::create_dir_all(a.state("")).unwrap();
        std::fs::write(a.state("warnings"), "push failed: earlier\n").unwrap();
        a.note(TASK_3, "offline");
        assert_silent(&a.job(&[]));
        assert_eq!(a.subjects(), ["update task files", "start"]);
        let warnings = a.warnings().unwrap();
        let lines: Vec<&str> = warnings.lines().collect();
        assert_eq!(lines.len(), 2, "{warnings}");
        assert_eq!(lines[0], "push failed: earlier");
        assert!(
            lines[1].starts_with(&format!(
                "fetch failed: fatal: '{}' does not appear to be a git repository; ",
                remote.display()
            )),
            "{warnings}"
        );

        std::fs::rename(&gone, &remote).unwrap();
        a.note(TASK_4, "online");
        assert_silent(&a.job(&[]));
        assert_eq!(world.remote_head(), a.head());
        assert_eq!(a.warnings(), None);
    }

    #[test]
    fn a_push_that_fails_twice_is_a_warning_cleared_by_the_next_push() {
        let world = World::new();
        let a = world.first();
        let tries = world.base().join("tries");
        world.pre_push(&format!(
            "echo try >> '{}'\necho nope >&2\nexit 1\n",
            tries.display()
        ));
        let pushed = world.remote_head();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        assert_eq!(std::fs::read_to_string(&tries).unwrap(), "try\ntry\n");
        assert_eq!(world.remote_head(), pushed);
        let warning = a.warnings().unwrap();
        assert!(
            warning.starts_with("push failed: nope; error: failed to push some refs to "),
            "{warning}"
        );

        std::fs::remove_file(world.home().join(".gitconfig")).unwrap();
        assert_silent(&a.job(&["--interval", "0"]));
        assert_eq!(world.remote_head(), a.head());
        assert_eq!(a.warnings(), None);
    }

    #[test]
    fn a_failed_rebase_is_a_warning_and_nothing_is_pushed() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        // A pre-rebase hook that refuses: the rebase fails, whatever git
        // says about conflicts (GS-005).
        let hooks = world.home().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        install(
            &hooks.join("pre-rebase"),
            "#!/bin/sh\necho refused >&2\nexit 1\n",
        );
        b.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
        b.note(TASK_4, "on b");
        let pushed = world.remote_head();
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), pushed);
        let warning = b.warnings().unwrap();
        assert!(warning.starts_with("rebase failed: refused"), "{warning}");

        b.git(&["config", "--unset", "core.hooksPath"]);
        assert_silent(&b.job(&["--interval", "0"]));
        assert_eq!(world.remote_head(), b.head());
        assert_eq!(b.warnings(), None);
    }

    #[test]
    fn a_remote_that_would_prompt_fails_fast() {
        let world = World::new();
        let a = world.first();
        // An ssh that asks the way OpenSSH does: through SSH_ASKPASS when
        // SSH_ASKPASS_REQUIRE=force, else on the terminal, where nobody
        // answers.
        install(
            &world.base().join("bin/ssh"),
            "#!/bin/sh\n\
             if [ \"$SSH_ASKPASS_REQUIRE\" = force ]; then\n\
               \"$SSH_ASKPASS\" 'Enter passphrase: ' || { echo 'Permission denied (publickey).' >&2; exit 255; }\n\
             fi\n\
             read -r answer\n\
             sleep 30\n\
             exit 255\n",
        );
        a.git(&[
            "remote",
            "set-url",
            "origin",
            "ssh://example.invalid/remote.git",
        ]);
        a.note(TASK_3, "on a");
        let started = Instant::now();
        assert_silent(&a.job(&[]));
        assert!(started.elapsed() < Duration::from_secs(10), "it waited");
        let warning = a.warnings().unwrap();
        assert!(
            warning.starts_with("fetch failed: Permission denied (publickey).; "),
            "{warning}"
        );
    }
}

mod conflict {
    use super::*;

    const B_ONLY: &str = "---\nid: 3\n---\n# Three\n- on b\n";

    /// The warning B's job leaves for a conflict on task 3.
    fn warning(b: &Machine) -> String {
        format!(
            "rebase conflict on task 3: local commits kept, nothing pushed until it is resolved: \
             cd {} && git pull --rebase, fix the files, git add them, git rebase --continue\n",
            b.store.display()
        )
    }

    /// A and B both note task 3; A's job pushes first, B's conflicts.
    fn conflicted(world: &World) -> (Machine<'_>, Machine<'_>) {
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.note(TASK_3, "on b");
        assert_silent(&b.job(&[]));
        (a, b)
    }

    #[test]
    fn the_second_sync_is_aborted_and_names_the_task() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        let pushed = world.remote_head();
        b.note(TASK_3, "on b");
        assert_silent(&b.job(&[]));

        assert!(!b.rebasing(), "the rebase was left in progress");
        assert_eq!(b.read(TASK_3), B_ONLY);
        assert_eq!(b.git(&["status", "--porcelain", "--", "*.md"]), "");
        assert_eq!(b.subjects(), ["update task files", "start"]);
        assert_eq!(world.remote_head(), pushed);
        assert_eq!(b.warnings(), Some(warning(&b)));
        assert_eq!(world.locked().lines().count(), 1);
    }

    #[test]
    fn a_later_write_neither_pushes_nor_clears_the_warning() {
        let world = World::new();
        let (_, b) = conflicted(&world);
        let pushed = world.remote_head();
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), pushed);
        assert_eq!(b.read(TASK_3), B_ONLY);
        assert_eq!(b.subjects().len(), 3);
        assert_eq!(b.warnings(), Some(warning(&b)));
    }

    #[test]
    fn a_rebase_left_for_the_next_round_keeps_the_warning() {
        let world = World::new();
        let (_, b) = conflicted(&world);
        // A write lands before the store lock: the rebase is not tried.
        std::fs::write(
            world.base().join("on-lock"),
            format!(
                "printf -- '- late\\n' >> '{}'\n",
                b.store.join(TASK_4).display()
            ),
        )
        .unwrap();
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));
        assert!(
            world.base().join("on-lock.run").exists(),
            "on-lock never ran"
        );
        assert!(b.read(TASK_4).ends_with("- on b\n- late\n"));
        assert_eq!(b.warnings(), Some(warning(&b)));
    }

    #[test]
    fn the_job_waits_for_a_manual_resolution_then_clears_the_warning_and_pushes() {
        let world = World::new();
        let (a, b) = conflicted(&world);
        let pushed = world.remote_head();

        // The commands of the warning: the pull stops on the conflict.
        assert_eq!(b.try_git(&["pull", "--rebase"]).status.code(), Some(1));
        assert!(b.rebasing());
        let stopped = b.head();
        std::fs::write(b.state("writes"), "log 4\n").unwrap();

        // A job meanwhile commits nothing and leaves the rebase alone.
        assert_silent(&b.job(&["--interval", "0"]));
        assert!(b.rebasing());
        assert_eq!(b.head(), stopped);
        assert_eq!(
            std::fs::read_to_string(b.state("writes")).unwrap(),
            "log 4\n"
        );
        assert_eq!(b.warnings(), Some(warning(&b)));
        let out = b
            .command(&["rebase", "--store", b.store.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "a rebase is in progress: finish it with git rebase --continue or --abort\n"
        );
        assert!(b.rebasing(), "the manual rebase was aborted");

        b.write(TASK_3, "---\nid: 3\n---\n# Three\n- on a\n- on b\n");
        b.git(&["add", TASK_3]);
        b.git(&["-c", "core.editor=true", "rebase", "--continue"]);
        assert_eq!(world.remote_head(), pushed);

        assert_silent(&b.job(&["--interval", "0"]));
        assert_eq!(world.remote_head(), b.head());
        assert_eq!(b.warnings(), None);
        assert_eq!(
            b.subjects(),
            ["update task files", "update task files", "start"]
        );
        assert_silent(&a.job(&["--interval", "0"]));
        assert_eq!(a.read(TASK_3), b.read(TASK_3));
    }
}
