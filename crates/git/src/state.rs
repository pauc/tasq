//! The plugin's own files, under `<store>/.tasq/plugins/git/`:
//!
//! - `job.lock`: held by the running job (one per store), created
//!   exclusively, taken for a dead job's once it is two minutes old.
//! - `pending`: touched by every `post-write`; the job removes it before
//!   each commit and goes round again while it is back.
//! - `writes`: one line per `post-write` not yet committed (`<command>
//!   <ids>`), for the commit message. Hooks append to it and the job drops
//!   what it committed, both under an advisory lock on the file.
//! - `warnings`: the lasting warnings tasq shows (`docs/plugins.md`).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How old a `job.lock` must be to be taken for a dead job's, as for the
/// store lock.
pub const STALE: Duration = Duration::from_secs(120);

/// The state directory of one store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    dir: PathBuf,
}

impl State {
    /// The state of the store at `store`.
    pub fn of(store: &Path) -> Self {
        Self {
            dir: store.join(".tasq").join("plugins").join("git"),
        }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    fn create_dir(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)
    }

    /// Appends `line` to `writes`.
    pub fn record(&self, line: &str) -> io::Result<()> {
        self.create_dir()?;
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.path("writes"))?;
        file.lock()?;
        file.write_all(format!("{line}\n").as_bytes())
    }

    /// The text of `writes`, empty when there is none.
    pub fn writes(&self) -> io::Result<String> {
        let Some(mut file) = open_existing(&self.path("writes"))? else {
            return Ok(String::new());
        };
        file.lock_shared()?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    /// Drops `taken`, as [`Self::writes`] returned it, from the start of
    /// `writes`, keeping the lines appended since.
    pub fn forget(&self, taken: &str) -> io::Result<()> {
        if taken.is_empty() {
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path("writes"))?;
        file.lock()?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        let rest = text.strip_prefix(taken).unwrap_or(&text);
        let rest = rest.to_owned();
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(rest.as_bytes())
    }

    /// Touches `pending`: the job must go round again.
    pub fn touch_pending(&self) -> io::Result<()> {
        self.create_dir()?;
        File::create(self.path("pending")).map(drop)
    }

    /// Removes `pending`, if there.
    pub fn clear_pending(&self) -> io::Result<()> {
        remove_if_present(&self.path("pending"))
    }

    /// Whether `pending` is there.
    pub fn is_pending(&self) -> bool {
        self.path("pending").exists()
    }

    /// Whether a live job holds `job.lock` at `now`.
    pub fn job_running(&self, now: SystemTime) -> bool {
        modified(&self.path("job.lock"))
            .ok()
            .flatten()
            .is_some_and(|modified| !is_stale(modified, now))
    }

    /// Takes `job.lock`, breaking a stale one; `None` while a live job
    /// holds it.
    pub fn lock_job(&self, now: SystemTime) -> io::Result<Option<JobLock>> {
        self.create_dir()?;
        let path = self.path("job.lock");
        // Twice: once more after breaking a dead job's lock. Two jobs
        // breaking it at once both remove it; one exclusive create wins.
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let token = token(now);
                    file.write_all(token.as_bytes())?;
                    return Ok(Some(JobLock { path, token }));
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
            match modified(&path)? {
                Some(modified) if !is_stale(modified, now) => return Ok(None),
                _ => remove_if_present(&path)?,
            }
        }
        Ok(None)
    }

    /// Replaces the warning lines starting with `kind` by `kind` followed
    /// by `message`, or removes them when `message` is `None`. The other
    /// lines are kept; the file goes when none is left.
    pub fn set_warning(&self, kind: &str, message: Option<&str>) -> io::Result<()> {
        let path = self.path("warnings");
        let mut text = String::new();
        if let Some(mut file) = open_existing(&path)? {
            file.read_to_string(&mut text)?;
        }
        let mut lines: Vec<String> = text
            .lines()
            .filter(|line| !line.starts_with(kind))
            .map(str::to_owned)
            .collect();
        if let Some(message) = message {
            lines.push(format!("{kind}{message}"));
        }
        if lines.is_empty() {
            return remove_if_present(&path);
        }
        self.create_dir()?;
        // A temp file and a rename, so tasq never reads half a line.
        let temp = self.path("warnings.tmp");
        std::fs::write(&temp, lines.join("\n") + "\n")?;
        std::fs::rename(temp, path)
    }
}

