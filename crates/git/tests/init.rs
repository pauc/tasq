//! `tasq git init`, run the way tasq's dispatch runs it: `tasq-git init
//! ...` with `TASQ_BIN` naming the `tasq` to ask for the store and the
//! `[plugin.git]` defaults. That `tasq` is a fake answering `store info
//! --json` and `config show --json`; git is the real one, under a `HOME`
//! the test owns, and the remote a bare repository on disk.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// A temp root holding `home`, `bin` (the fake `tasq`), the store
/// directory (absent until a test creates it) and a bare remote.
struct Env {
    root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    store: PathBuf,
}

impl Env {
    /// No store yet, `[plugin.git]` empty, no `.gitconfig`.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let home = base.join("home");
        let bin = base.join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let store = base.join("data").join("stores").join("default");
        let env = Self {
            root,
            home,
            bin,
            store,
        };
        env.answers("store-info.json", &env.store_info());
        env.plugin_config(&serde_json::json!({}));
        let script = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = --set ]; then\n\
               printf '%s\\n' \"$2\" >> '{dir}/sets'\n\
               shift 2\n\
             fi\n\
             case \"$1 $2 $3\" in\n\
             \"store info --json\") answer='{dir}/store-info.json' ;;\n\
             \"config show --json\") answer='{dir}/config-show.json' ;;\n\
             *) echo \"fake tasq: $*\" >&2; exit 2 ;;\n\
             esac\n\
             IFS= read -r line < \"$answer\"\n\
             printf '%s\\n' \"$line\"\n",
            dir = env.base().display()
        );
        install(&env.bin.join("tasq"), &script);
        env
    }

    fn base(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    /// What the real `tasq store info --json` prints for this store.
    fn store_info(&self) -> String {
        serde_json::json!({
            "data_dir": {"from": "defaults", "value": self.store.parent().unwrap()},
            "plugins": ["git"],
            "schema": 1,
            "store": {"location": self.store, "name": "tasq", "task_count": 0},
            "store_name": {"from": "defaults", "value": "default"},
        })
        .to_string()
    }

    fn answers(&self, file: &str, text: &str) {
        std::fs::write(self.base().join(file), text).unwrap();
    }

    /// `[plugin.git]` as `tasq config show --json` would hand it over.
    fn plugin_config(&self, table: &serde_json::Value) {
        let doc = serde_json::json!({
            "config": {"plugin": {"git": table}, "store": {"name": "default"}},
            "profile": null,
            "profiles": [],
            "layers": [],
        });
        self.answers("config-show.json", &doc.to_string());
    }

    /// A store directory with `files` (relative path, contents).
    fn store_with(&self, files: &[(&str, &str)]) {
        for (path, text) in files {
            let path = self.store.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    fn gitconfig(&self, text: &str) {
        std::fs::write(self.home.join(".gitconfig"), text).unwrap();
    }

    /// `PATH`: the fake `tasq` first, then the test's own (for git, sh, cat).
    fn path(&self) -> OsString {
        let mut dirs = vec![self.bin.clone()];
        dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        std::env::join_paths(dirs).unwrap()
    }

    /// `tasq-git <args>` as dispatch runs it: `TASQ_BIN` set, nothing else
    /// of the test's environment but `PATH` and `HOME`.
    fn tasq_git(&self, args: &[&str]) -> Output {
        self.command(args)
            .env("TASQ_BIN", self.bin.join("tasq"))
            .output()
            .unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tasq-git"));
        cmd.args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", self.path())
            .current_dir(&self.home);
        cmd
    }

    /// `git <args>` in `dir`; panics on failure.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_output(dir, args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn git_output(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", self.path())
            .output()
            .unwrap()
    }

    /// The store repository's local config value of `key`, `None` if unset.
    fn config(&self, key: &str) -> Option<String> {
        let out = self.git_output(&self.store, &["config", "--local", "--get", key]);
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim_end().to_owned())
    }

    /// The files of the commit at `rev` in `dir`, sorted.
    fn tree(&self, dir: &Path, rev: &str) -> Vec<String> {
        self.git(dir, &["ls-tree", "-r", "--name-only", rev])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// An empty bare repository; returns its `file://` URL.
    fn remote(&self) -> String {
        let dir = self.base().join("remote.git");
        self.git(
            &self.base(),
            &["init", "-q", "--bare", dir.to_str().unwrap()],
        );
        format!("file://{}", dir.display())
    }

    fn remote_dir(&self) -> PathBuf {
        self.base().join("remote.git")
    }

    /// Pushes one commit holding `file` to `branch` of the remote, from a
    /// scratch repository: another store's history.
    fn remote_branch(&self, url: &str, branch: &str) {
        let dir = self.base().join(format!("other-{branch}"));
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q", "-b", branch]);
        std::fs::write(
            dir.join("01K0000000000000000000000X.md"),
            "---\nid: 9\n---\n# X\n",
        )
        .unwrap();
        self.git(&dir, &["add", "-A"]);
        self.git(
            &dir,
            &[
                "-c",
                "user.name=Other",
                "-c",
                "user.email=o@x",
                "commit",
                "-q",
                "-m",
                "x",
            ],
        );
        self.git(&dir, &["push", "-q", url, branch]);
    }
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
                std::thread::sleep(std::time::Duration::from_millis(2));
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

fn assert_ok(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{}", stderr(out));
    assert_eq!(stderr(out), "");
}

const TASK_3: (&str, &str) = (
    "01K6Q8ZT3D0000000000000003.md",
    "---\nid: 3\n---\n# Three\n",
);
const TASK_4: (&str, &str) = ("01K6Q8ZT3D0000000000000004.md", "---\nid: 4\n---\n# Four\n");

const ME: [&str; 5] = [
    "init",
    "--name",
    "Me Personal",
    "--email",
    "me@example.invalid",
];

mod local {
    use super::*;

    #[test]
    fn the_first_commit_holds_exactly_the_task_files() {
        let env = Env::new();
        env.store_with(&[
            TASK_3,
            TASK_4,
            (".tasq/lock", ""),
            (".tasq/plugins/git/warnings", "old\n"),
            ("notes.txt", "mine\n"),
            (
                "sub/01K6Q8ZT3D0000000000000005.md",
                "---\nid: 5\n---\n# Five\n",
            ),
            (".#01K6Q8ZT3D0000000000000003.md", "an editor lock\n"),
            ("01K6Q8ZT3D0000000000000006.MD", "---\nid: 6\n---\n# Six\n"),
        ]);
        let out = env.tasq_git(&ME);
        assert_ok(&out);
        assert_eq!(
            stdout(&out),
            format!(
                "initialized {} as a git repository on branch default: 3 task files committed\n",
                env.store.display()
            )
        );
        assert_eq!(
            env.tree(&env.store, "HEAD"),
            [TASK_3.0, TASK_4.0, "01K6Q8ZT3D0000000000000006.MD"]
        );
        assert_eq!(
            env.git(&env.store, &["log", "--format=%an <%ae>|%cn <%ce>|%s"]),
            "Me Personal <me@example.invalid>|Me Personal <me@example.invalid>|tasq git init\n"
        );
        assert_eq!(
            env.git(&env.store, &["branch", "--show-current"]),
            "default\n"
        );
        assert_eq!(env.git(&env.store, &["remote"]), "");
        assert!(
            env.store.join(".tasq/lock").is_file(),
            "the store lost .tasq"
        );
        assert!(env.store.join("notes.txt").is_file());
    }

    #[test]
    fn one_task_file_is_singular() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let out = env.tasq_git(&ME);
        assert_ok(&out);
        assert!(
            stdout(&out).ends_with(" on branch default: 1 task file committed\n"),
            "{}",
            stdout(&out)
        );
    }

    #[test]
    fn an_absent_store_is_created_with_an_empty_first_commit() {
        let env = Env::new();
        let out = env.tasq_git(&ME);
        assert_ok(&out);
        assert!(
            stdout(&out).ends_with(": 0 task files committed\n"),
            "{}",
            stdout(&out)
        );
        assert_eq!(env.tree(&env.store, "HEAD"), Vec::<String>::new());
        assert_eq!(env.git(&env.store, &["rev-list", "--count", "HEAD"]), "1\n");
    }

    #[test]
    fn branch_names_the_local_branch() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let mut args = ME.to_vec();
        args.extend(["--branch", "laptop"]);
        assert_ok(&env.tasq_git(&args));
        assert_eq!(
            env.git(&env.store, &["branch", "--show-current"]),
            "laptop\n"
        );
    }

    #[test]
    fn a_repository_is_refused_and_left_alone() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        env.git(&env.store, &["init", "-q", "-b", "mine"]);
        let out = env.tasq_git(&ME);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: {} is already a git repository\n",
                env.store.display()
            )
        );
        assert_eq!(env.git(&env.store, &["branch", "--show-current"]), "mine\n");
        assert_eq!(env.config("user.name"), None);
    }

    #[test]
    fn a_failed_commit_leaves_no_repository() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let hooks = env.base().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        install(
            &hooks.join("pre-commit"),
            "#!/bin/sh\necho no commits today >&2\nexit 1\n",
        );
        env.gitconfig(&format!("[core]\n\thooksPath = {}\n", hooks.display()));
        let out = env.tasq_git(&ME);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq-git: git commit failed: ")
                && stderr(&out).contains("no commits today"),
            "{}",
            stderr(&out)
        );
        assert!(
            !env.store.join(".git").exists(),
            "a half-made repository stayed"
        );
        assert!(env.store.join(TASK_3.0).is_file(), "a task file went");
    }

    #[test]
    fn a_bad_branch_name_is_gits_error_and_leaves_no_repository() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let mut args = ME.to_vec();
        args.extend(["--branch", "two words"]);
        let out = env.tasq_git(&args);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "tasq-git: git init failed: fatal: invalid initial branch name: 'two words'\n"
        );
        assert!(!env.store.join(".git").exists());
    }

    /// Nothing to undo when git never ran: its error is the one reported.
    #[test]
    fn no_git_is_reported_as_such() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let out = env
            .command(&ME)
            .env("TASQ_BIN", env.bin.join("tasq"))
            .env("PATH", &env.bin)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq-git: git init failed: could not run git: "),
            "{}",
            stderr(&out)
        );
        assert!(!env.store.join(".git").exists());
    }
}

