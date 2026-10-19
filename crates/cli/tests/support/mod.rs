//! Harness for the CLI integration tests: a temporary copy of the fixture
//! store as the default store under an isolated `HOME`
//! (`~/.local/share/tasq/stores/default`, ADR-0026), a `PATH` holding only
//! `git`, and `TZ=UTC` (file names are ULIDs minted from the clock), so the
//! binary under test never sees the real store, the real config or the
//! machine's zone.

#![allow(
    dead_code,
    reason = "each integration test crate uses a different subset of the helpers"
)]

use std::path::{Path, PathBuf};
use std::process::Output;

use tempfile::TempDir;

/// An isolated environment for one test.
pub struct TestEnv {
    /// Owns the temporary tree.
    pub root: TempDir,
    /// `<root>/home`, what `HOME` points at; also the working directory.
    pub home: PathBuf,
    /// `<root>/bin`, the only `PATH` entry.
    pub bin: PathBuf,
}

impl TestEnv {
    /// A copy of `crates/store/tests/fixtures/stores/default` as the
    /// default store (ids 1, 3, 4, 6 and 7).
    pub fn fixture() -> Self {
        let env = Self::bare();
        copy_dir(&fixture_dir(), &env.store());
        env
    }

    /// No store at all: the directory does not exist until the first write.
    pub fn empty() -> Self {
        Self::bare()
    }

    fn bare() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        // Canonical, so paths the binary canonicalizes (project, worktree)
        // normalise the same way as the ones it is given.
        let root_path = root.path().canonicalize().unwrap();
        let home = root_path.join("home");
        let bin = root_path.join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = tasq tests\n\temail = tasq-tests@example.invalid\n",
        )
        .unwrap();
        if let Some(git) = find_on_path("git") {
            std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
        }
        Self { root, home, bin }
    }

    /// The default `data_dir` under this `HOME`:
    /// `<home>/.local/share/tasq/stores`.
    pub fn data_dir(&self) -> PathBuf {
        self.home
            .join(".local")
            .join("share")
            .join("tasq")
            .join("stores")
    }

    /// The default store, `<data_dir>/default`.
    pub fn store(&self) -> PathBuf {
        self.data_dir().join("default")
    }

    /// The binary under test with only this environment, run from `home`,
    /// on a fixed day (`TASQ_NOW`, overridable) so relative due dates do
    /// not move, in UTC so minted file names do not depend on the zone.
    pub fn tasq(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_tasq"));
        cmd.env_clear()
            .env("TASQ_NOW", "2026-10-06 09:00")
            .env("TZ", "UTC")
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .current_dir(&self.home);
        // Under `cargo llvm-cov` the binary is instrumented: without this it
        // drops `default_*.profraw` in its working directory, which a test
        // worktree then counts as a change.
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            cmd.env("LLVM_PROFILE_FILE", profile);
        }
        cmd
    }

    /// [`Self::tasq`] with worktrees enabled (`TASQ_WORKTREE_ENABLED=1`, ADR 0022).
    pub fn tasq_wt(&self) -> assert_cmd::Command {
        let mut cmd = self.tasq();
        cmd.env("TASQ_WORKTREE_ENABLED", "1");
        cmd
    }

    /// Writes a file called `name` inside the store, creating the store
    /// directory when it does not exist yet (a task placed by hand, or a
    /// stray markdown file).
    pub fn write_task(&self, name: &str, text: &str) {
        std::fs::create_dir_all(self.store()).unwrap();
        std::fs::write(self.store().join(name), text).unwrap();
    }

    /// The task files of the store (`*.md`, not dot files), sorted by
    /// name; empty when the store does not exist.
    pub fn task_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.store()) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "md")
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        files.sort();
        files
    }

    /// The one file in the store whose frontmatter says `id: <id>`.
    pub fn file_of(&self, id: u64) -> PathBuf {
        let line = format!("id: {id}");
        let found: Vec<PathBuf> = self
            .task_files()
            .into_iter()
            .filter(|p| {
                std::fs::read_to_string(p)
                    .unwrap()
                    .lines()
                    .skip(1)
                    .take_while(|l| *l != "---")
                    .any(|l| l.trim() == line)
            })
            .collect();
        match found.as_slice() {
            [one] => one.clone(),
            [] => panic!("no task file with id {id} in {}", self.store().display()),
            many => panic!("id {id} is claimed by {many:?}"),
        }
    }

    /// Contents of the file of task `id`.
    pub fn read_task(&self, id: u64) -> String {
        std::fs::read_to_string(self.file_of(id)).unwrap()
    }

    /// Writes `<home>/.tasq.toml`.
    pub fn write_project_config(&self, text: &str) {
        std::fs::write(self.home.join(".tasq.toml"), text).unwrap();
    }

    /// Writes `<home>/.config/tasq/config.toml`.
    pub fn write_global_config(&self, text: &str) -> PathBuf {
        let dir = self.home.join(".config").join("tasq");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, text).unwrap();
        file
    }

    /// Replaces the temporary root with `[ROOT]` so output can be snapshotted.
    pub fn normalize(&self, text: &str) -> String {
        let canonical = self.root.path().canonicalize().unwrap();
        text.replace(&canonical.display().to_string(), "[ROOT]")
            .replace(&self.root.path().display().to_string(), "[ROOT]")
    }

    /// Runs `git <args>` in `dir` with this environment; panics on failure.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Makes `dir` a git repository with one commit on `main`.
    pub fn git_repo(&self, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        self.git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("README"), "hi\n").unwrap();
        self.git(dir, &["add", "README"]);
        self.git(dir, &["commit", "-q", "-m", "init"]);
    }

    /// Installs an executable script called `name` in `bin`.
    pub fn fake_tool(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let script = self.bin.join(name);
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Probe until runnable (ETXTBSY when another test thread forks).
        for _ in 0..200 {
            if std::process::Command::new(&script)
                .env("FAKE_PROBE", "1")
                .output()
                .is_ok()
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("fake {name} never became runnable");
    }
}

