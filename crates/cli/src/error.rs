//! Error type of the CLI and its mapping to exit codes.

use std::fmt;

use tasq_core::clock::TimeError;
use tasq_core::config::ConfigError;
use tasq_core::model::ModelError;
use tasq_core::store::StoreError;

/// What a command can fail with.
#[derive(Debug)]
pub enum CliError {
    /// Something the user can fix: printed as `tasq: <message>`, exit `1`.
    User(String),
    /// The command already printed its findings and only needs a non-zero
    /// exit code (`tasq doctor` with a failed check). Exit `code`.
    Silent(u8),
    /// A failure inside `tasq` itself (I/O on stdout, a pager that cannot
    /// be waited for): `tasq: internal error: <message>`, exit `2`.
    Internal(anyhow::Error),
}

/// `Result` with [`CliError`].
pub type Result<T> = std::result::Result<T, CliError>;

impl CliError {
    /// A user error with `message`.
    pub fn user(message: impl Into<String>) -> Self {
        Self::User(message.into())
    }

    /// The process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::User(_) => 1,
            Self::Silent(code) => *code,
            Self::Internal(_) => 2,
        }
    }

    /// Prints the error to stderr the way the user should see it.
    pub fn report(&self) {
        match self {
            Self::User(message) => eprintln!("tasq: {message}"),
            Self::Silent(_) => {}
            Self::Internal(e) => eprintln!("tasq: internal error: {e:#}"),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(message) => f.write_str(message),
            Self::Silent(code) => write!(f, "exit {code}"),
            Self::Internal(e) => write!(f, "internal error: {e:#}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<StoreError> for CliError {
    fn from(e: StoreError) -> Self {
        Self::User(e.to_string())
    }
}

impl From<ConfigError> for CliError {
    fn from(e: ConfigError) -> Self {
        Self::User(e.to_string())
    }
}

impl From<ModelError> for CliError {
    fn from(e: ModelError) -> Self {
        Self::User(e.to_string())
    }
}

impl From<TimeError> for CliError {
    fn from(e: TimeError) -> Self {
        Self::User(e.to_string())
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl From<anyhow::Error> for CliError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(CliError::user("x").exit_code(), 1);
        assert_eq!(CliError::Silent(1).exit_code(), 1);
        assert_eq!(CliError::Silent(3).exit_code(), 3);
        assert_eq!(CliError::Internal(anyhow::anyhow!("boom")).exit_code(), 2);
    }

    #[test]
    fn display() {
        assert_eq!(
            CliError::user("no task with id 9").to_string(),
            "no task with id 9"
        );
        assert_eq!(CliError::Silent(1).to_string(), "exit 1");
        assert_eq!(
            CliError::Internal(anyhow::anyhow!("boom")).to_string(),
            "internal error: boom"
        );
    }

    #[test]
    fn domain_errors_are_user_errors() {
        let e: CliError = StoreError::NotFound(tasq_core::model::TaskId::from(9)).into();
        assert!(matches!(e, CliError::User(ref m) if m == "no task with id 9"));
        let e: CliError = ModelError::EmptyId.into();
        assert!(matches!(e, CliError::User(_)));
        let e: CliError = std::io::Error::other("disk").into();
        assert!(matches!(e, CliError::Internal(_)));
    }
}
