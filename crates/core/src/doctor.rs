//! Diagnostics: the [`Check`]s a [`Store`](crate::store::Store) reports
//! through [`Store::doctor`](crate::store::Store::doctor), and the CLI
//! prints as `tasq doctor`'s OK/WARN/FAIL lines or as JSON.

use std::fmt;

use serde::{Deserialize, Serialize};

/// How a check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Fine.
    Ok,
    /// Works, but the user should know.
    Warn,
    /// Broken; `tasq doctor` exits 1.
    Fail,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        })
    }
}

/// One diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// Short identifier (`notebook`, `index`, `git`, ...).
    pub name: String,
    /// The verdict.
    pub status: CheckStatus,
    /// One line saying what was found.
    pub detail: String,
    /// What to do about it, for `Warn` and `Fail`.
    pub fix: Option<String>,
}

impl Check {
    /// An `Ok` check.
    pub fn ok(name: &str, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Ok, detail, None)
    }

    /// A `Warn` check with its fix.
    pub fn warn(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Warn, detail, Some(fix.into()))
    }

    /// A `Fail` check with its fix.
    pub fn fail(name: &str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Fail, detail, Some(fix.into()))
    }

    fn new(
        name: &str,
        status: CheckStatus,
        detail: impl Into<String>,
        fix: Option<String>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            status,
            detail: detail.into(),
            fix,
        }
    }
}

/// Whether any check failed (the CLI exits 1 then).
pub fn any_failed(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.status == CheckStatus::Fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_display_and_serde() {
        assert_eq!(CheckStatus::Ok.to_string(), "OK");
        assert_eq!(CheckStatus::Warn.to_string(), "WARN");
        assert_eq!(CheckStatus::Fail.to_string(), "FAIL");
        assert_eq!(
            serde_json::to_string(&CheckStatus::Warn).unwrap(),
            "\"warn\""
        );
        let c = Check::fail("index", "missing", "reconcile");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            "{\"name\":\"index\",\"status\":\"fail\",\"detail\":\"missing\",\"fix\":\"reconcile\"}"
        );
        assert_eq!(serde_json::from_str::<Check>(&json).unwrap(), c);
        assert_eq!(
            serde_json::to_string(&Check::ok("nb", "fine")).unwrap(),
            "{\"name\":\"nb\",\"status\":\"ok\",\"detail\":\"fine\",\"fix\":null}"
        );
    }

    #[test]
    fn constructors() {
        let ok = Check::ok("a", "fine");
        assert_eq!(
            (
                ok.name.as_str(),
                ok.status,
                ok.detail.as_str(),
                ok.fix.as_deref()
            ),
            ("a", CheckStatus::Ok, "fine", None)
        );
        let warn = Check::warn("b", "meh", "do x");
        assert_eq!(
            (warn.status, warn.fix.as_deref()),
            (CheckStatus::Warn, Some("do x"))
        );
        let fail = Check::fail("c", "bad", "do y");
        assert_eq!(
            (fail.status, fail.fix.as_deref()),
            (CheckStatus::Fail, Some("do y"))
        );
        assert!(!any_failed(&[ok.clone(), warn.clone()]));
        assert!(any_failed(&[ok, warn, fail]));
        assert!(!any_failed(&[]));
    }
}