/// A task file: the frontmatter block (`id` first, then `front`: zero or
/// more `key: value` lines, each ending in `\n`) and `body`, which starts
/// with the `# Title` line and ends in `\n`.
pub fn v2(id: u64, front: &str, body: &str) -> String {
    assert!(
        front.is_empty() || front.ends_with('\n'),
        "front must end in a newline: {front:?}"
    );
    format!("---\nid: {id}\n{front}---\n{body}")
}

/// The ULID file name for a task created at `when` (`YYYY-MM-DD HH:MM`,
/// UTC) whose random part is the number `n`: what the store would mint at
/// that time, with a predictable tail (the fixture store's names have
/// tails 1 to 5).
pub fn ulid_name(when: &str, n: u64) -> String {
    let millis = chrono::NaiveDateTime::parse_from_str(when, "%Y-%m-%d %H:%M")
        .unwrap()
        .and_utc()
        .timestamp_millis();
    format!(
        "{}.md",
        tasq_store::ulid::Ulid::from_parts(u64::try_from(millis).unwrap(), u128::from(n))
    )
}

/// A loopback HTTP server answering GET requests from a fixed route table
/// (exact request target → status and JSON body; anything else is 404),
/// recording every request target. Lives until the test process exits.
pub struct FakeHttp {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeHttp {
    /// Starts a server with `routes`.
    pub fn start(routes: Vec<(&str, u16, &str)>) -> Self {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&requests);
        let routes: Vec<(String, u16, String)> = routes
            .into_iter()
            .map(|(p, s, b)| (p.to_owned(), s, b.to_owned()))
            .collect();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let target = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                log.lock().unwrap().push(target.clone());
                let (status, body) = routes.iter().find(|(p, _, _)| *p == target).map_or(
                    (404, "{\"message\":\"404 Not Found\"}".to_owned()),
                    |(_, s, b)| (*s, b.clone()),
                );
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { base, requests }
    }

    /// The request targets seen so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Standard output as text.
pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Standard error as text.
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The fixture store in the source tree.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../store/tests/fixtures/stores/default")
        .canonicalize()
        .expect("fixture store exists")
}

/// Copies `from` into `to` recursively (`to` is created).
pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// The first `PATH` entry of the test process holding `name`.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
