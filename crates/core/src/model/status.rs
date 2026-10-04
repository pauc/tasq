//! Workflow statuses.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// A workflow status such as `in-progress` or `ready`.
///
/// A status is a lowercase kebab-case word (`[a-z0-9]+(-[a-z0-9]+)*`), the
/// shape that is also a valid nb tag. Which statuses exist is decided by the
/// [`Workflow`], not by this type: [`Status::new`] only checks the spelling,
/// [`Workflow::parse_status`] checks membership.
///
/// The five statuses of the original script are available as constants
/// ([`Status::IN_PROGRESS`] and friends).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Status(Cow<'static, str>);

impl Status {
    /// Actively being worked on.
    pub const IN_PROGRESS: Self = Self(Cow::Borrowed("in-progress"));
    /// Ready to be picked up.
    pub const READY: Self = Self(Cow::Borrowed("ready"));
    /// Waiting on someone or something else.
    pub const WAITING: Self = Self(Cow::Borrowed("waiting"));
    /// Cannot proceed.
    pub const BLOCKED: Self = Self(Cow::Borrowed("blocked"));
    /// Parked for later.
    pub const LATER: Self = Self(Cow::Borrowed("later"));

    /// The five statuses of the original script, in display order.
    pub const DEFAULTS: [Self; 5] = [
        Self::IN_PROGRESS,
        Self::READY,
        Self::WAITING,
        Self::BLOCKED,
        Self::LATER,
    ];

    /// Validates the spelling of `name` (lowercase kebab-case, no `#`).
    ///
    /// This does not check that the status belongs to any [`Workflow`].
    pub fn new(name: impl Into<String>) -> Result<Self, ModelError> {
        let name = name.into();
        if Self::is_well_formed(&name) {
            Ok(Self(Cow::Owned(name)))
        } else {
            Err(ModelError::InvalidStatus(name))
        }
    }

    fn is_well_formed(name: &str) -> bool {
        !name.is_empty()
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--")
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    /// The status name, without `#`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The tag form the file uses: `#ready`.
    pub fn to_hash(&self) -> String {
        format!("#{}", self.0)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Status {
    type Err = ModelError;

    /// Same as [`Status::new`]: spelling only, no workflow membership.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Status {
    type Error = ModelError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<Status> for String {
    fn from(s: Status) -> Self {
        s.0.into_owned()
    }
}

/// The ordered list of statuses a store's tasks may be in.
///
/// Order matters: it is the order groups are shown in and the order `next`
/// searches. The default workflow is the script's
/// `in-progress, ready, waiting, blocked, later`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    /// Statuses in display order.
    pub statuses: Vec<Status>,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            statuses: Status::DEFAULTS.to_vec(),
        }
    }
}

impl Workflow {
    /// A workflow from an explicit, ordered status list.
    pub fn new(statuses: Vec<Status>) -> Self {
        Self { statuses }
    }

    /// Looks `name` up in the workflow, with or without a leading `#`.
    ///
    /// Returns `None` for text that is not one of the configured statuses,
    /// which is how the format layer tells a status tag from a topic tag.
    /// The comparison is exact: no trimming, no case folding.
    pub fn parse_status(&self, name: &str) -> Option<Status> {
        let name = name.strip_prefix('#').unwrap_or(name);
        self.statuses.iter().find(|s| s.as_str() == name).cloned()
    }

    /// Whether `status` is one of the configured statuses.
    pub fn contains(&self, status: &Status) -> bool {
        self.statuses.contains(status)
    }

