//! Test harness for the nb store: an isolated copy of the fixture notebook
//! and a way to run the real `nb` against it.
//!
//! Every `nb` invocation gets *only* the environment built here: `NB_DIR`,
//! `NBRC_PATH` and `HOME` inside a temporary directory, `NB_AUTO_SYNC=0`,
//! and the real `PATH` (to find `nb`, `bash` and `git`). Nothing can touch
//! the real `~/.nb`.
//!
//! Tests that need `nb` call [`nb_or_skip`]: without `nb` on `PATH` they
//! print a notice and pass, unless `TASQ_REQUIRE_NB=1` is set (CI), in which
//! case they fail.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use tasq_core::model::Workflow;
use tasq_store_nb::sanitize::strip_ansi;
use tasq_store_nb::{NbStore, NbStoreOptions};
use tempfile::TempDir;

/// Name of the fixture notebook (and the default `store.notebook`).
pub const NOTEBOOK: &str = "home";

/// Index line numbers of the fixture, for readable tests.
pub mod id {
    /// Open, `#gitlab #A #in-progress`, every section.
    pub const FULL: u64 = 1;
    /// `notes.md`, not a todo.
    pub const NOTE: u64 = 2;
    /// Open, `#support #B #ready`.
    pub const SUPPORT: u64 = 3;
    /// Done, `#gitlab #C`.
    pub const DONE: u64 = 4;
    /// Index line whose file does not exist.
    pub const MISSING: u64 = 5;
    /// Open, `#waiting`, no priority tag.
    pub const WAITING: u64 = 6;
    /// Open, no `## Tags` section.
    pub const NO_TAGS: u64 = 7;
}

/// File names of the fixture todos, by id.
pub fn file_name(id: u64) -> &'static str {
    match id {
        id::FULL => "20260901090000.todo.md",
        id::NOTE => "notes.md",
        id::SUPPORT => "20260902100000.todo.md",
        id::DONE => "20260903110000.todo.md",
        id::MISSING => "20260905130000.todo.md",
        id::WAITING => "20260904120000.todo.md",
        id::NO_TAGS => "20260906140000.todo.md",
        _ => panic!("no fixture file for id {id}"),
    }
}

/// An isolated nb environment around a copy of the fixture notebook.
pub struct NbEnv {
    /// Owns the temporary tree; dropped last.
    pub root: TempDir,
    /// `<tmp>/nb`, what `NB_DIR` points at.
    pub nb_dir: PathBuf,
    /// `<tmp>/nbrc`, an empty rc file so nb never reads the real one.
    pub nbrc_path: PathBuf,
    /// `<tmp>/home`, what `HOME` points at (holds a `.gitconfig` so nb's
    /// git commits have an identity).
    pub home: PathBuf,
    /// The complete environment `nb` runs with.
    pub env: Vec<(String, String)>,
}

