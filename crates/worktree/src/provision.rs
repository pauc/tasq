//! Carrying out a provisioning plan ([`tasq_core::worktree::provision`]).
//!
//! In two phases, so that a broken template stops the launch before
//! anything exists: [`prepare`] reads and renders every template, then the
//! caller creates the worktree, then [`apply`] copies, links and writes.

use std::io;
use std::path::{Path, PathBuf};

use tasq_core::worktree::provision::Step;
use tasq_core::worktree::vars::Vars;
use thiserror::Error;

/// A step ready to run: templates are rendered already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    /// Copy `source` (a file or a directory) to `target`.
    Copy {
        /// In the main checkout.
        source: PathBuf,
        /// In the worktree.
        target: PathBuf,
    },
    /// Make `target` a link to `source`.
    Symlink {
        /// In the main checkout.
        source: PathBuf,
        /// In the worktree.
        target: PathBuf,
    },
    /// Write the rendered `text` to `target`, replacing what is there.
    Write {
        /// In the worktree.
        target: PathBuf,
        /// The rendered template.
        text: String,
    },
}

/// Why provisioning failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProvisionError {
    /// A template is missing, unreadable or does not render.
    #[error("template {}: {message}", .path.display())]
    Template {
        /// The template file.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// Writing into the worktree failed.
    #[error("{}: {message}", .path.display())]
    Write {
        /// The file or directory being made.
        path: PathBuf,
        /// The OS error.
        message: String,
    },
}

/// Reads and renders every template of `steps` with `vars`; copies and
/// links pass through unchanged.
///
/// # Errors
///
/// [`ProvisionError::Template`] for a template that is missing, unreadable
/// or names an unknown variable.
pub fn prepare(steps: &[Step], vars: &Vars) -> Result<Vec<Ready>, ProvisionError> {
    steps
        .iter()
        .map(|step| match step {
            Step::Copy { source, target } => Ok(Ready::Copy {
                source: source.clone(),
                target: target.clone(),
            }),
            Step::Symlink { source, target } => Ok(Ready::Symlink {
                source: source.clone(),
                target: target.clone(),
            }),
            Step::Render { source, target } => {
                let failed = |message: String| ProvisionError::Template {
                    path: source.clone(),
                    message,
                };
                let template =
                    std::fs::read_to_string(source).map_err(|e| failed(e.to_string()))?;
                let text = vars.render(&template).map_err(|e| failed(e.to_string()))?;
                Ok(Ready::Write {
                    target: target.clone(),
                    text,
                })
            }
        })
        .collect()
}

/// Runs the prepared steps in order. A copy or link whose source does not
/// exist, or whose target the checkout already has, is skipped with a
/// warning; the warnings come back.
///
/// # Errors
///
/// [`ProvisionError::Write`] when the filesystem refuses a copy, a link or
/// a write.
pub fn apply(steps: &[Ready]) -> Result<Vec<String>, ProvisionError> {
    let mut warnings = Vec::new();
    for step in steps {
        match step {
            Ready::Copy { source, target } | Ready::Symlink { source, target } => {
                let verb = if matches!(step, Ready::Copy { .. }) {
                    "copy"
                } else {
                    "link"
                };
                if std::fs::symlink_metadata(source).is_err() {
                    warnings.push(format!("{verb}: {} not found, skipped", source.display()));
                    continue;
                }
                if std::fs::symlink_metadata(target).is_ok() {
                    warnings.push(format!(
                        "{verb}: {} is in the checkout already, kept",
                        target.display()
                    ));
                    continue;
                }
                in_place(target, |t| match step {
                    Ready::Copy { .. } => copy_tree(source, t),
                    _ => std::os::unix::fs::symlink(source, t),
                })?;
            }
            Ready::Write { target, text } => in_place(target, |t| std::fs::write(t, text))?,
        }
    }
    Ok(warnings)
}

/// Creates `target`'s parent directories, then `make(target)`.
fn in_place(
    target: &Path,
    make: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), ProvisionError> {
    let failed = |e: io::Error| ProvisionError::Write {
        path: target.to_path_buf(),
        message: e.to_string(),
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    make(target).map_err(failed)
}

/// Copies a file, a link (as a link) or a directory with everything in it.
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    let kind = std::fs::symlink_metadata(source)?.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(source)?, target)
    } else if kind.is_dir() {
        std::fs::create_dir(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(source, target).map(|_| ())
    }
}
