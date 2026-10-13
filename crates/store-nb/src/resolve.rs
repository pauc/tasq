//! Turning the configured notebook into a directory: a name is
//! `$NB_DIR/<name>` (default `~/.nb/<name>`), as the script's `NB_HOME`
//! block found it; a value containing `/` (`~/...` included) is the
//! directory itself (ADR-0023). `nb` is never asked.

use std::path::{Path, PathBuf};

use tasq_core::store::StoreError;

use crate::store::NbStoreOptions;

/// The config key every resolution error names.
pub const SETTING: &str = "store.notebook";

/// nb's data directory: `NB_DIR` from the options' environment, else
/// `<home>/.nb`. `None` when neither is available.
pub fn nb_dir(options: &NbStoreOptions) -> Option<PathBuf> {
    options
        .env
        .iter()
        .find(|(k, v)| k == "NB_DIR" && !v.is_empty())
        .map(|(_, v)| PathBuf::from(v))
        .or_else(|| options.home.as_deref().map(|home| home.join(".nb")))
}

/// Whether `notebook` is a path rather than a name under `NB_DIR`.
pub fn is_path(notebook: &str) -> bool {
    notebook.contains('/') || notebook == "~"
}

/// Resolves `notebook` to its directory, which must exist.
pub fn resolve_notebook(notebook: &str, options: &NbStoreOptions) -> Result<PathBuf, StoreError> {
    let config_error = |message: String| StoreError::Config {
        setting: SETTING,
        file: options.config_file.clone(),
        message,
    };
    if notebook.is_empty() {
        return Err(config_error("notebook name must not be empty".to_owned()));
    }
    let candidate = if is_path(notebook) {
        let tilde = (notebook == "~")
            .then_some("")
            .or_else(|| notebook.strip_prefix("~/"));
        let expanded = match (tilde, options.home.as_deref()) {
            (Some(rest), Some(home)) => home.join(rest),
            (Some(_), None) => {
                return Err(config_error(format!(
                    "notebook path {notebook:?} starts with ~ but there is no home directory"
                )));
            }
            (None, _) => PathBuf::from(notebook),
        };
        if !expanded.is_absolute() {
            return Err(config_error(format!(
                "notebook path {notebook:?} must be absolute or start with ~"
            )));
        }
        expanded
    } else {
        let Some(dir) = nb_dir(options) else {
            return Err(config_error(format!(
                "notebook {notebook:?} not found: no NB_DIR or home directory to look in"
            )));
        };
        dir.join(notebook)
    };
    if candidate.is_dir() {
        Ok(candidate)
    } else {
        Err(config_error(format!(
            "notebook {notebook:?} not found: {} is not a directory",
            candidate.display()
        )))
    }
}

/// `<dir>/.index`.
pub fn index_path(dir: &Path) -> PathBuf {
    dir.join(".index")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::model::Workflow;

    fn options(env: &[(&str, &str)], home: Option<&str>) -> NbStoreOptions {
        NbStoreOptions {
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            home: home.map(PathBuf::from),
            config_file: None,
            workflow: Workflow::default(),
            sync_interval: 0,
        }
    }

    #[test]
    fn nb_dir_prefers_the_variable_over_home() {
        assert_eq!(
            nb_dir(&options(&[("NB_DIR", "/data/nb")], Some("/home/u"))),
            Some(PathBuf::from("/data/nb"))
        );
        assert_eq!(
            nb_dir(&options(&[("NB_DIR", "")], Some("/home/u"))),
            Some(PathBuf::from("/home/u/.nb")),
            "an empty NB_DIR is unset, as in the shell's ${{NB_DIR:-...}}"
        );
        assert_eq!(
            nb_dir(&options(&[("HOME", "/ignored")], Some("/home/u"))),
            Some(PathBuf::from("/home/u/.nb"))
        );
        assert_eq!(nb_dir(&options(&[], None)), None);
    }

    #[test]
    fn index_path_is_dot_index() {
        assert_eq!(
            index_path(Path::new("/nb/home")),
            PathBuf::from("/nb/home/.index")
        );
    }

    #[test]
    fn empty_name_is_a_config_error() {
        let mut opts = options(&[], Some("/h"));
        opts.config_file = Some("/c.toml".into());
        let err = resolve_notebook("", &opts).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook (set in /c.toml): notebook name must not be empty"
        );
    }

    #[test]
    fn without_nb_dir_or_home() {
        let err = resolve_notebook("home", &options(&[], None)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook: notebook \"home\" not found: no NB_DIR or home directory to look in"
        );
    }

    #[test]
    fn names_resolve_under_nb_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("work")).unwrap();
        let dir = tmp.path().display().to_string();
        let opts = options(&[("NB_DIR", &dir)], None);
        assert_eq!(
            resolve_notebook("work", &opts).unwrap(),
            tmp.path().join("work")
        );
        let err = resolve_notebook("play", &opts).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "store.notebook: notebook \"play\" not found: {} is not a directory",
                tmp.path().join("play").display()
            )
        );
    }

    #[test]
    fn paths_are_used_as_they_are() {
        let tmp = tempfile::tempdir().unwrap();
        let notebook = tmp.path().join("notes");
        std::fs::create_dir_all(&notebook).unwrap();
        let no_nb_dir = options(&[], None);
        let given = notebook.display().to_string();
        assert_eq!(resolve_notebook(&given, &no_nb_dir).unwrap(), notebook);
        let home = tmp.path().display().to_string();
        let opts = options(&[], Some(&home));
        assert_eq!(resolve_notebook("~/notes", &opts).unwrap(), notebook);
        assert_eq!(resolve_notebook("~", &opts).unwrap(), tmp.path());
        let err = resolve_notebook("~/notes", &no_nb_dir).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook: notebook path \"~/notes\" starts with ~ but there is no home directory"
        );
        let err = resolve_notebook("rel/notes", &opts).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook: notebook path \"rel/notes\" must be absolute or start with ~"
        );
        let err = resolve_notebook("~/gone", &opts).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "store.notebook: notebook \"~/gone\" not found: {} is not a directory",
                tmp.path().join("gone").display()
            )
        );
    }

    #[test]
    fn what_counts_as_a_path() {
        assert!(is_path("/abs"));
        assert!(is_path("rel/x"));
        assert!(is_path("~"));
        assert!(is_path("~/x"));
        assert!(!is_path("home"));
        assert!(!is_path("my~notes"));
        assert!(!is_path("~notes"), "~user is not expanded");
    }
}