mod identity {
    use super::*;

    #[test]
    fn flags_land_in_the_repository_config() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let mut args = ME.to_vec();
        args.extend(["--ssh-key", "/home/me/.ssh/personal"]);
        assert_ok(&env.tasq_git(&args));
        assert_eq!(env.config("user.name").as_deref(), Some("Me Personal"));
        assert_eq!(
            env.config("user.email").as_deref(),
            Some("me@example.invalid")
        );
        assert_eq!(
            env.config("core.sshCommand").as_deref(),
            Some("ssh -i '/home/me/.ssh/personal' -o IdentitiesOnly=yes")
        );
    }

    #[test]
    fn plugin_config_gives_the_defaults() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        env.plugin_config(&serde_json::json!({
            "name": "From Config",
            "email": "config@example.invalid",
            "ssh_key": "~/.ssh/it's mine",
            "interval": 60,
        }));
        assert_ok(&env.tasq_git(&["init"]));
        assert_eq!(env.config("user.name").as_deref(), Some("From Config"));
        assert_eq!(
            env.config("user.email").as_deref(),
            Some("config@example.invalid")
        );
        assert_eq!(
            env.config("core.sshCommand").as_deref(),
            Some(r"ssh -i '~/.ssh/it'\''s mine' -o IdentitiesOnly=yes")
        );
        assert_eq!(
            env.git(&env.store, &["log", "--format=%an <%ae>"]),
            "From Config <config@example.invalid>\n"
        );
    }

    #[test]
    fn flags_win_over_the_defaults_key_by_key() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        env.plugin_config(&serde_json::json!({
            "name": "From Config",
            "email": "config@example.invalid",
            "ssh_key": "/config/key",
        }));
        assert_ok(&env.tasq_git(&["init", "--name", "From Flag", "--ssh-key", "/flag/key"]));
        assert_eq!(env.config("user.name").as_deref(), Some("From Flag"));
        assert_eq!(
            env.config("user.email").as_deref(),
            Some("config@example.invalid")
        );
        assert_eq!(
            env.config("core.sshCommand").as_deref(),
            Some("ssh -i '/flag/key' -o IdentitiesOnly=yes")
        );
    }

    #[test]
    fn without_any_git_resolves_it_and_nothing_is_written() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        env.gitconfig("[user]\n\tname = Global\n\temail = global@example.invalid\n");
        assert_ok(&env.tasq_git(&["init"]));
        assert_eq!(env.config("user.name"), None);
        assert_eq!(env.config("user.email"), None);
        assert_eq!(env.config("core.sshCommand"), None);
        assert_eq!(
            env.git(&env.store, &["log", "--format=%an <%ae>"]),
            "Global <global@example.invalid>\n"
        );
    }

    #[test]
    fn exported_git_variables_are_ignored() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let elsewhere = env.base().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let out = env
            .command(&ME)
            .env("TASQ_BIN", env.bin.join("tasq"))
            .env("GIT_DIR", &elsewhere)
            .env("GIT_WORK_TREE", &elsewhere)
            .env("GIT_AUTHOR_NAME", "Work Me")
            .env("GIT_AUTHOR_EMAIL", "me@work.invalid")
            .env("GIT_COMMITTER_NAME", "Work Me")
            .env("GIT_COMMITTER_EMAIL", "me@work.invalid")
            .output()
            .unwrap();
        assert_ok(&out);
        assert_eq!(
            env.git(&env.store, &["log", "--format=%an <%ae>|%cn <%ce>"]),
            "Me Personal <me@example.invalid>|Me Personal <me@example.invalid>\n"
        );
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    #[test]
    fn a_default_that_is_not_a_string_is_refused() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        env.plugin_config(&serde_json::json!({"email": 7}));
        let out = env.tasq_git(&["init"]);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "tasq-git: [plugin.git] email must be a string, not 7\n"
        );
        assert!(!env.store.join(".git").exists());
    }
}

