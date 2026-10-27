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
//!
//! Duplicate ids (GS-006): `doctor --json` notes the call in `doctors` and
//! answers from the store in `FAKE_STORE`: an `ids` failure for an id two
//! files claim, the `ids` check `ok` otherwise.

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
             if [ \"$1 $2 $3\" = 'store info --json' ]; then\n\
               printf '%s\\n' \"$TASQ_EVENT\" >> \"$base/events\"\n\
               printf '{{\"schema\":1,\"store\":{{\"location\":\"%s\",\"name\":\"tasq\",\"task_count\":0}},\"store_name\":{{\"from\":\"defaults\",\"value\":\"default\"}}}}\\n' \"$FAKE_STORE\"\n\
               exit 0\n\
             fi\n\
             if [ \"$1 $2\" = 'doctor --json' ]; then\n\
               printf '%s\\n' \"$FAKE_STORE\" >> \"$base/doctors\"\n\
               [ -z \"$FAKE_DOCTOR_BROKEN\" ] || {{ echo 'doctor broke' >&2; exit 2; }}\n\
               dup=$(sed -n 's/^id: //p' \"$FAKE_STORE\"/*.md | sort | uniq -d | head -n 1)\n\
               if [ -z \"$dup\" ]; then\n\
                 echo '{{\"schema\":1,\"ok\":true,\"checks\":[{{\"name\":\"ids\",\"status\":\"ok\",\"detail\":\"no id is claimed twice\",\"fix\":null}}]}}'\n\
                 exit 0\n\
               fi\n\
               printf '{{\"schema\":1,\"ok\":false,\"checks\":[{{\"name\":\"task-file\",\"status\":\"fail\",\"detail\":\"x.md is not a task file\",\"fix\":\"move it\"}},{{\"name\":\"ids\",\"status\":\"fail\",\"detail\":\"id %s is claimed twice\",\"fix\":\"renumber the newer\"}}]}}\\n' \"$dup\"\n\
               exit 1\n\
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

    /// The stores `tasq doctor --json` was asked about, one per call.
    fn doctors(&self) -> String {
        std::fs::read_to_string(self.base().join("doctors")).unwrap_or_default()
    }

    /// The commands run under `tasq store lock`, one per line.
    fn locked(&self) -> String {
        std::fs::read_to_string(self.base().join("locked")).unwrap_or_default()
    }

    /// A `git` first on `PATH` that runs `body` for a command line
    /// matching the `case` pattern `when`, then the real git.
    fn wrap_git(&self, when: &str, body: &str) {
        install(
            &self.base().join("bin/git"),
            &format!(
                "#!/bin/sh\ncase \" $* \" in\n  {when}) {body} ;;\nesac\nexec '{}' \"$@\"\n",
                real_git().display()
            ),
        );
    }

    fn unwrap_git(&self) {
        std::fs::remove_file(self.base().join("bin/git")).unwrap();
    }

    /// `script` run before every `git push`, which fails when it does.
    fn on_push(&self, script: &str) {
        let file = self.base().join("on-push");
        std::fs::write(&file, script).unwrap();
        self.wrap_git("*' push '*", &format!("sh '{}' || exit 1", file.display()));
    }

    /// A global git config that signs every commit with a `gpg` that
    /// cannot, and hooks that refuse everything.
    fn hostile_gitconfig(&self) {
        let hooks = self.home().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for hook in ["pre-commit", "pre-push", "pre-rebase"] {
            install(
                &hooks.join(hook),
                &format!("#!/bin/sh\necho global {hook} >&2\nexit 1\n"),
            );
        }
        let gpg = self.base().join("bin/no-gpg");
        install(&gpg, "#!/bin/sh\necho no pinentry >&2\nexit 2\n");
        std::fs::write(
            self.home().join(".gitconfig"),
            format!(
                "[commit]\n\tgpgSign = true\n[gpg]\n\tprogram = {}\n[core]\n\thooksPath = {}\n",
                gpg.display(),
                hooks.display()
            ),
        )
        .unwrap();
    }

    /// Whether a job holds machine `m`'s `job.lock`.
    fn job_running(m: &Machine) -> bool {
        std::fs::File::open(m.state("job.lock")).is_ok_and(|file| file.try_lock().is_err())
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

    /// `tasq-git sync` (`tasq git sync`), in the foreground.
    fn sync(&self) -> Output {
        self.command(&["sync"]).output().unwrap()
    }

    /// `tasq-git hook` with the `store-open` payload carrying `config`.
    fn store_open(&self, config: &serde_json::Value) -> Output {
        self.hook("store-open", config)
    }

    /// The plugin's `doctor` answer, parsed.
    fn doctor(&self) -> serde_json::Value {
        let out = self.hook("doctor", &serde_json::json!({}));
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(stderr(&out), "");
        serde_json::from_str(&stdout(&out)).unwrap()
    }

    /// `tasq-git hook` with the payload of `event` carrying `config`.
    fn hook(&self, event: &str, config: &serde_json::Value) -> Output {
        let payload = serde_json::json!({
            "schema": 1,
            "event": event,
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
            .env("FAKE_STORE", &self.store)
            .current_dir(self.world.home());
        cmd
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
        assert!(!World::job_running(&b));
    }

    #[test]
    fn the_users_signing_and_hooks_are_not_applied() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        world.hostile_gitconfig();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.note(TASK_4, "on b");
        // B commits, rebases its commit onto A's and pushes it.
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), b.head());
        assert_eq!(
            b.subjects(),
            ["update task files", "update task files", "start"]
        );
        assert_eq!(b.git(&["log", "-2", "--format=%G?"]), "N\nN\n");
        assert_eq!(a.warnings(), None);
        assert_eq!(b.warnings(), None);
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
        world.on_push(&format!(
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
    fn a_job_started_during_a_long_round_does_nothing() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        // By the rebase, B's round has run for three minutes; a second job
        // started then must find the job lock held, however old it looks.
        let old = world.base().join("old");
        std::fs::File::create(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(180))
            .unwrap();
        let second = world.base().join("second");
        std::fs::write(
            world.base().join("on-lock"),
            format!(
                "touch -r '{old}' '{lock}'\n\
                 '{exe}' job --store '{store}' --interval 0 > '{second}' 2>&1\n\
                 echo \"exit $?\" >> '{second}'\n",
                old = old.display(),
                lock = b.state("job.lock").display(),
                exe = env!("CARGO_BIN_EXE_tasq-git"),
                store = b.store.display(),
                second = second.display(),
            ),
        )
        .unwrap();
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "exit 0\n");
        assert_eq!(world.fetches(), 1, "the second job fetched");
        assert_eq!(world.locked().lines().count(), 1);
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
        eventually("the job to exit", || !World::job_running(&b));
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
        eventually("the job to exit", || !World::job_running(&b));
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
    fn an_interval_that_is_not_whole_seconds_is_an_error_and_the_default_applies() {
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
        // Never fetched: due whatever the interval, so a job fetched.
        eventually("the fetch", || a.state("last-fetch").exists());
        eventually("the job to exit", || !World::job_running(&a));
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
        world.on_push(&format!(
            "echo try >> '{}'\necho nope >&2\nexit 1\n",
            tries.display()
        ));
        let pushed = world.remote_head();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        assert_eq!(std::fs::read_to_string(&tries).unwrap(), "try\ntry\n");
        assert_eq!(world.remote_head(), pushed);
        assert_eq!(a.warnings().as_deref(), Some("push failed: nope\n"));

        world.unwrap_git();
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
        // A rebase that refuses before it starts: it fails, whatever git
        // says about conflicts (GS-005).
        world.wrap_git("*' rebase --quiet '*", "echo refused >&2; exit 1");
        b.note(TASK_4, "on b");
        let pushed = world.remote_head();
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), pushed);
        let warning = b.warnings().unwrap();
        assert!(warning.starts_with("rebase failed: refused"), "{warning}");

        world.unwrap_git();
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

    #[test]
    fn a_remote_that_hangs_is_given_up_on() {
        let world = World::new();
        let a = world.first();
        install(
            &world.base().join("bin/ssh"),
            "#!/bin/sh\nsleep 20\nexit 255\n",
        );
        a.git(&[
            "remote",
            "set-url",
            "origin",
            "ssh://example.invalid/remote.git",
        ]);
        a.note(TASK_3, "on a");
        let started = Instant::now();
        assert_silent(&a.job(&["--timeout", "1"]));
        assert!(started.elapsed() < Duration::from_secs(10), "it waited");
        assert_eq!(
            a.warnings().as_deref(),
            Some("fetch failed: git timed out after 1 s\n")
        );
        assert_eq!(a.subjects(), ["update task files", "start"]);
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
    fn a_conflict_whose_files_cannot_be_listed_is_still_aborted() {
        let world = World::new();
        let (_a, b) = conflicted(&world);
        world.wrap_git("*' diff --name-only '*", "echo diff broke >&2; exit 128");
        assert_silent(&b.job(&["--interval", "0"]));
        assert!(!b.rebasing(), "the stopped rebase was left behind");
        assert_eq!(b.read(TASK_3), B_ONLY);
        assert_eq!(b.warnings().as_deref(), Some("rebase failed: diff broke\n"));

        world.unwrap_git();
        assert_silent(&b.job(&["--interval", "0"]));
        assert_eq!(b.warnings().unwrap(), warning(&b));
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

mod duplicate {
    use super::*;

    /// Task 5 as machine A created it offline.
    const A_5: &str = "01K6Q8ZT3D000000000000000A.md";
    /// Task 5 as machine B created it offline, later.
    const B_5: &str = "01K6Q8ZT3E000000000000000B.md";

    const WARNING: &str = "duplicate id: id 5 is claimed twice; fix: renumber the newer\n";

    #[test]
    fn an_id_pulled_in_twice_is_a_warning_until_fixed_by_hand() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.write(A_5, "---\nid: 5\n---\n# Five on a\n");
        b.write(B_5, "---\nid: 5\n---\n# Five on b\n");

        // A pulls nothing: doctor is not asked.
        assert_silent(&a.job(&[]));
        assert_eq!(world.doctors(), "");
        assert_eq!(a.warnings(), None);

        // B's rebase brings A's task 5.
        assert_silent(&b.job(&[]));
        assert_eq!(world.remote_head(), b.head(), "the duplicate is pushed");
        assert_eq!(b.warnings().as_deref(), Some(WARNING));
        assert_eq!(world.doctors(), format!("{}\n", b.store.display()));

        // A pulls B's task 5.
        assert_silent(&a.job(&["--interval", "0"]));
        assert_eq!(a.warnings().as_deref(), Some(WARNING));

        // The warning stands until doctor says otherwise: a round that pulls
        // nothing asks again.
        assert_silent(&b.job(&["--interval", "0"]));
        assert_eq!(b.warnings().as_deref(), Some(WARNING));
        assert_eq!(world.doctors().lines().count(), 3);

        // The fix by hand on B: committed, pushed, the warning gone.
        b.write(B_5, "---\nid: 6\n---\n# Five on b\n");
        assert_silent(&b.job(&[]));
        assert_eq!(b.warnings(), None);
        assert_eq!(world.remote_head(), b.head());

        // A pulls the fix.
        assert_silent(&a.job(&["--interval", "0"]));
        assert_eq!(a.warnings(), None);
        assert_eq!(a.head(), b.head());
        assert_eq!(world.doctors().lines().count(), 5);

        // No warning and nothing pulled: doctor is not asked.
        assert_silent(&a.job(&["--interval", "0"]));
        assert_eq!(world.doctors().lines().count(), 5);
    }

    #[test]
    fn a_doctor_that_fails_leaves_the_warnings_as_they_were() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        std::fs::create_dir_all(b.state("")).unwrap();
        std::fs::write(b.state("warnings"), WARNING).unwrap();
        let out = b
            .command(&["job", "--store", b.store.to_str().unwrap()])
            .env("FAKE_DOCTOR_BROKEN", "1")
            .output()
            .unwrap();
        assert_silent(&out);
        assert_eq!(b.head(), a.head(), "the pull happened");
        assert_eq!(world.doctors().lines().count(), 1, "doctor was asked");
        assert_eq!(b.warnings().as_deref(), Some(WARNING));
    }
}

/// `tasq git sync`: the job's steps in the foreground, one line each.
mod foreground {
    use super::*;

    /// The output and exit code of `tasq-git sync`, stderr empty.
    fn synced(m: &Machine) -> (String, Option<i32>) {
        let out = m.sync();
        assert_eq!(stderr(&out), "");
        (stdout(&out), out.status.code())
    }

    #[test]
    fn a_clean_sync_prints_each_step() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        b.note(TASK_4, "on b");
        assert_silent(&b.job(&[]));
        std::fs::create_dir_all(a.state("")).unwrap();
        std::fs::write(a.state("writes"), "log 3\n").unwrap();
        a.note(TASK_3, "on a");
        // Fetched a moment ago: the foreground fetches all the same.
        a.fetched(Duration::ZERO);

        assert_eq!(
            synced(&a),
            (
                "committed: log 3: Three\n\
                 fetched origin/default\n\
                 pulled 1 commit from origin/default\n\
                 pushed 1 commit to origin/default\n"
                    .to_owned(),
                Some(0)
            )
        );
        assert_eq!(world.remote_head(), a.head());
        assert_eq!(a.subjects(), ["log 3: Three", "update task files", "start"]);
        assert_eq!(a.warnings(), None);
        assert!(!World::job_running(&a));
        assert_eq!(
            std::fs::read_to_string(world.base().join("events")).unwrap(),
            "sync\n",
            "tasq is told not to fire events at the plugin"
        );
    }

    #[test]
    fn several_commits_are_counted() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        b.note(TASK_4, "on b");
        b.git(&["commit", "-q", "-am", "one"]);
        b.note(TASK_4, "again");
        b.git(&["commit", "-q", "-am", "two"]);
        b.git(&["push", "-q"]);
        a.note(TASK_3, "on a");
        a.git(&["commit", "-q", "-am", "three"]);
        a.note(TASK_3, "again");
        assert_eq!(
            synced(&a),
            (
                "committed: update task files\n\
                 fetched origin/default\n\
                 pulled 2 commits from origin/default\n\
                 pushed 2 commits to origin/default\n"
                    .to_owned(),
                Some(0)
            )
        );
    }

    #[test]
    fn nothing_to_do_is_up_to_date() {
        let world = World::new();
        let a = world.first();
        assert_eq!(
            synced(&a),
            (
                "nothing to commit\n\
                 fetched origin/default\n\
                 up to date with origin/default\n"
                    .to_owned(),
                Some(0)
            )
        );
        assert!(a.state("last-fetch").exists());
    }

    #[test]
    fn an_offline_remote_is_the_failed_step_and_exit_1() {
        let world = World::new();
        let a = world.first();
        let remote = world.base().join("remote.git");
        std::fs::rename(&remote, world.base().join("gone.git")).unwrap();
        a.note(TASK_3, "offline");
        let (out, code) = synced(&a);
        assert_eq!(code, Some(1));
        let failure = format!(
            "fetch failed: fatal: '{}' does not appear to be a git repository; fatal: Could \
             not read from remote repository.; Please make sure you have the correct access \
             rights; and the repository exists.",
            remote.display()
        );
        assert_eq!(out, format!("committed: update task files\n{failure}\n"));
        assert_eq!(a.warnings(), Some(format!("{failure}\n")));
    }

    #[test]
    fn a_conflict_is_aborted_and_reported() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.note(TASK_3, "on b");
        let pushed = world.remote_head();
        let (out, code) = synced(&b);
        assert_eq!(code, Some(1));
        let failure = format!(
            "rebase conflict on task 3: local commits kept, nothing pushed until it is \
             resolved: cd {} && git pull --rebase, fix the files, git add them, git rebase \
             --continue",
            b.store.display()
        );
        assert_eq!(
            out,
            format!("committed: update task files\nfetched origin/default\n{failure}\n")
        );
        assert_eq!(b.warnings(), Some(format!("{failure}\n")));
        assert_eq!(world.remote_head(), pushed);
        assert!(!b.rebasing());
        assert!(b.read(TASK_3).ends_with("# Three\n- on b\n"));
    }

    #[test]
    fn a_rejected_push_is_tried_again() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        a.git(&["commit", "-q", "-am", "from a"]);
        world.on_push(&format!(
            "[ -f '{done}' ] && exit 0\n: > '{done}'\ngit -C '{a}' push -q\n",
            done = world.base().join("raced").display(),
            a = a.store.display()
        ));
        b.note(TASK_4, "on b");
        let (out, code) = synced(&b);
        assert_eq!(code, Some(0), "{out}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 6, "{out}");
        assert_eq!(
            lines[..2],
            ["committed: update task files", "fetched origin/default"]
        );
        assert!(
            lines[2].starts_with("push failed, fetching again: "),
            "{out}"
        );
        assert!(lines[2].contains("! [rejected]"), "{out}");
        assert!(
            lines[2].contains(&format!(
                "; error: failed to push some refs to '{}'; ",
                world.url()
            )),
            "{out}"
        );
        assert_eq!(
            lines[3..],
            [
                "fetched origin/default",
                "pulled 1 commit from origin/default",
                "pushed 1 commit to origin/default",
            ]
        );
        assert_eq!(world.remote_head(), b.head());
    }

    #[test]
    fn a_store_without_an_upstream_only_commits() {
        let world = World::new();
        let a = world.first();
        a.git(&["branch", "--unset-upstream"]);
        a.note(TASK_3, "local");
        assert_eq!(
            synced(&a),
            (
                "committed: update task files\nno upstream: nothing to fetch or push\n".to_owned(),
                Some(0)
            )
        );
        assert_eq!(a.subjects(), ["update task files", "start"]);
    }

    #[test]
    fn the_users_signing_and_hooks_are_not_applied() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        world.hostile_gitconfig();
        a.note(TASK_3, "on a");
        assert_eq!(synced(&a).1, Some(0));
        b.note(TASK_4, "on b");
        assert_eq!(
            synced(&b),
            (
                "committed: update task files\n\
                 fetched origin/default\n\
                 pulled 1 commit from origin/default\n\
                 pushed 1 commit to origin/default\n"
                    .to_owned(),
                Some(0)
            )
        );
        assert_eq!(world.remote_head(), b.head());
        assert_eq!(b.git(&["log", "-2", "--format=%G?"]), "N\nN\n");
    }

    #[test]
    fn a_pulled_duplicate_id_is_reported() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.write(
            "01K6Q8ZT3D000000000000000A.md",
            "---\nid: 5\n---\n# Five on a\n",
        );
        assert_silent(&a.job(&[]));
        b.write(
            "01K6Q8ZT3E000000000000000B.md",
            "---\nid: 5\n---\n# Five on b\n",
        );
        let (out, code) = synced(&b);
        assert_eq!(code, Some(1));
        assert_eq!(
            out,
            "committed: update task files\n\
             fetched origin/default\n\
             pulled 1 commit from origin/default\n\
             pushed 1 commit to origin/default\n\
             duplicate id: id 5 is claimed twice; fix: renumber the newer\n"
        );
    }

    #[test]
    fn it_waits_for_a_running_job() {
        let world = World::new();
        let a = world.first();
        std::fs::create_dir_all(a.state("")).unwrap();
        let lock = std::fs::File::create(a.state("job.lock")).unwrap();
        lock.lock().unwrap();
        let mut child = a
            .command(&["sync"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(child.try_wait().unwrap().is_none(), "it did not wait");
        assert!(!a.state("last-fetch").exists());
        drop(lock);
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(a.state("last-fetch").exists());
    }

    #[test]
    fn a_rebase_in_progress_is_refused() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.note(TASK_3, "on b");
        b.git(&["commit", "-q", "-am", "on b"]);
        assert_eq!(b.try_git(&["pull", "--rebase"]).status.code(), Some(1));
        let out = b.sync();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: a rebase is in progress in {}: finish it with git rebase --continue \
                 or --abort\n",
                b.store.display()
            )
        );
        assert!(b.rebasing());
    }

    #[test]
    fn a_plain_store_is_refused() {
        let world = World::new();
        let plain = world.machine("plain");
        std::fs::create_dir_all(&plain.store).unwrap();
        let out = plain.sync();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: {} is not a git repository: tasq git init makes it one, or tasq git \
                 clone <url> fills an empty store\n",
                plain.store.display()
            )
        );
        assert!(!plain.state("").exists());
    }

    #[test]
    fn a_failed_commit_is_an_error() {
        let world = World::new();
        let a = world.first();
        world.wrap_git("*' commit '*", "echo no >&2; exit 1");
        a.note(TASK_3, "on a");
        let out = a.sync();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(stderr(&out), "tasq-git: commit failed: no\n");
    }
}

