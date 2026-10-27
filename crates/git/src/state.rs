//! The plugin's own files, under `<store>/.tasq/plugins/git/`:
//!
//! - `job.lock`: locked by the running job (one per store) with an OS
//!   file lock, which goes with the job: a job that died blocks nothing,
//!   and one that runs long (a slow fetch) is never taken for dead.
//! - `pending`: touched by every `post-write`; the job removes it before
//!   each commit and goes round again while it is back.
//! - `writes`: one line per `post-write` not yet committed (`<command>
//!   <ids>`), for the commit message. Hooks append to it and the job drops
//!   what it committed, both under an advisory lock on the file.
//! - `last-fetch`: when the job last looked at the remote, in Unix
//!   seconds; `store-open` starts a job once it is older than the interval.
//! - `warnings`: the lasting warnings tasq shows (`docs/plugins.md`).

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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

    /// Whether a job holds `job.lock`. One that cannot be opened is no
    /// job's: the job started for it reports why.
    pub fn job_running(&self) -> bool {
        open_existing(&self.path("job.lock"))
            .ok()
            .flatten()
            .is_some_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
    }

    /// Takes `job.lock`; `None` while another job holds it.
    pub fn lock_job(&self) -> io::Result<Option<JobLock>> {
        let file = self.job_lock_file()?;
        match file.try_lock() {
            Ok(()) => Ok(Some(JobLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }

    /// Takes `job.lock`, waiting while another job holds it.
    pub fn wait_job(&self) -> io::Result<JobLock> {
        let file = self.job_lock_file()?;
        file.lock()?;
        Ok(JobLock { _file: file })
    }

    /// `job.lock`, created if missing; it stays when the job is done.
    fn job_lock_file(&self) -> io::Result<File> {
        self.create_dir()?;
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path("job.lock"))
    }

    /// Whether the remote is due a look at `now`: `last-fetch` is missing,
    /// unreadable, from the future (the clock went back) or at least
    /// `interval` old.
    pub fn fetch_due(&self, now: SystemTime, interval: Duration) -> bool {
        self.last_fetch()
            .is_none_or(|last| now.duration_since(last).map_or(true, |age| age >= interval))
    }

    /// When the job last looked at the remote; `None` when it never did,
    /// or `last-fetch` cannot be read.
    pub fn last_fetch(&self) -> Option<SystemTime> {
        std::fs::read_to_string(self.path("last-fetch"))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    /// Records `now` as the last look at the remote.
    pub fn record_fetch(&self, now: SystemTime) -> io::Result<()> {
        self.create_dir()?;
        let secs = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        std::fs::write(self.path("last-fetch"), format!("{secs}\n"))
    }

    /// Replaces the warning lines starting with `kind` by `kind` followed
    /// by `message`, or removes them when `message` is `None`. The other
    /// lines are kept; the file goes when none is left.
    pub fn set_warning(&self, kind: &str, message: Option<&str>) -> io::Result<()> {
        self.set_warnings(kind, message.as_slice())
    }

    /// Whether a warning line starts with `kind`.
    pub fn has_warning(&self, kind: &str) -> io::Result<bool> {
        Ok(self.warnings()?.lines().any(|line| line.starts_with(kind)))
    }

    /// The text of `warnings`, empty when there is none.
    fn warnings(&self) -> io::Result<String> {
        let mut text = String::new();
        if let Some(mut file) = open_existing(&self.path("warnings"))? {
            file.read_to_string(&mut text)?;
        }
        Ok(text)
    }

    /// [`Self::set_warning`] with one line per message, none removing them.
    pub fn set_warnings(&self, kind: &str, messages: &[&str]) -> io::Result<()> {
        let path = self.path("warnings");
        let mut lines: Vec<String> = self
            .warnings()?
            .lines()
            .filter(|line| !line.starts_with(kind))
            .map(str::to_owned)
            .collect();
        lines.extend(messages.iter().map(|message| format!("{kind}{message}")));
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

/// Removes the file `path`; one that is not there is fine.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The lock on `job.lock`, released when dropped (or when the job dies).
#[derive(Debug)]
pub struct JobLock {
    _file: File,
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
        remove_if_present(&missing).unwrap();
        assert!(remove_if_present(&below).is_err());
        remove_if_present(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn a_job_lock_that_cannot_be_opened_is_an_error_and_no_job() {
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        state.create_dir().unwrap();
        std::fs::create_dir(state.path("job.lock")).unwrap();
        assert!(state.lock_job().is_err());
        assert!(state.wait_job().is_err());
        assert!(!state.job_running());
    }

    #[test]
    fn a_state_directory_that_cannot_be_written_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        state.create_dir().unwrap();
        let read_only = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(&state.dir, read_only).unwrap();
        let result = state.lock_job();
        std::fs::set_permissions(&state.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            result.map(|_| ()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn one_job_holds_the_lock_until_it_drops_it() {
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        assert!(!state.job_running(), "no lock file yet");
        let lock = state.lock_job().unwrap().unwrap();
        assert!(state.job_running());
        assert!(state.lock_job().unwrap().is_none());
        let other = File::open(state.path("job.lock")).unwrap();
        assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
        drop(lock);
        assert!(!state.job_running());
        assert!(state.path("job.lock").exists(), "the file stays");
        let again = state.wait_job().unwrap();
        assert!(state.job_running());
        assert!(state.lock_job().unwrap().is_none());
        drop(again);
        assert!(state.lock_job().unwrap().is_some());
    }

    #[test]
    fn a_fetch_is_due_once_the_interval_has_passed() {
        let root = tempfile::tempdir().unwrap();
        let state = State::of(root.path());
        let minute = Duration::from_secs(60);
        assert!(state.fetch_due(at(0), minute), "never fetched");
        state.record_fetch(at(0)).unwrap();
        assert_eq!(
            std::fs::read_to_string(state.path("last-fetch")).unwrap(),
            format!("{}\n", T.as_secs())
        );
        assert!(!state.fetch_due(at(59), minute));
        assert!(state.fetch_due(at(60), minute));
        assert!(state.fetch_due(at(61), minute));
        assert!(state.fetch_due(at(0), Duration::ZERO));
        assert!(!state.fetch_due(at(0), minute), "just now");
        let earlier = SystemTime::UNIX_EPOCH + T - Duration::from_secs(1);
        assert!(state.fetch_due(earlier, minute), "the clock went back");
        std::fs::write(state.path("last-fetch"), "soon\n").unwrap();
        assert!(state.fetch_due(at(0), minute));
    }
}
