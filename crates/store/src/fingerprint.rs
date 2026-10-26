//! [`fingerprint()`]: what the TUI compares to notice that task files
//! changed under it (another terminal, a hand edit, a plugin's job).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use crate::store::is_task_file_name;

/// The names, lengths and modification times of the task files in `dir`
/// (the `.md` regular files the store reads), hashed into one number: equal while
/// none was added, removed, renamed, rewritten or touched. Nothing else
/// counts, so the lock and `.tasq/` leave it alone. A directory that does
/// not exist is the same as an empty one. Symlinks are followed, as the
/// store reads them: a dangling one (or a file that vanished meanwhile) is
/// left out. Only meaningful within one process.
///
/// # Errors
///
/// The directory cannot be listed, or a listed file's metadata cannot be
/// read for another reason than it being gone (a symlink loop).
pub fn fingerprint(dir: &Path) -> std::io::Result<u64> {
    let mut files = Vec::new();
    let listing = match std::fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(hash(&files)),
        Err(e) => return Err(e),
    };
    for entry in listing {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_task_file_name(&name) {
            continue;
        }
        let meta = match std::fs::metadata(entry.path()) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if !meta.is_file() {
            continue;
        }
        files.push((name, meta.len(), meta.modified()?));
    }
    files.sort();
    Ok(hash(&files))
}

fn hash(files: &[(String, u64, std::time::SystemTime)]) -> u64 {
    let mut hasher = DefaultHasher::new();
    files.hash(&mut hasher);
    hasher.finish()
}