/// The plugin's `doctor` checks for a store that is a repository.
mod doctor {
    use super::*;

    fn check(name: &str, status: &str, message: &str) -> serde_json::Value {
        serde_json::json!({"name": name, "status": status, "message": message})
    }

    fn fixed(name: &str, status: &str, message: &str, fix: &str) -> serde_json::Value {
        serde_json::json!({"name": name, "status": status, "message": message, "fix": fix})
    }

    #[test]
    fn a_store_in_sync_has_each_check_ok() {
        let world = World::new();
        let a = world.first();
        a.fetched(Duration::from_secs(300));
        assert_eq!(
            a.doctor(),
            serde_json::json!([
                check(
                    "identity",
                    "ok",
                    "commits as Me Personal <me@example.invalid>"
                ),
                check(
                    "remote",
                    "ok",
                    &format!("branch default tracks origin/default at {}", world.url())
                ),
                check("push", "ok", "every commit is pushed"),
                check("fetch", "ok", "last fetch 5 min ago"),
                check("rebase", "ok", "no rebase in progress"),
            ])
        );
    }

    #[test]
    fn unpushed_commits_a_key_and_no_fetch_yet() {
        let world = World::new();
        let a = world.first();
        a.git(&[
            "config",
            "core.sshCommand",
            "ssh -i '/keys/me' -o IdentitiesOnly=yes",
        ]);
        a.note(TASK_3, "one");
        a.git(&["commit", "-q", "-am", "one"]);
        a.note(TASK_3, "two");
        a.git(&["commit", "-q", "-am", "two"]);
        assert_eq!(
            a.doctor(),
            serde_json::json!([
                check(
                    "identity",
                    "ok",
                    "commits as Me Personal <me@example.invalid>, pushes with ssh -i '/keys/me' \
                     -o IdentitiesOnly=yes"
                ),
                check(
                    "remote",
                    "ok",
                    &format!("branch default tracks origin/default at {}", world.url())
                ),
                fixed("push", "warn", "2 commits not pushed", "tasq git sync"),
                fixed("fetch", "warn", "never fetched", "tasq git sync"),
                check("rebase", "ok", "no rebase in progress"),
            ])
        );
        a.git(&["reset", "-q", "--hard", "HEAD~1"]);
        assert_eq!(
            a.doctor()[2],
            fixed("push", "warn", "1 commit not pushed", "tasq git sync")
        );
    }

