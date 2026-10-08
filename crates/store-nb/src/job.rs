//! The detached job that commits after a write, and pulls and pushes every
//! `store.sync_interval` seconds (ADR-0023), plus the files it coordinates
//! through in `<notebook>/.git/tasq/`:
//!
//! - `lock`: held by a writer from before its write until its job exits, so
//!   jobs never interleave and a pull never runs under a write. Older than
//!   [`STALE_AFTER`] it is considered abandoned and removed.
//! - `failed`: the last job's failure, one line, for the next command to
//!   report.
//! - `last-sync`: the time of the last sync attempt, in Unix seconds.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The job's state directory, relative to the notebook.
pub const STATE_DIR: &str = ".git/tasq";

/// A lock older than this is left over from a job that died.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

/// How often a waiting writer looks at the lock again.
const POLL: Duration = Duration::from_millis(10);

/// The job, run by `sh` with the notebook as working directory and
/// `lock failed last-sync message sync` as `$1`..`$5`. It commits what is
/// staged by `git add -A` (nothing when the write changed nothing git
/// tracks), then, when `$5` is `1` and a remote exists, records the time
/// and runs `git pull --rebase` and `git push` with SSH in batch mode so it
/// never prompts. A failed rebase is aborted. Any failure writes
/// `<step>: <git output>` to the `failed` file. The lock goes on exit.
pub const SCRIPT: &str = r#"lock=$1 failed=$2 last=$3 msg=$4 sync=$5
trap 'rm -f "$lock"' EXIT
fail() { printf '%s: %s\n' "$1" "$2" | tr '\n' ' ' >"$failed"; exit 1; }
out=$(git add -A 2>&1) || fail staging "$out"
if ! git diff --cached --quiet 2>/dev/null; then
  out=$(git commit -q -m "$msg" 2>&1) || fail committing "$out"
fi
[ "$sync" = 1 ] || exit 0
[ -n "$(git remote)" ] || exit 0
date +%s >"$last"
if [ -z "${GIT_SSH_COMMAND:-}" ] && [ -z "${GIT_SSH:-}" ]; then
  GIT_SSH_COMMAND="$(git config core.sshCommand || echo ssh) -o BatchMode=yes"
  export GIT_SSH_COMMAND
fi
out=$(git pull --rebase -q 2>&1) || { git rebase --abort >/dev/null 2>&1; fail pulling "$out"; }
out=$(git push -q 2>&1) || fail pushing "$out"
"#;

/// The coordination files of one notebook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    dir: PathBuf,
}

impl State {
    /// The state of the notebook at `notebook`.
    pub fn new(notebook: &Path) -> Self {
        Self {
            dir: notebook.join(STATE_DIR),
        }
    }

    /// `.git/tasq/lock`.
    pub fn lock_path(&self) -> PathBuf {
        self.dir.join("lock")
    }

    /// `.git/tasq/failed`.
    pub fn failed_path(&self) -> PathBuf {
        self.dir.join("failed")
    }

    /// `.git/tasq/last-sync`.
    pub fn last_sync_path(&self) -> PathBuf {
        self.dir.join("last-sync")
    }

    /// Takes the lock once nobody holds it, removing a stale one, and
    /// returns its path. Waits as long as a live job holds it.
    pub fn acquire(&self) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.lock_path();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path, SystemTime::now()) {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        std::thread::sleep(POLL);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The last job's failure, removed so it is reported once. `None` when
    /// the last job succeeded or there is nothing to read.
    pub fn take_failure(&self) -> Option<String> {
        let path = self.failed_path();
        let text = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// When the last sync was attempted, in Unix seconds.
    pub fn last_sync(&self) -> Option<u64> {
        std::fs::read_to_string(self.last_sync_path())
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Records a sync attempt at `now` (Unix seconds).
    pub fn record_sync(&self, now: u64) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.last_sync_path(), format!("{now}\n"))
    }
}

/// Whether the lock file at `path` was last touched [`STALE_AFTER`] or more
/// before `now`. A lock whose age cannot be read is not stale.
fn is_stale(path: &Path, now: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= STALE_AFTER)
}

/// Whether a write at `now` should also sync: automatic sync is on
/// (`interval > 0`) and the last attempt, if any, is `interval` seconds or
/// more in the past.
pub fn sync_due(last: Option<u64>, now: u64, interval: u64) -> bool {
    interval > 0 && last.is_none_or(|last| now.saturating_sub(last) >= interval)
}