impl NbEnv {
    /// Copies `tests/fixtures/nb/home` to `<tmp>/nb/home`.
    pub fn fixture() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let nb_dir = root.path().join("nb");
        let home = root.path().join("home");
        let nbrc_path = root.path().join("nbrc");
        copy_dir(&fixture_dir(), &nb_dir.join(NOTEBOOK));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&nbrc_path, "").unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = tasq tests\n\temail = tasq-tests@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let env = vec![
            ("NB_DIR".to_owned(), nb_dir.display().to_string()),
            ("NBRC_PATH".to_owned(), nbrc_path.display().to_string()),
            ("NB_AUTO_SYNC".to_owned(), "0".to_owned()),
            ("HOME".to_owned(), home.display().to_string()),
            ("PATH".to_owned(), path),
        ];
        Self {
            root,
            nb_dir,
            nbrc_path,
            home,
            env,
        }
    }

    /// `<tmp>/nb/home`.
    pub fn notebook(&self) -> PathBuf {
        self.nb_dir.join(NOTEBOOK)
    }

    /// Path of a fixture file inside the copied notebook.
    pub fn file(&self, id: u64) -> PathBuf {
        self.notebook().join(file_name(id))
    }

    /// Contents of a notebook file.
    pub fn read(&self, name: &str) -> String {
        let path = self.notebook().join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Overwrites a notebook file.
    pub fn write(&self, name: &str, text: &str) {
        std::fs::write(self.notebook().join(name), text).unwrap();
    }

    /// Modification time of a notebook file.
    pub fn mtime(&self, name: &str) -> SystemTime {
        std::fs::metadata(self.notebook().join(name))
            .unwrap()
            .modified()
            .unwrap()
    }

    /// Modification times of every file in the notebook, by name.
    pub fn mtimes(&self) -> Vec<(String, SystemTime)> {
        let mut out: Vec<(String, SystemTime)> = std::fs::read_dir(self.notebook())
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.metadata().unwrap().modified().unwrap(),
                )
            })
            .collect();
        out.sort();
        out
    }

    /// Makes the notebook a git repository with one commit, as nb's own
    /// `nb notebooks add` would (nb only recognises notebooks that are git
    /// repositories when asked through `nb notebooks show`).
    pub fn git_init(&self) {
        for args in [
            &["init", "-q"][..],
            &["add", "-A"],
            &["commit", "-q", "-m", "[nb] Initialize"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(self.notebook())
                .env_clear()
                .envs(self.env.iter().map(|(k, v)| (k, v)))
                .status()
                .expect("git runs");
            assert!(status.success(), "git {args:?} failed");
        }
    }

    /// `git log --format=%s` of the notebook, newest first.
    pub fn git_subjects(&self) -> Vec<String> {
        let out = Command::new("git")
            .args(["log", "--format=%s"])
            .current_dir(self.notebook())
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Runs `nb <args>` with only this environment and returns its sanitised
    /// standard output. Panics when nb fails.
    pub fn nb(&self, args: &[&str]) -> String {
        let out = Command::new("nb")
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .expect("nb runs");
        let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
        assert!(
            out.status.success(),
            "nb {args:?} failed: {}\n{}",
            strip_ansi(&String::from_utf8_lossy(&out.stderr)),
            stdout
        );
        stdout
    }

    /// Store options pointing at this environment.
    pub fn options(&self) -> NbStoreOptions {
        NbStoreOptions::new(Workflow::default())
            .with_env(self.env.clone())
            .with_home(&self.home)
    }

    /// Opens the fixture notebook through `NbStore::open` with the default
    /// `store.notebook = "home"`.
    pub fn open(&self) -> NbStore {
        NbStore::open(&tasq_core::config::StoreConfig::default(), &self.options())
            .expect("fixture notebook opens")
    }
}

/// `tests/fixtures/nb/home` in the source tree.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nb/home")
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

/// Whether `nb` is on the current `PATH`.
pub fn nb_available() -> bool {
    std::env::var("PATH").is_ok_and(|path| {
        tasq_store_nb::nb::find_in_path(&path, tasq_store_nb::nb::PROGRAM).is_some()
    })
}

/// `true` when the test may run. Without `nb`, prints a notice and returns
/// `false` so the test passes vacuously, unless `TASQ_REQUIRE_NB=1`, which
/// makes the test fail instead.
pub fn nb_or_skip(test: &str) -> bool {
    if nb_available() {
        return true;
    }
    assert!(
        !std::env::var("TASQ_REQUIRE_NB").is_ok_and(|v| v == "1"),
        "{test}: nb is not on PATH but TASQ_REQUIRE_NB=1"
    );
    eprintln!("{test}: skipped, nb is not on PATH (set TASQ_REQUIRE_NB=1 to make this a failure)");
    false
}

/// Ids printed by `nb todos` (`[7] ✔️  [ ] Title`), in output order.
pub fn nb_ids(output: &str) -> Vec<u64> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix('[')?;
            let end = rest.find(']')?;
            rest[..end].parse().ok()
        })
        .collect()
}

/// A directory holding a fake `nb` script with `body` as its contents
/// (after the shebang). The script's argv and the invocation count go to
/// `<dir>/argv.log`, one line per call.
pub fn fake_nb(body: &str) -> TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("nb");
    let log = dir.path().join("argv.log");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{body}\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

/// The argv lines recorded by a [`fake_nb`].
pub fn fake_nb_calls(dir: &TempDir) -> Vec<String> {
    std::fs::read_to_string(dir.path().join("argv.log"))
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}
