//! Provisioning a new worktree (plan WT-203): what to copy, link and render
//! into it once `git worktree add` made it. [`plan`] is pure; the
//! `tasq-worktree` crate carries the steps out.
//!
//! Copies and links come from the repository's main checkout, at the same
//! relative path; template sources are relative to the configuration file
//! that lists them. Every target stays inside the worktree.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::config::WorktreeConfig;

/// One provisioning step, with absolute paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Copy a file or directory of the main checkout (`worktree.copies`).
    Copy {
        /// In the main checkout.
        source: PathBuf,
        /// In the worktree.
        target: PathBuf,
    },
    /// Link to a file or directory of the main checkout
    /// (`worktree.symlinks`).
    Symlink {
        /// In the main checkout; what the link points to.
        source: PathBuf,
        /// In the worktree; the link itself.
        target: PathBuf,
    },
    /// Render a template into the worktree (`worktree.templates`).
    Render {
        /// The template file.
        source: PathBuf,
        /// In the worktree.
        target: PathBuf,
    },
}

impl Step {
    /// One line for `--dry-run`: the verb and the target relative to
    /// `worktree`, with where it comes from.
    pub fn describe(&self, worktree: &Path) -> String {
        let rel = |p: &Path| p.strip_prefix(worktree).unwrap_or(p).display().to_string();
        match self {
            Self::Copy { source, target } => {
                format!("copy {} from {}", rel(target), source.display())
            }
            Self::Symlink { source, target } => {
                format!("link {} -> {}", rel(target), source.display())
            }
            Self::Render { source, target } => {
                format!("render {} from {}", rel(target), source.display())
            }
        }
    }
}

/// A configured path that would reach outside the checkout or worktree.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "worktree.{key}: {} must be a relative path that stays inside the worktree",
    .path.display()
)]
pub struct Outside {
    /// The key that lists it (`symlinks`, `copies`, `templates`).
    pub key: &'static str,
    /// The path as configured.
    pub path: PathBuf,
}

/// The steps that provision `worktree`, in the order they run: copies,
/// then links, then templates (so a template can replace a copied file).
/// `main` is the repository's main checkout; `templates_dir` is the
/// directory of the configuration file that lists the templates.
///
/// # Errors
///
/// [`Outside`] for an absolute path, or one with `..`, among the copies,
/// the links or the template targets. A template source may be anywhere.
pub fn plan(
    config: &WorktreeConfig,
    main: &Path,
    worktree: &Path,
    templates_dir: &Path,
) -> Result<Vec<Step>, Outside> {
    let mut steps = Vec::new();
    for path in &config.copies {
        let rel = inside("copies", Path::new(path))?;
        steps.push(Step::Copy {
            source: main.join(rel),
            target: worktree.join(rel),
        });
    }
    for path in &config.symlinks {
        let rel = inside("symlinks", Path::new(path))?;
        steps.push(Step::Symlink {
            source: main.join(rel),
            target: worktree.join(rel),
        });
    }
    for template in &config.templates {
        let rel = inside("templates", &template.target)?;
        steps.push(Step::Render {
            source: templates_dir.join(&template.source),
            target: worktree.join(rel),
        });
    }
    Ok(steps)
}

/// `path` when it is relative and made only of names (`.` allowed).
fn inside<'a>(key: &'static str, path: &'a Path) -> Result<&'a Path, Outside> {
    let only_names = path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    let has_a_name = path.components().any(|c| matches!(c, Component::Normal(_)));
    if only_names && has_a_name {
        Ok(path)
    } else {
        Err(Outside {
            key,
            path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TemplateFile;

    fn config(copies: &[&str], symlinks: &[&str], templates: &[(&str, &str)]) -> WorktreeConfig {
        WorktreeConfig {
            copies: copies.iter().map(|s| (*s).to_owned()).collect(),
            symlinks: symlinks.iter().map(|s| (*s).to_owned()).collect(),
            templates: templates
                .iter()
                .map(|(s, t)| TemplateFile {
                    source: s.into(),
                    target: t.into(),
                })
                .collect(),
            ..WorktreeConfig::default()
        }
    }

    fn plan_of(c: &WorktreeConfig) -> Result<Vec<Step>, Outside> {
        plan(
            c,
            Path::new("/ws/app"),
            Path::new("/ws/app-b"),
            Path::new("/ws"),
        )
    }

    #[test]
    fn copies_then_links_then_templates() {
        let c = config(
            &["./.envrc"],
            &["node_modules"],
            &[("t/env", "config/.env")],
        );
        assert_eq!(
            plan_of(&c),
            Ok(vec![
                Step::Copy {
                    source: "/ws/app/./.envrc".into(),
                    target: "/ws/app-b/./.envrc".into(),
                },
                Step::Symlink {
                    source: "/ws/app/node_modules".into(),
                    target: "/ws/app-b/node_modules".into(),
                },
                Step::Render {
                    source: "/ws/t/env".into(),
                    target: "/ws/app-b/config/.env".into(),
                },
            ])
        );
        assert_eq!(plan_of(&WorktreeConfig::default()), Ok(Vec::new()));
    }

    #[test]
    fn nothing_reaches_outside() {
        for (c, key, path) in [
            (config(&["/etc/passwd"], &[], &[]), "copies", "/etc/passwd"),
            (config(&[], &["../shared"], &[]), "symlinks", "../shared"),
            (config(&[], &["a/../../b"], &[]), "symlinks", "a/../../b"),
            (config(&[], &["."], &[]), "symlinks", "."),
            (config(&[], &[""], &[]), "symlinks", ""),
            (config(&[], &[], &[("t", "/abs")]), "templates", "/abs"),
            (config(&[], &[], &[("t", "../up")]), "templates", "../up"),
        ] {
            assert_eq!(
                plan_of(&c),
                Err(Outside {
                    key,
                    path: path.into()
                }),
                "{key} {path}"
            );
        }
        // A template source may live anywhere.
        let c = config(&[], &[], &[("/etc/tasq/envrc", ".envrc")]);
        assert_eq!(plan_of(&c).unwrap().len(), 1);
        assert_eq!(
            Outside {
                key: "copies",
                path: "../x".into()
            }
            .to_string(),
            "worktree.copies: ../x must be a relative path that stays inside the worktree"
        );
    }

    #[test]
    fn descriptions() {
        let wt = Path::new("/ws/app-b");
        let c = config(&[".envrc"], &["node_modules"], &[("t/env", "config/.env")]);
        let lines: Vec<String> = plan_of(&c)
            .unwrap()
            .iter()
            .map(|s| s.describe(wt))
            .collect();
        assert_eq!(
            lines,
            [
                "copy .envrc from /ws/app/.envrc",
                "link node_modules -> /ws/app/node_modules",
                "render config/.env from /ws/t/env",
            ]
        );
        let elsewhere = Step::Copy {
            source: "/a".into(),
            target: "/b".into(),
        };
        assert_eq!(elsewhere.describe(wt), "copy /b from /a");
    }
}