/// The current time in Unix seconds.
///
/// Reason: reads the system clock; [`sync_due`] holds the logic.
#[mutants::skip]
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Starts [`SCRIPT`] detached in `notebook` and returns once it runs in the
/// background: `sh` puts the job in a subshell and exits, so nothing waits
/// for the commit and no zombie is left behind. The job owns `lock` from
/// here on. `env` is the whole environment it gets.
///
/// Reason: spawns a process; the commits and syncs it makes are asserted by
/// the integration tests on temporary repositories.
#[mutants::skip]
pub fn spawn(
    notebook: &Path,
    env: &[(String, String)],
    state: &State,
    lock: &Path,
    message: &str,
    sync: bool,
) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("(\n{SCRIPT}) </dev/null >/dev/null 2>&1 &"))
        .arg("sh")
        .arg(lock)
        .arg(state.failed_path())
        .arg(state.last_sync_path())
        .arg(message)
        .arg(if sync { "1" } else { "0" })
        .current_dir(notebook)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "sh could not start the job ({status})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `state.acquire()` on a thread, `None` when it has not returned
    /// within two seconds (a wait that would never end).
    fn acquire_soon(state: &State) -> Option<std::io::Result<PathBuf>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let state = state.clone();
        std::thread::spawn(move || {
            let _ = tx.send(state.acquire());
        });
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }

    #[test]
    fn paths_live_under_dot_git_tasq() {
        let s = State::new(Path::new("/nb/home"));
        assert_eq!(s.lock_path(), PathBuf::from("/nb/home/.git/tasq/lock"));
        assert_eq!(s.failed_path(), PathBuf::from("/nb/home/.git/tasq/failed"));
        assert_eq!(
            s.last_sync_path(),
            PathBuf::from("/nb/home/.git/tasq/last-sync")
        );
    }

    #[test]
    fn sync_is_due_after_the_interval_only() {
        assert!(sync_due(None, 100, 3600), "never synced");
        assert!(!sync_due(None, 100, 0), "0 disables");
        assert!(!sync_due(Some(100), 5000, 0), "0 disables");
        assert!(!sync_due(Some(100), 3699, 3600));
        assert!(sync_due(Some(100), 3700, 3600));
        assert!(sync_due(Some(100), 3701, 3600));
        assert!(
            !sync_due(Some(200), 100, 3600),
            "a last sync in the future is not old"
        );
    }

    #[test]
    fn staleness_is_the_age_of_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("lock");
        std::fs::write(&lock, "").unwrap();
        let modified = std::fs::metadata(&lock).unwrap().modified().unwrap();
        assert!(!is_stale(&lock, modified));
        assert!(!is_stale(
            &lock,
            modified + STALE_AFTER - Duration::from_secs(1)
        ));
        assert!(is_stale(&lock, modified + STALE_AFTER));
        assert!(!is_stale(&lock, modified - Duration::from_secs(5)));
        assert!(!is_stale(&dir.path().join("none"), modified + STALE_AFTER));
    }

    #[test]
    fn acquire_takes_a_free_lock_and_replaces_a_stale_one() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path());
        let lock = state.acquire().unwrap();
        assert_eq!(lock, state.lock_path());
        assert!(lock.is_file());
        // An abandoned lock: older than STALE_AFTER.
        let old = SystemTime::now() - STALE_AFTER - Duration::from_secs(1);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(acquire_soon(&state).expect("not waited on").unwrap(), lock);
        let modified = std::fs::metadata(&lock).unwrap().modified().unwrap();
        assert!(modified > old, "a fresh lock replaced the stale one");
    }

    #[test]
    fn acquire_waits_for_a_live_lock() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path());
        let lock = state.acquire().unwrap();
        let release = std::thread::spawn({
            let lock = lock.clone();
            move || {
                std::thread::sleep(Duration::from_millis(150));
                std::fs::remove_file(lock).unwrap();
            }
        });
        let started = std::time::Instant::now();
        state.acquire().unwrap();
        assert!(started.elapsed() >= Duration::from_millis(150));
        release.join().unwrap();
    }

    #[test]
    fn acquire_reports_a_lock_it_cannot_create() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path());
        let state_dir = dir.path().join(STATE_DIR);
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = acquire_soon(&state);
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let root = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
        };
        if root {
            return; // root ignores the permission bits
        }
        let err = result.expect("an error, not a wait").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn acquire_reports_a_state_directory_it_cannot_make() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".git"), "gitdir: elsewhere\n").unwrap();
        assert!(State::new(dir.path()).acquire().is_err());
    }

    #[test]
    fn failure_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path());
        assert_eq!(state.take_failure(), None);
        std::fs::create_dir_all(dir.path().join(STATE_DIR)).unwrap();
        std::fs::write(state.failed_path(), "pushing: rejected \n").unwrap();
        assert_eq!(state.take_failure().as_deref(), Some("pushing: rejected"));
        assert_eq!(state.take_failure(), None);
        std::fs::write(state.failed_path(), "  \n").unwrap();
        assert_eq!(state.take_failure(), None, "blank is no failure");
        assert!(!state.failed_path().exists());
    }

    #[test]
    fn last_sync_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path());
        assert_eq!(state.last_sync(), None);
        state.record_sync(1_700_000_000).unwrap();
        assert_eq!(
            std::fs::read_to_string(state.last_sync_path()).unwrap(),
            "1700000000\n"
        );
        assert_eq!(state.last_sync(), Some(1_700_000_000));
        std::fs::write(state.last_sync_path(), "soon").unwrap();
        assert_eq!(state.last_sync(), None);
    }
}