    /// Position of `status` in the workflow, for ordering groups.
    pub fn position(&self, status: &Status) -> Option<usize> {
        self.statuses.iter().position(|s| s == status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workflow_is_the_scripts_five_in_order() {
        let wf = Workflow::default();
        let names: Vec<&str> = wf.statuses.iter().map(Status::as_str).collect();
        assert_eq!(
            names,
            ["in-progress", "ready", "waiting", "blocked", "later"]
        );
        assert_eq!(
            Workflow::default(),
            Workflow::new(Status::DEFAULTS.to_vec())
        );
    }

    #[test]
    fn constants_spell_correctly() {
        assert_eq!(Status::IN_PROGRESS.as_str(), "in-progress");
        assert_eq!(Status::READY.as_str(), "ready");
        assert_eq!(Status::WAITING.as_str(), "waiting");
        assert_eq!(Status::BLOCKED.as_str(), "blocked");
        assert_eq!(Status::LATER.as_str(), "later");
        for s in Status::DEFAULTS {
            assert_eq!(Status::new(s.as_str()), Ok(s.clone()), "{s}");
        }
    }

    #[test]
    fn new_accepts_lowercase_kebab_and_digits() {
        for ok in ["ready", "in-progress", "q3", "a-b-c", "x"] {
            let s = Status::new(ok).unwrap();
            assert_eq!(s.as_str(), ok);
            assert_eq!(s.to_string(), ok);
            assert_eq!(s.to_hash(), format!("#{ok}"));
            assert_eq!(ok.parse::<Status>(), Ok(s.clone()));
            assert_eq!(String::from(s), ok);
        }
    }

    #[test]
    fn new_rejects_bad_spelling_with_the_text() {
        for bad in [
            "",
            "Ready",
            "#ready",
            "in progress",
            "-ready",
            "ready-",
            "in--progress",
            "ré",
            "a_b",
            "a.b",
        ] {
            assert_eq!(
                Status::new(bad),
                Err(ModelError::InvalidStatus(bad.to_owned())),
                "{bad:?}"
            );
            assert_eq!(
                bad.parse::<Status>(),
                Err(ModelError::InvalidStatus(bad.to_owned())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn owned_and_borrowed_statuses_compare_equal() {
        assert_eq!(Status::new("ready").unwrap(), Status::READY);
        assert_eq!(Status::new("in-progress").unwrap(), Status::IN_PROGRESS);
    }

    #[test]
    fn parse_status_finds_members_with_or_without_hash() {
        let wf = Workflow::default();
        assert_eq!(wf.parse_status("ready"), Some(Status::READY));
        assert_eq!(wf.parse_status("#ready"), Some(Status::READY));
        assert_eq!(wf.parse_status("in-progress"), Some(Status::IN_PROGRESS));
        assert_eq!(wf.parse_status("#later"), Some(Status::LATER));
    }

    #[test]
    fn parse_status_rejects_non_members_and_sloppy_text() {
        let wf = Workflow::default();
        for bad in [
            "done", "A", "#A", "gitlab", "Ready", " ready", "ready ", "", "#", "##ready",
        ] {
            assert_eq!(wf.parse_status(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn custom_workflow_only_knows_its_own_statuses() {
        let wf = Workflow::new(vec![
            Status::new("todo").unwrap(),
            Status::new("doing").unwrap(),
        ]);
        assert_eq!(
            wf.parse_status("doing"),
            Some(Status::new("doing").unwrap())
        );
        assert_eq!(wf.parse_status("ready"), None);
        assert!(wf.contains(&Status::new("todo").unwrap()));
        assert!(!wf.contains(&Status::READY));
        assert_eq!(wf.position(&Status::new("doing").unwrap()), Some(1));
        assert_eq!(wf.position(&Status::READY), None);
    }

    #[test]
    fn default_workflow_positions() {
        let wf = Workflow::default();
        assert_eq!(wf.position(&Status::IN_PROGRESS), Some(0));
        assert_eq!(wf.position(&Status::LATER), Some(4));
        assert!(wf.contains(&Status::BLOCKED));
    }

    #[test]
    fn serde_is_a_plain_validated_string() {
        assert_eq!(serde_json::to_string(&Status::READY).unwrap(), "\"ready\"");
        assert_eq!(
            serde_json::from_str::<Status>("\"in-progress\"").unwrap(),
            Status::IN_PROGRESS
        );
        assert!(serde_json::from_str::<Status>("\"#ready\"").is_err());
        assert!(Status::try_from("Ready".to_owned()).is_err());
        let wf: Workflow = serde_json::from_str(r#"{"statuses":["todo","done"]}"#).unwrap();
        assert_eq!(wf.statuses.len(), 2);
    }

    #[test]
    fn error_message() {
        assert_eq!(
            ModelError::InvalidStatus("X".into()).to_string(),
            "invalid status \"X\": expected lowercase words joined by '-'"
        );
    }
}
