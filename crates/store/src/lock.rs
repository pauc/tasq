//! The store lock: `.tasq/lock` inside the store directory, held by a write
//! from reading the id map to the end of the write (ADR-0026).
//!
//! The lock is a file created exclusively. A writer that dies leaves it
//! behind; a lock [`STALE_AFTER`] or older is removed by the next writer. A
//! live lock is waited on. There is no hand-over to any job: dropping the
//! lock removes the file. Reads never lock.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{Local, NaiveDateTime, TimeZone};
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
        Self::acquire_within(dir, clock, None).map(|lock| lock.expect("no limit to pass"))
    }

    /// [`StoreLock::acquire`], giving up once a lock is still held `limit`
    /// after the call (`tasq store lock --timeout`): `None` then. The limit
    /// is wall time; staleness is still judged by `clock`. No limit, or one
    /// too large for the clock, waits as long as the lock is held.
    pub fn acquire_within(
        dir: &Path,
        clock: &dyn Clock,
        limit: Option<Duration>,
    ) -> std::io::Result<Option<Self>> {
        let deadline = limit.and_then(|limit| Instant::now().checked_add(limit));
        let path = lock_path(dir);
        std::fs::create_dir_all(dir.join(STATE_DIR))?;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Some(Self { path })),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A stale lock that cannot be removed is waited on
                    // like a live one, up to the limit.
                    if is_stale(&path, instant(clock)) && std::fs::remove_file(&path).is_ok() {
                        continue;
                    }
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Ok(None);
                    }
                    std::thread::sleep(POLL);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Marks the lock as touched now, so that a holder that keeps it longer
    /// than [`STALE_AFTER`] (`tasq store lock`) is not taken for dead.
    pub fn refresh(&self) -> std::io::Result<()> {
        OpenOptions::new()
            .write(true)
            .open(&self.path)?
            .set_modified(SystemTime::now())
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
    instant_at(clock.now())
}

/// The instant of `local`, a wall-clock time in the local zone, read as
/// [`instant`] reads the clock.
pub fn instant_at(local: NaiveDateTime) -> SystemTime {
    Local
        .from_local_datetime(&local)
        .earliest()
        .map_or_else(|| local.and_utc().into(), Into::into)
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
    fn acquire_within_gives_up_on_a_live_lock_at_the_limit() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let held = StoreLock::acquire(&dir, &clock()).unwrap();
        touch(held.path(), now());
        assert!(
            StoreLock::acquire_within(&dir, &clock(), Some(Duration::ZERO))
                .unwrap()
                .is_none(),
            "no wait at all"
        );
        let started = std::time::Instant::now();
        assert!(
            StoreLock::acquire_within(&dir, &clock(), Some(Duration::from_millis(200)))
                .unwrap()
                .is_none()
        );
        assert!(started.elapsed() >= Duration::from_millis(200), "waited");
        assert!(held.path().is_file(), "the holder keeps its lock");
        drop(held);
        let lock = StoreLock::acquire_within(&dir, &clock(), Some(Duration::ZERO))
            .unwrap()
            .unwrap();
        assert_eq!(lock.path(), lock_path(&dir));
    }

    #[test]
    fn acquire_within_breaks_a_stale_lock_and_has_no_limit_past_the_clock() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let path = lock_path(&dir);
        std::fs::create_dir_all(dir.join(STATE_DIR)).unwrap();
        std::fs::write(&path, "").unwrap();
        touch(&path, now() - STALE_AFTER);
        let lock = StoreLock::acquire_within(&dir, &clock(), Some(Duration::ZERO))
            .unwrap()
            .unwrap();
        assert_eq!(lock.path(), path);
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(StoreLock::acquire_within(
                &waiter,
                &clock(),
                Some(Duration::MAX),
            ));
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "a limit past the clock waits"
        );
        drop(lock);
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_refreshed_lock_is_not_stale() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let lock = StoreLock::acquire(&dir, &clock()).unwrap();
        let wall = SystemTime::now();
        touch(lock.path(), wall - STALE_AFTER);
        assert!(is_stale(lock.path(), wall));
        lock.refresh().unwrap();
        assert!(!is_stale(lock.path(), wall));
        std::fs::remove_file(lock.path()).unwrap();
        assert!(lock.refresh().is_err(), "a lock that is gone");
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_lock_that_cannot_be_removed_is_waited_on_up_to_the_limit() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_owned();
        let path = lock_path(&dir);
        let state = dir.join(STATE_DIR);
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(&path, "").unwrap();
        touch(&path, now() - STALE_AFTER);
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = std::fs::write(state.join("probe"), "").is_ok();
        let taken = StoreLock::acquire_within(&dir, &clock(), Some(Duration::from_millis(50)));
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
        if writable {
            return; // root ignores the mode
        }
        assert!(taken.unwrap().is_none());
        assert!(path.is_file());
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