mod remote {
    use super::*;

    fn with_url<'a>(url: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = ME.to_vec();
        args.insert(1, url);
        args.extend(extra);
        args
    }

    #[test]
    fn an_empty_remote_gets_a_branch_named_after_the_store() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = env.remote();
        let out = env.tasq_git(&with_url(&url, &[]));
        assert_ok(&out);
        assert_eq!(
            stdout(&out),
            format!(
                "initialized {} as a git repository on branch default: 1 task file committed\n\
                 pushed branch default to {url}\n",
                env.store.display()
            )
        );
        let local = env.git(&env.store, &["rev-parse", "HEAD"]);
        assert_eq!(env.git(&env.remote_dir(), &["rev-parse", "default"]), local);
        assert_eq!(env.config("remote.origin.url"), Some(url));
        assert_eq!(
            env.config("branch.default.remote").as_deref(),
            Some("origin")
        );
        assert_eq!(
            env.config("branch.default.merge").as_deref(),
            Some("refs/heads/default")
        );
    }

    #[test]
    fn a_missing_branch_is_pushed_beside_the_others() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = env.remote();
        env.remote_branch(&url, "work");
        let other = env.git(&env.remote_dir(), &["rev-parse", "work"]);
        assert_ok(&env.tasq_git(&with_url(&url, &[])));
        assert_eq!(env.tree(&env.remote_dir(), "default"), [TASK_3.0]);
        assert_eq!(env.git(&env.remote_dir(), &["rev-parse", "work"]), other);
    }

    #[test]
    fn branch_names_the_remote_branch() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = env.remote();
        let out = env.tasq_git(&with_url(&url, &["--branch", "laptop"]));
        assert_ok(&out);
        assert!(
            stdout(&out).ends_with(&format!("pushed branch laptop to {url}\n")),
            "{}",
            stdout(&out)
        );
        assert_eq!(env.tree(&env.remote_dir(), "laptop"), [TASK_3.0]);
        assert_eq!(
            env.config("branch.laptop.merge").as_deref(),
            Some("refs/heads/laptop")
        );
    }

    #[test]
    fn a_branch_with_history_is_refused_with_the_fix() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = env.remote();
        env.remote_branch(&url, "default");
        let before = env.git(&env.remote_dir(), &["rev-parse", "default"]);
        let out = env.tasq_git(&with_url(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: branch default of {url} already has history; tasq git init never \
                 merges two task histories\n\
                 fix: tasq git clone {url} --branch default into an empty store, or \
                 tasq git init {url} --branch <another> to push to a new branch\n"
            )
        );
        assert!(
            !env.store.join(".git").exists(),
            "the store became a repository"
        );
        assert_eq!(
            env.git(&env.remote_dir(), &["rev-parse", "default"]),
            before
        );
    }

    #[test]
    fn an_unreachable_remote_leaves_the_store_alone() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = format!("file://{}", env.base().join("nowhere.git").display());
        let out = env.tasq_git(&with_url(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with(&format!("tasq-git: could not reach {url}: ")),
            "{}",
            stderr(&out)
        );
        assert!(!env.store.join(".git").exists());
    }

    #[test]
    fn a_failed_push_leaves_no_repository() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let url = env.remote();
        let hooks = env.base().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        install(
            &hooks.join("pre-push"),
            "#!/bin/sh\necho no pushes today >&2\nexit 1\n",
        );
        env.gitconfig(&format!("[core]\n\thooksPath = {}\n", hooks.display()));
        let out = env.tasq_git(&with_url(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq-git: git push failed: ")
                && stderr(&out).contains("no pushes today"),
            "{}",
            stderr(&out)
        );
        assert!(!env.store.join(".git").exists());
        assert_eq!(env.git(&env.remote_dir(), &["branch"]), "");
    }
}

