//! Running the `nb` command line tool, isolated to an injected environment.
//!
//! The store never inherits the process environment implicitly: the caller
//! hands over the variables `nb` should see (`NB_DIR`, `NBRC_PATH`, `PATH`,
//! `HOME`, ...). The CLI passes its whole environment; tests pass a
//! temporary one, so no test can ever touch a real `~/.nb`.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::sanitize::strip_ansi;

/// Name of the executable looked up on `PATH`.
pub const PROGRAM: &str = "nb";

/// Why running `nb` failed.
#[derive(Debug, Error)]
pub enum NbError {
    /// The process could not be started.
    #[error("cannot run {}: {source}", program.display())]
    Spawn {
        /// The executable.
        program: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The process exited with a failure status.
    #[error("nb {args} failed{}: {stderr}", status.map(|s| format!(" with status {s}")).unwrap_or_default())]
    Failed {
        /// The arguments, space separated.
        args: String,
        /// The exit status, when the process exited normally.
        status: Option<i32>,
        /// Sanitised, trimmed standard error.
        stderr: String,
    },
}

/// A located `nb` executable plus the environment it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nb {
    program: PathBuf,
    env: Vec<(String, String)>,
}

impl Nb {
    /// Finds `nb` on the `PATH` entry of `env`; `None` when it is not there.
    pub fn locate(env: &[(String, String)]) -> Option<Self> {
        let path_var = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str())?;
        let program = find_in_path(path_var, PROGRAM)?;
        Some(Self::at(program, env.to_vec()))
    }

    /// Uses `program` as `nb`, with `env` as its environment.
    pub fn at(program: impl Into<PathBuf>, env: Vec<(String, String)>) -> Self {
        Self {
            program: program.into(),
            env,
        }
    }

    /// The executable.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The environment the process gets (and nothing else).
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// Runs `nb <args>` and returns its sanitised standard output (ANSI
    /// escapes and carriage returns removed). Fails when the process cannot
    /// start or exits unsuccessfully.
    ///
    /// Reason: spawns a process; the arguments and environment it passes are
    /// checked by the integration tests with a fake `nb`, not by a unit test.
    #[mutants::skip]
    pub fn run(&self, args: &[&str]) -> Result<String, NbError> {
        let output = Command::new(&self.program)
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .map_err(|source| NbError::Spawn {
                program: self.program.clone(),
                source,
            })?;
        if output.status.success() {
            Ok(strip_ansi(&String::from_utf8_lossy(&output.stdout)))
        } else {
            Err(NbError::Failed {
                args: args.join(" "),
                status: output.status.code(),
                stderr: strip_ansi(&String::from_utf8_lossy(&output.stderr))
                    .trim()
                    .to_owned(),
            })
        }
    }
}

/// The first executable file called `name` in the `:`-separated `path_var`.
/// Empty entries are skipped (the shell would treat them as the current
/// directory, which we do not want for a tool we then run).
pub fn find_in_path(path_var: &str, name: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| is_executable(candidate))
}

/// A regular file with (on Unix) at least one execute bit set.
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(path: &str) -> Vec<(String, String)> {
        vec![("PATH".to_owned(), path.to_owned())]
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn locate_needs_a_path_variable() {
        assert_eq!(Nb::locate(&[]), None);
        assert_eq!(Nb::locate(&[("HOME".to_owned(), "/h".to_owned())]), None);
    }

    #[test]
    #[cfg(unix)]
    fn locate_finds_the_first_executable_nb() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("nb"), "not executable").unwrap();
        std::fs::write(second.join("nb"), "#!/bin/sh\n").unwrap();
        make_executable(&second.join("nb"));
        let path_var = format!(
            ":{}:{}:{}",
            first.display(),
            second.display(),
            dir.path().display()
        );
        let nb = Nb::locate(&env(&path_var)).expect("found");
        assert_eq!(nb.program(), second.join("nb"));
        assert_eq!(nb.env(), env(&path_var));
        assert_eq!(find_in_path(&path_var, "missing"), None);
        assert_eq!(find_in_path("", "nb"), None);
        // A directory called `nb` is not an executable.
        std::fs::create_dir_all(dir.path().join("nb")).unwrap();
        assert_eq!(find_in_path(&dir.path().display().to_string(), "nb"), None);
    }

    #[test]
    fn at_keeps_program_and_env() {
        let nb = Nb::at("/usr/bin/nb", env("/usr/bin"));
        assert_eq!(nb.program(), Path::new("/usr/bin/nb"));
        assert_eq!(nb.env(), env("/usr/bin"));
    }

    #[test]
    fn error_messages() {
        let e = NbError::Failed {
            args: "index verify".into(),
            status: Some(1),
            stderr: "Index corrupted".into(),
        };
        assert_eq!(
            e.to_string(),
            "nb index verify failed with status 1: Index corrupted"
        );
        let e = NbError::Failed {
            args: "x".into(),
            status: None,
            stderr: String::new(),
        };
        assert_eq!(e.to_string(), "nb x failed: ");
        let e = NbError::Spawn {
            program: "/no/nb".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        };
        assert_eq!(e.to_string(), "cannot run /no/nb: gone");
    }
}
