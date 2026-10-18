//! Change detection for a task file: its modification time plus a hash of
//! its bytes. Two revisions compare equal only when neither changed.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// A snapshot of a file's identity at the moment it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revision {
    mtime: Option<SystemTime>,
    len: u64,
    hash: u64,
}

impl Revision {
    /// The revision of `path` given its current contents (`bytes` must be
    /// what was just read from it, so the hash and the mtime match).
    pub fn of(path: &Path, bytes: &[u8]) -> io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        Ok(Self::from_parts(meta.modified().ok(), bytes))
    }

    /// Builds a revision from an mtime and the file's bytes.
    pub fn from_parts(mtime: Option<SystemTime>, bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            mtime,
            len: bytes.len() as u64,
            hash: hasher.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn same_time_and_bytes_are_equal() {
        let t = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10));
        assert_eq!(
            Revision::from_parts(t, b"abc"),
            Revision::from_parts(t, b"abc")
        );
    }

    #[test]
    fn different_bytes_or_time_differ() {
        let t = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10));
        let later = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(11));
        assert_ne!(
            Revision::from_parts(t, b"abc"),
            Revision::from_parts(t, b"abd")
        );
        assert_ne!(
            Revision::from_parts(t, b"abc"),
            Revision::from_parts(later, b"abc")
        );
        assert_ne!(
            Revision::from_parts(t, b"abc"),
            Revision::from_parts(None, b"abc")
        );
        assert_ne!(
            Revision::from_parts(t, b"ab"),
            Revision::from_parts(t, b"abc"),
            "length is part of the identity"
        );
    }

    #[test]
    fn of_reads_the_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"one").unwrap();
        let first = Revision::of(&path, b"one").unwrap();
        assert_eq!(first, Revision::of(&path, b"one").unwrap());
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(first, Revision::from_parts(Some(mtime), b"one"));
        assert!(Revision::of(&dir.path().join("missing"), b"").is_err());
    }
}