mod tasq {
    use super::*;

    #[test]
    fn tasq_on_path_is_used_without_tasq_bin() {
        let env = Env::new();
        env.store_with(&[TASK_3]);
        let out = env.command(&ME).output().unwrap();
        assert_ok(&out);
        assert_eq!(env.tree(&env.store, "HEAD"), [TASK_3.0]);
    }

    #[test]
    fn a_failing_tasq_is_an_error() {
        let env = Env::new();
        install(
            &env.bin.join("broken"),
            "#!/bin/sh\necho 'config: bad key' >&2\nexit 1\n",
        );
        let out = env
            .command(&ME)
            .env("TASQ_BIN", env.bin.join("broken"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            "tasq-git: tasq store info --json failed: config: bad key\n"
        );
    }

    #[test]
    fn an_answer_that_is_not_store_info_is_an_error() {
        let env = Env::new();
        env.answers("store-info.json", r#"{"schema":1}"#);
        let out = env.tasq_git(&ME);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq-git: tasq store info --json answered something else: "),
            "{}",
            stderr(&out)
        );
    }

    #[test]
    fn an_answer_that_is_not_config_is_an_error() {
        let env = Env::new();
        env.answers("config-show.json", "[]");
        let out = env.tasq_git(&ME);
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with("tasq-git: tasq config show --json answered something else: "),
            "{}",
            stderr(&out)
        );
    }
}

