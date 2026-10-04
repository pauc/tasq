//! Turning the configured notebook name into a directory, as the script's
//! `NB_HOME` block did: `$NB_DIR/<name>` (default `~/.nb/<name>`) when that
//! is a directory, otherwise `nb notebooks show <name> --path`.

use std::path::{Path, PathBuf};

use tasq_core::store::StoreError;

use crate::nb::Nb;
use crate::sanitize::last_line;
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

/// Resolves `notebook` to its directory. Never runs `nb` when the local
/// candidate exists; `nb` is `None` when it is not installed.
pub fn resolve_notebook(
    notebook: &str,
    options: &NbStoreOptions,
    nb: Option<&Nb>,
) -> Result<PathBuf, StoreError> {
    let config_error = |message: String| StoreError::Config {
        setting: SETTING,
        file: options.config_file.clone(),
        message,
    };
    if notebook.is_empty() {
        return Err(config_error("notebook name must not be empty".to_owned()));
    }
    let candidate = nb_dir(options).map(|dir| dir.join(notebook));
    if let Some(candidate) = &candidate
        && candidate.is_dir()
    {
        return Ok(candidate.clone());
    }
    let looked_in = candidate.as_deref().map_or_else(
        || "no NB_DIR or home directory to look in".to_owned(),
        |c| format!("{} is not a directory", c.display()),
    );
    let Some(nb) = nb else {
        return Err(config_error(format!(
            "notebook {notebook:?} not found: {looked_in}, and nb is not on PATH to ask"
        )));
    };
    let path = match nb.run(&["notebooks", "show", notebook, "--path"]) {
        Ok(out) => last_line(&out),
        Err(e) => {
            return Err(config_error(format!(
                "notebook {notebook:?} not found: {looked_in}, and {e}"
            )));
        }
    };
    if path.is_empty() {
        return Err(config_error(format!(
            "notebook {notebook:?} not found: {looked_in}, and nb printed no path"
        )));
    }
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err(config_error(format!(
            "notebook {notebook:?} resolved by nb to {}, which is not a directory",
            path.display()
        )));
    }
    Ok(path)
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
            bookkeeper: tasq_core::config::Bookkeeper::Auto,
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
        let err = resolve_notebook("", &opts, None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook (set in /c.toml): notebook name must not be empty"
        );
    }

    #[test]
    fn without_nb_dir_or_home_and_no_nb() {
        let err = resolve_notebook("home", &options(&[], None), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "store.notebook: notebook \"home\" not found: no NB_DIR or home directory to look in, and nb is not on PATH to ask"
        );
    }
}