/// `path` opened for reading; `None` when it does not exist.
fn open_existing(path: &Path) -> io::Result<Option<File>> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// When `path` was last modified; `None` when it does not exist.
fn modified(path: &Path) -> io::Result<Option<SystemTime>> {
    match std::fs::metadata(path) {
        Ok(meta) => meta.modified().map(Some),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Removes the file `path`; one that is not there is fine.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// `job.lock`, removed when dropped unless another job has taken it since
/// (this one went stale).
#[derive(Debug)]
pub struct JobLock {
    path: PathBuf,
    token: String,
}

impl Drop for JobLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|text| text == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// What a job writes into its `job.lock`: its process and the time it
/// took the lock.
fn token(now: SystemTime) -> String {
    let nanos = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{} {nanos}\n", std::process::id())
}

/// Whether a lock last stamped at `modified` is a dead holder's at `now`.
/// One stamped after `now` is live.
fn is_stale(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified).is_ok_and(|age| age >= STALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(1_000_000);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + T + Duration::from_secs(secs)
    }

    #[test]
    fn only_a_missing_file_is_fine() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let missing = root.path().join("missing");
        // Below a file: not "missing", an error.
        let below = file.join("child");
        assert!(open_existing(&missing).unwrap().is_none());
        assert!(open_existing(&below).is_err());
        assert_eq!(modified(&missing).unwrap(), None);
        assert!(modified(&below).is_err());
        assert!(modified(&file).unwrap().is_some());
        remove_if_present(&missing).unwrap();
        assert!(remove_if_present(&below).is_err());
        remove_if_present(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn a_job_lock_that_cannot_be_created_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        state.create_dir().unwrap();
        std::fs::create_dir(state.path("job.lock")).unwrap();
        std::fs::write(state.path("job.lock").join("x"), "").unwrap();
        // A directory in its place: exists, is old enough to break (its
        // mtime is now, the clock two hours on), and cannot be removed.
        let later = SystemTime::now() + Duration::from_secs(7200);
        assert!(state.lock_job(later).is_err());
    }

    #[test]
    fn a_state_directory_that_cannot_be_written_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        state.create_dir().unwrap();
        let read_only = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(&state.dir, read_only).unwrap();
        let result = state.lock_job(SystemTime::now());
        std::fs::set_permissions(&state.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            result.map(|_| ()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn a_lock_is_stale_from_two_minutes_on() {
        assert!(!is_stale(at(0), at(119)));
        assert!(is_stale(at(0), at(120)));
        assert!(is_stale(at(0), at(121)));
        assert!(!is_stale(at(10), at(0)));
    }

    #[test]
    fn job_lock_follows_the_clock_it_is_given() {
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        let lock = state.lock_job(at(0)).unwrap().unwrap();
        File::options()
            .write(true)
            .open(state.path("job.lock"))
            .unwrap()
            .set_modified(at(0))
            .unwrap();
        assert!(state.job_running(at(119)));
        assert_eq!(state.lock_job(at(119)).unwrap().map(|_| ()), None);
        assert!(!state.job_running(at(120)));
        let again = state.lock_job(at(120)).unwrap();
        assert!(again.is_some());
        // The stale holder lost it: its drop leaves the new holder's lock.
        drop(lock);
        assert!(state.path("job.lock").exists());
        drop(again);
        assert!(!state.path("job.lock").exists());
        assert!(!state.job_running(at(0)));
    }
}
