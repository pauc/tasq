//! The store lock: `.tasq/lock` inside the store directory, held by a write
//! from reading the id map to the end of the write (ADR-0026).
//!
//! The lock is a file created exclusively. A writer that dies leaves it
//! behind; a lock [`STALE_AFTER`] or older is removed by the next writer. A
//! live lock is waited on. There is no hand-over to any job: dropping the
//! lock removes the file. Reads never lock.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{Local, TimeZone};
use tasq_core::clock::Clock;

/// The directory inside the store that holds the lock.
pub const STATE_DIR: &str = ".tasq";

/// A lock this old was left behind by a writer that died.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

/// How often a waiter looks at a live lock.
const POLL: Duration = Duration::from_millis(10);

/// `<dir>/.tasq/lock`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(STATE_DIR).join("lock")
}

/// The held lock of a store; dropping it removes the file.
#[derive(Debug)]
pub struct StoreLock {
    path: PathBuf,
}

impl StoreLock {
    /// Takes the lock of the store at `dir` once nobody holds it: creates
    /// `dir` and its `.tasq/` when they do not exist (the first write
    /// creates the store), removes a lock that is stale by `clock`, and
    /// waits as long as a live one is held. The clock is read on every
    /// poll, so a holder that dies while a waiter is polling is broken
    /// once its lock reaches [`STALE_AFTER`].
    pub fn acquire(dir: &Path, clock: &dyn Clock) -> std::io::Result<Self> {
        let path = lock_path(dir);
        std::fs::create_dir_all(dir.join(STATE_DIR))?;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path, instant(clock)) {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        std::thread::sleep(POLL);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Whether the lock file at `path` was last touched [`STALE_AFTER`] or more
/// before `now`. A lock whose age cannot be read, or that is from the
/// future, is not stale.
pub fn is_stale(path: &Path, now: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= STALE_AFTER)
}

/// The instant `clock` says it is: its local time read in the local zone
/// (the inverse of the store's `local_time`). A local time that exists
/// twice (a daylight saving fold) is the earlier instant; one that does
/// not exist (skipped by the change) is read as UTC.
pub fn instant(clock: &dyn Clock) -> SystemTime {
    let now = clock.now();
    Local
        .from_local_datetime(&now)
        .earliest()
        .map_or_else(|| now.and_utc().into(), Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::mpsc::Receiver;

    use chrono::{DateTime, NaiveDateTime};
    use tasq_core::clock::FixedClock;

    /// Whatever the wall clock says, the lock tests run at this time.
    fn clock() -> FixedClock {
        FixedClock::at("2026-10-07 09:30")
    }

    /// The instant of [`clock`].
    fn now() -> SystemTime {
        instant(&clock())
    }

    /// A clock that advances by a minute on every reading, so a waiter
    /// that re-reads it sees a lock go stale.
    struct Ticking(Cell<NaiveDateTime>);

    impl Clock for Ticking {
        fn now(&self) -> NaiveDateTime {
            let at = self.0.get();
            self.0.set(at + chrono::Duration::minutes(1));
            at
        }
    }

    fn touch(path: &Path, at: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    /// `StoreLock::acquire(dir, now())` on a thread; its result arrives on
    /// the receiver.
    fn acquire_in_background(dir: &Path) -> Receiver<std::io::Result<StoreLock>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = dir.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(StoreLock::acquire(&dir, &clock()));
        });
        rx
    }

    /// `acquire` on a thread, `None` when it has not returned within two
    /// seconds (a wait that would never end).
    fn acquire_soon(dir: &Path) -> Option<std::io::Result<StoreLock>> {
        acquire_in_background(dir)
            .recv_timeout(Duration::from_secs(2))
            .ok()
    }

    #[test]
    fn the_lock_lives_under_dot_tasq() {
        assert_eq!(
            lock_path(Path::new("/stores/default")),
            PathBuf::from("/stores/default/.tasq/lock")
        );
    }

    #[test]
    fn acquire_creates_the_store_directory_and_drop_releases() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("store");
        let lock = StoreLock::acquire(&dir, &clock()).unwrap();
        assert_eq!(lock.path(), lock_path(&dir));
        assert!(lock.path().is_file(), "held");
        assert!(dir.is_dir());
        drop(lock);
        assert!(!lock_path(&dir).exists(), "released");
        assert!(dir.join(STATE_DIR).is_dir(), "the state directory stays");
    }

    #[test]
    fn a_live_lock_is_waited_on_until_it_is_released() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let held = StoreLock::acquire(&dir, &clock()).unwrap();
        touch(held.path(), now());
        let waiting = acquire_in_background(&dir);
        assert!(
            waiting.recv_timeout(Duration::from_millis(300)).is_err(),
            "still waiting while the lock is held"
        );
        drop(held);
        let taken = waiting
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert!(taken.path().is_file());
    }

    #[test]
    fn a_stale_lock_is_broken_and_a_lock_from_the_future_is_not() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let path = lock_path(&dir);
        std::fs::create_dir_all(dir.join(STATE_DIR)).unwrap();
        std::fs::write(&path, "").unwrap();
        touch(&path, now() - STALE_AFTER + Duration::from_millis(1));
        assert!(!is_stale(&path, now()), "one millisecond short");
        assert!(acquire_soon(&dir).is_none(), "waited on");

        touch(&path, now() - STALE_AFTER);
        assert!(is_stale(&path, now()), "exactly the threshold");
        let lock = acquire_soon(&dir).unwrap().unwrap();
        assert_eq!(lock.path(), path);
        drop(lock);

        std::fs::write(&path, "").unwrap();
        touch(&path, now() + Duration::from_secs(3600));
        assert!(!is_stale(&path, now()), "a lock from the future is live");
        assert!(acquire_soon(&dir).is_none());
        assert!(!is_stale(&dir.join("absent"), now()), "no file, no age");
    }

    #[test]
    fn a_waiter_breaks_a_lock_that_goes_stale_while_it_polls() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let path = lock_path(&dir);
        std::fs::create_dir_all(dir.join(STATE_DIR)).unwrap();
        std::fs::write(&path, "").unwrap();
        touch(&path, now());
        // Live on the first reading (age 0), live on the second (one
        // minute), stale on the third (two minutes): the clock is read
        // on every poll, not once on entry.
        let ticking = Ticking(Cell::new(clock().0));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(StoreLock::acquire(&dir, &ticking));
        });
        let lock = rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(lock.path(), path);
    }

    #[test]
    fn the_clock_is_read_in_the_local_zone() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let local = FixedClock(DateTime::<Local>::from(at).naive_local());
        assert_eq!(instant(&local), at);
        assert!(
            instant(&FixedClock::at("1960-01-01 00:00")) < SystemTime::UNIX_EPOCH,
            "before the epoch"
        );
    }

    #[test]
    fn acquire_reports_a_state_directory_it_cannot_create() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("store");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(STATE_DIR), "a file where the directory goes").unwrap();
        let err = StoreLock::acquire(&dir, &clock()).unwrap_err();
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotADirectory
            ),
            "{err:?}"
        );
    }
}
