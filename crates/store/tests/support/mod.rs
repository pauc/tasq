//! A temporary store directory for the integration tests: nothing in it
//! until a test writes a file, no `nb`, no `git`.

#![allow(dead_code, reason = "each test file uses a subset of the helpers")]

use std::path::{Path, PathBuf};

use tasq_core::model::Workflow;
use tasq_store::TasqStore;

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