/// `tasq git clone <url>`: a store from a remote branch.
mod clone {
    use super::*;

    const X: &str = "01K0000000000000000000000X.md";

    fn clone<'a>(url: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec!["clone", url];
        args.extend(extra);
        args
    }

    /// A remote whose `default` branch holds task 9.
    fn remote_with_default(env: &Env) -> String {
        let url = env.remote();
        env.remote_branch(&url, "default");
        url
    }

    #[test]
    fn an_absent_store_gets_the_branch_with_its_upstream_and_identity() {
        let env = Env::new();
        let url = remote_with_default(&env);
        let out = env.tasq_git(&clone(
            &url,
            &["--name", "Me Personal", "--email", "me@example.invalid"],
        ));
        assert_ok(&out);
        assert_eq!(
            stdout(&out),
            format!(
                "cloned branch default of {url} into {}: 1 task file\n",
                env.store.display()
            )
        );
        assert_eq!(
            std::fs::read_to_string(env.store.join(X)).unwrap(),
            "---\nid: 9\n---\n# X\n"
        );
        assert_eq!(
            env.git(&env.store, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
            "origin/default\n"
        );
        assert_eq!(
            env.config("remote.origin.fetch").as_deref(),
            Some("+refs/heads/default:refs/remotes/origin/default")
        );
        assert_eq!(env.config("user.name").as_deref(), Some("Me Personal"));
        assert_eq!(
            env.config("user.email").as_deref(),
            Some("me@example.invalid")
        );
        assert_eq!(env.config("core.sshCommand"), None);
        assert_eq!(
            env.git(&env.store, &["status", "--porcelain"]),
            "",
            "the working tree is the branch"
        );
    }

    #[test]
    fn several_task_files_are_counted() {
        let env = Env::new();
        let url = env.remote();
        let dir = env.base().join("src");
        std::fs::create_dir_all(&dir).unwrap();
        env.git(&dir, &["init", "-q", "-b", "default"]);
        std::fs::write(dir.join(TASK_3.0), TASK_3.1).unwrap();
        std::fs::write(dir.join(TASK_4.0), TASK_4.1).unwrap();
        env.git(&dir, &["add", "-A"]);
        env.git(
            &dir,
            &[
                "-c",
                "user.name=O",
                "-c",
                "user.email=o@x",
                "commit",
                "-q",
                "-m",
                "x",
            ],
        );
        env.git(&dir, &["push", "-q", &url, "default"]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_ok(&out);
        assert!(
            stdout(&out).ends_with(": 2 task files\n"),
            "{}",
            stdout(&out)
        );
    }

    #[test]
    fn a_store_with_other_files_keeps_them() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.store_with(&[(".tasq/lock", ""), ("notes.txt", "mine\n")]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_ok(&out);
        assert!(env.store.join(X).is_file());
        assert_eq!(
            std::fs::read_to_string(env.store.join("notes.txt")).unwrap(),
            "mine\n"
        );
        assert!(env.store.join(".tasq/lock").is_file());
    }

    #[test]
    fn a_store_with_task_files_is_refused() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.store_with(&[("01K6Q8ZT3D0000000000000003.MD", TASK_3.1)]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(stdout(&out), "");
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: {} already has task files; tasq git clone only fills an empty \
                 store\nfix: tasq git clone {url} --store <another> into a new store, or tasq \
                 git init {url} --branch <another> to push this one\n",
                env.store.display()
            )
        );
        assert!(!env.store.join(".git").exists());
    }

    #[test]
    fn hidden_or_nested_markdown_is_no_task_file() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.store_with(&[
            (".#x.md", "lock\n"),
            ("sub/y.md", "nested\n"),
            ("z.txt", ""),
        ]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_ok(&out);
    }

    #[test]
    fn a_repository_is_refused() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.store_with(&[("notes.txt", "")]);
        env.git(&env.store, &["init", "-q"]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: {} is already a git repository\n",
                env.store.display()
            )
        );
    }

    #[test]
    fn a_missing_branch_is_refused_with_the_fix() {
        let env = Env::new();
        let url = remote_with_default(&env);
        let out = env.tasq_git(&clone(&url, &["--branch", "work"]));
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            stderr(&out),
            format!(
                "tasq-git: branch work of {url} does not exist\nfix: tasq git init {url} \
                 --branch work pushes a store to it\n"
            )
        );
        assert!(!env.store.exists(), "the store was created");
    }

    #[test]
    fn an_unreachable_remote_is_refused() {
        let env = Env::new();
        let url = format!("file://{}", env.base().join("nowhere.git").display());
        let out = env.tasq_git(&clone(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).starts_with(&format!("tasq-git: could not reach {url}: ")),
            "{}",
            stderr(&out)
        );
        assert!(!env.store.exists());
    }

    #[test]
    fn branch_picks_the_remote_and_local_branch() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.remote_branch(&url, "work");
        let out = env.tasq_git(&clone(&url, &["--branch", "work"]));
        assert_ok(&out);
        assert!(
            stdout(&out).starts_with(&format!("cloned branch work of {url} into ")),
            "{}",
            stdout(&out)
        );
        assert_eq!(env.git(&env.store, &["branch", "--show-current"]), "work\n");
        assert_eq!(
            env.git(&env.store, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
            "origin/work\n"
        );
        assert_eq!(env.git(&env.store, &["branch", "--list"]), "* work\n");
    }

    #[test]
    fn store_names_the_store_and_the_branch() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.remote_branch(&url, "work");
        let work = env.base().join("data").join("stores").join("work");
        env.answers(
            "store-info.json",
            &serde_json::json!({
                "store": {"location": work, "name": "tasq", "task_count": 0},
                "store_name": {"from": "--set", "value": "work"},
            })
            .to_string(),
        );
        let out = env.tasq_git(&clone(&url, &["--store", "work"]));
        assert_ok(&out);
        assert_eq!(
            std::fs::read_to_string(env.base().join("sets")).unwrap(),
            "store.name=work\n"
        );
        assert_eq!(env.git(&work, &["branch", "--show-current"]), "work\n");
        assert!(work.join(X).is_file());
        assert!(!env.store.exists());
    }

    #[test]
    fn without_store_tasq_resolves_it() {
        let env = Env::new();
        let url = remote_with_default(&env);
        assert_ok(&env.tasq_git(&clone(&url, &[])));
        assert!(!env.base().join("sets").exists());
    }

    #[test]
    fn plugin_config_gives_the_identity_defaults() {
        let env = Env::new();
        let url = remote_with_default(&env);
        env.plugin_config(&serde_json::json!({
            "name": "Me Personal",
            "email": "me@example.invalid",
            "ssh_key": "/keys/me",
        }));
        let out = env.tasq_git(&clone(&url, &["--name", "Flag"]));
        assert_ok(&out);
        assert_eq!(env.config("user.name").as_deref(), Some("Flag"));
        assert_eq!(
            env.config("user.email").as_deref(),
            Some("me@example.invalid")
        );
        assert_eq!(
            env.config("core.sshCommand").as_deref(),
            Some("ssh -i '/keys/me' -o IdentitiesOnly=yes")
        );
    }

    #[test]
    fn a_failed_checkout_leaves_no_repository() {
        let env = Env::new();
        let url = env.remote();
        let dir = env.base().join("src");
        std::fs::create_dir_all(&dir).unwrap();
        env.git(&dir, &["init", "-q", "-b", "default"]);
        std::fs::write(dir.join("notes.txt"), "theirs\n").unwrap();
        env.git(&dir, &["add", "-A"]);
        env.git(
            &dir,
            &[
                "-c",
                "user.name=O",
                "-c",
                "user.email=o@x",
                "commit",
                "-q",
                "-m",
                "x",
            ],
        );
        env.git(&dir, &["push", "-q", &url, "default"]);
        env.store_with(&[("notes.txt", "mine\n")]);
        let out = env.tasq_git(&clone(&url, &[]));
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out)
                .starts_with("tasq-git: git checkout failed: error: The following untracked"),
            "{}",
            stderr(&out)
        );
        assert!(!env.store.join(".git").exists());
        assert_eq!(
            std::fs::read_to_string(env.store.join("notes.txt")).unwrap(),
            "mine\n"
        );
    }
}
