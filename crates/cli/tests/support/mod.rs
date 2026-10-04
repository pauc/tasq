//! Harness for the CLI integration tests: a temporary copy of the nb store's
//! fixture notebook, an isolated `HOME` and a `PATH` holding only `git`, so
//! the binary under test never sees the real `~/.nb`, the real config or a
//! real `nb` (which keeps the output identical whether or not nb is
//! installed).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Output;

use tempfile::TempDir;

/// An isolated environment for one test.
pub struct TestEnv {
    /// Owns the temporary tree.
    pub root: TempDir,
    /// `<root>/nb`, what `NB_DIR` points at.
    pub nb_dir: PathBuf,
    /// `<root>/home`, what `HOME` points at; also the working directory.
    pub home: PathBuf,
    /// `<root>/bin`, the only `PATH` entry.
    pub bin: PathBuf,
}

impl TestEnv {
    /// A copy of `crates/store-nb/tests/fixtures/nb/home` as notebook `home`.
    pub fn fixture() -> Self {
        let env = Self::bare();
        copy_dir(&fixture_dir(), &env.notebook());
        env
    }

    /// A notebook `home` with an empty index and no tasks.
    pub fn empty() -> Self {
        let env = Self::bare();
        std::fs::create_dir_all(env.notebook()).unwrap();
        std::fs::write(env.notebook().join(".index"), "").unwrap();
        env
    }

    fn bare() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        // Canonical, so paths the binary canonicalizes (project, worktree)
        // normalise the same way as the ones it is given.
        let root_path = root.path().canonicalize().unwrap();
        let nb_dir = root_path.join("nb");
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
        Self {
            root,
            nb_dir,
            home,
            bin,
        }
    }

    /// `<root>/nb/home`.
    pub fn notebook(&self) -> PathBuf {
        self.nb_dir.join("home")
    }

    /// The binary under test with only this environment, run from `home`.
    pub fn tasq(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_tasq"));
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("NB_DIR", &self.nb_dir)
            .env("PATH", &self.bin)
            .current_dir(&self.home);
        cmd
    }

    /// Writes a file inside the notebook.
    pub fn write_task(&self, name: &str, text: &str) {
        std::fs::write(self.notebook().join(name), text).unwrap();
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

    /// Contents of a notebook file.
    pub fn read_task(&self, name: &str) -> String {
        std::fs::read_to_string(self.notebook().join(name)).unwrap()
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

/// The nb store's fixture notebook in the source tree.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../store-nb/tests/fixtures/nb/home")
        .canonicalize()
        .expect("fixture notebook exists")
}

fn copy_dir(from: &Path, to: &Path) {
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
