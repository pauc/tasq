//! A temporary store directory for the integration tests: nothing in it
//! until a test writes a file, no `git`.

#![allow(dead_code, reason = "each test file uses a subset of the helpers")]

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tasq_core::clock::FixedClock;
use tasq_core::model::Workflow;
use tasq_store::TasqStore;
use tasq_store::store::RandomSource;
use tasq_store::ulid::Ulid;

/// A temp root holding the store directory `store` (not created until a
/// test writes into it).
pub struct TempStore {
    root: tempfile::TempDir,
}

impl TempStore {
    /// A fresh root whose store directory does not exist yet.
    pub fn absent() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    /// A fresh root whose store directory exists and is empty.
    pub fn empty() -> Self {
        let this = Self::absent();
        std::fs::create_dir(this.dir()).unwrap();
        this
    }

    /// The root above the store directory.
    pub fn root_path(&self) -> &Path {
        self.root.path()
    }

    /// The store directory.
    pub fn dir(&self) -> PathBuf {
        self.root.path().join("store")
    }

    /// `<dir>/<name>`.
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir().join(name)
    }

    /// Writes `text` to `<dir>/<name>`, creating the directory.
    pub fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.file(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Opens the store over `dir()` with the default workflow.
    pub fn open(&self) -> TasqStore {
        TasqStore::new(self.dir(), Workflow::default())
    }
}

/// The names of the entries of `dir`, sorted; empty when it does not exist.
pub fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The instant every test's clock is frozen at.
pub const CLOCK: &str = "2026-10-04 12:34";

/// A millisecond timestamp derived from the fixed clock the same way the store derives it:
/// the local wall-clock time read in the machine's zone, so the expectation holds in any `TZ`.
pub fn clock_millis() -> u64 {
    local_millis(tasq_core::clock::FixedClock::at(CLOCK).0)
}

/// `local`, a wall-clock time in the machine's zone, as milliseconds since the Unix epoch.
pub fn local_millis(local: chrono::NaiveDateTime) -> u64 {
    use chrono::TimeZone;
    let millis = chrono::Local
        .from_local_datetime(&local)
        .earliest()
        .map_or_else(
            || local.and_utc().timestamp_millis(),
            |t| t.timestamp_millis(),
        );
    u64::try_from(millis).unwrap()
}

impl TempStore {
    /// Opens the store over `dir()` with the default workflow, the clock frozen at `clock`
    /// (`YYYY-MM-DD HH:MM`) and `random` as the source of the random part of minted ULIDs.
    pub fn open_with(&self, clock: &str, random: RandomSource) -> TasqStore {
        TasqStore::new(self.dir(), Workflow::default())
            .with_clock(Box::new(FixedClock::at(clock)))
            .with_random(random)
    }

    /// Opens the store with the clock frozen at [`CLOCK`] and the random part of every minted
    /// ULID taken from `random`, in order (the last value repeats once the list is used up).
    pub fn open_at(&self, random: &[u128]) -> TasqStore {
        let mut queue: std::collections::VecDeque<u128> = random.iter().copied().collect();
        let last = *random.last().expect("at least one random value");
        self.open_with(CLOCK, Box::new(move || queue.pop_front().unwrap_or(last)))
    }

    /// `<dir>/.tasq/lock`.
    pub fn lock_path(&self) -> PathBuf {
        self.dir().join(".tasq").join("lock")
    }
}

/// `<ULID>.md` for a ULID minted at `millis` with the random part `random`.
pub fn ulid_name(millis: u64, random: u128) -> String {
    format!("{}.md", Ulid::from_parts(millis, random))
}

/// A minimal task file.
pub fn task_file(id: u64, title: &str, status: &str) -> String {
    format!(
        "---\nid: {id}\nstatus: {status}\n---\n# {title}\n\n## Progress\n\n- 2026-10-04 10:15: created\n"
    )
}

/// Sets the modification time of `path`.
pub fn touch(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// Whether the tests run as root, where permission bits do not apply.
#[cfg(unix)]
pub fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}