    #[test]
    fn a_store_without_an_upstream_has_no_remote_checks() {
        let world = World::new();
        let a = world.first();
        a.git(&["branch", "--unset-upstream"]);
        assert_eq!(
            a.doctor(),
            serde_json::json!([
                check(
                    "identity",
                    "ok",
                    "commits as Me Personal <me@example.invalid>"
                ),
                check(
                    "remote",
                    "ok",
                    "branch default has no upstream: commits stay local"
                ),
                check("rebase", "ok", "no rebase in progress"),
            ])
        );
    }

    #[test]
    fn a_rebase_in_progress_fails() {
        let world = World::new();
        let a = world.first();
        let b = world.second();
        a.note(TASK_3, "on a");
        assert_silent(&a.job(&[]));
        b.note(TASK_3, "on b");
        b.git(&["commit", "-q", "-am", "on b"]);
        assert_eq!(b.try_git(&["pull", "--rebase"]).status.code(), Some(1));
        let store = b.store.display();
        assert_eq!(
            b.doctor(),
            serde_json::json!([
                check(
                    "identity",
                    "ok",
                    "commits as Me Personal <me@example.invalid>"
                ),
                fixed(
                    "rebase",
                    "fail",
                    &format!("a rebase is in progress in {store}"),
                    &format!(
                        "cd {store}, fix the files, git add them, git rebase --continue (or git \
                         rebase --abort)"
                    )
                ),
            ])
        );
    }

    #[test]
    fn an_unknown_identity_fails() {
        let world = World::new();
        let a = world.first();
        a.git(&["config", "--unset", "user.email"]);
        std::fs::write(
            world.home().join(".gitconfig"),
            "[user]\n\tuseConfigOnly = true\n",
        )
        .unwrap();
        let store = a.store.display();
        assert_eq!(
            a.doctor()[0],
            fixed(
                "identity",
                "fail",
                "git cannot tell who commits: fatal: no email was given and auto-detection is \
                 disabled",
                &format!(
                    "git -C {store} config user.name '<name>' && git -C {store} config \
                     user.email '<email>'"
                )
            )
        );
    }
}
