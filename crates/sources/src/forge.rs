//! The [`Forge`] trait: what the review-request and work-item sources need
//! from GitLab or GitHub, plus the JSON helpers both clients share.

use serde_json::Value;
use tasq_core::config::ForgeKind;

use crate::http::{HttpError, excerpt};
use crate::url::ForgeRef;

/// The authenticated user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Numeric id (GitLab `id`, GitHub `id`).
    pub id: u64,
    /// `username` / `login`.
    pub username: String,
}

/// A merge request's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MrState {
    /// Open (possibly a draft).
    Open,
    /// Merged.
    Merged,
    /// Closed without merging.
    Closed,
}

/// A merge request or pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    /// `group/project` or `owner/repo`.
    pub project: String,
    /// iid / number.
    pub number: u64,
    /// Title.
    pub title: String,
    /// Web URL.
    pub url: String,
    /// State.
    pub state: MrState,
    /// Marked as a draft / WIP.
    pub draft: bool,
    /// Whether the authenticated user has approved it (only filled by
    /// [`Forge::merge_request`]; lists leave it `false`).
    pub approved_by_me: bool,
}

impl MergeRequest {
    /// `project!number`.
    pub fn short(&self) -> String {
        format!("{}!{}", self.project, self.number)
    }
}

/// An issue or work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// `group/project` or `owner/repo`.
    pub project: String,
    /// iid / number.
    pub number: u64,
    /// Title.
    pub title: String,
    /// Web URL.
    pub url: String,
    /// Closed.
    pub closed: bool,
    /// Usernames assigned.
    pub assignees: Vec<String>,
    /// Label names.
    pub labels: Vec<String>,
}

impl WorkItem {
    /// `project#number`.
    pub fn short(&self) -> String {
        format!("{}#{}", self.project, self.number)
    }
}

/// A GitLab or GitHub API.
pub trait Forge {
    /// Which API.
    fn kind(&self) -> ForgeKind;
    /// The host the URLs belong to.
    fn host(&self) -> &str;
    /// The authenticated user.
    fn current_user(&self) -> Result<User, HttpError>;
    /// Open merge requests where the user is a requested reviewer.
    fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError>;
    /// Open issues assigned to the user.
    fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError>;
    /// One merge request, with `approved_by_me` filled in.
    fn merge_request(&self, reference: &ForgeRef) -> Result<MergeRequest, HttpError>;
    /// One issue.
    fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError>;
}

/// The default API base for `host`: GitLab `https://<host>/api/v4`; GitHub
/// `https://api.github.com` for `github.com`, else `https://<host>/api/v3`.
pub fn default_api_base(kind: ForgeKind, host: &str) -> String {
    match kind {
        ForgeKind::Gitlab => format!("https://{host}/api/v4"),
        ForgeKind::Github if host == "github.com" => "https://api.github.com".to_owned(),
        ForgeKind::Github => format!("https://{host}/api/v3"),
    }
}

/// `value[key]` as text, else a decode error naming the field.
pub fn str_field<'a>(value: &'a Value, key: &str, url: &str) -> Result<&'a str, HttpError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| missing(url, key, value))
}

/// `value[key]` as an unsigned integer, else a decode error.
pub fn u64_field(value: &Value, key: &str, url: &str) -> Result<u64, HttpError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| missing(url, key, value))
}

/// `value[key]` as a boolean, `false` when absent or not a boolean.
pub fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The strings at `value[key][*][inner]` (GitHub `labels[].name`,
/// GitLab `assignees[].username`).
pub fn nested_strings(value: &Value, key: &str, inner: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.get(inner).and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The strings at `value[key][*]` (GitLab `labels`).
pub fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// A decode error for a missing or mistyped `field`.
pub fn missing(url: &str, field: &str, value: &Value) -> HttpError {
    HttpError::Decode {
        url: url.to_owned(),
        message: format!("missing or invalid field `{field}`"),
        excerpt: excerpt(&value.to_string()),
    }
}

/// The project and number of an item from its web URL, which both APIs
/// return; a URL the parser does not understand is a decode error.
pub fn project_and_number(url_field: &str, api_url: &str) -> Result<(String, u64), HttpError> {
    crate::url::parse_ref(url_field)
        .map(|r| (r.project, r.number))
        .ok_or_else(|| HttpError::Decode {
            url: api_url.to_owned(),
            message: format!("web URL {url_field:?} is not a merge request or issue URL"),
            excerpt: String::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_bases() {
        assert_eq!(
            default_api_base(ForgeKind::Gitlab, "gitlab.example.com"),
            "https://gitlab.example.com/api/v4"
        );
        assert_eq!(
            default_api_base(ForgeKind::Github, "github.com"),
            "https://api.github.com"
        );
        assert_eq!(
            default_api_base(ForgeKind::Github, "ghe.example.com"),
            "https://ghe.example.com/api/v3"
        );
    }

    #[test]
    fn json_helpers() {
        let v: Value = serde_json::from_str(
            "{\"title\":\"T\",\"iid\":3,\"draft\":true,\"labels\":[\"a\",\"b\"],\"assignees\":[{\"username\":\"me\"},{\"nope\":1}]}",
        )
        .unwrap();
        assert_eq!(str_field(&v, "title", "u").unwrap(), "T");
        assert_eq!(u64_field(&v, "iid", "u").unwrap(), 3);
        assert!(bool_field(&v, "draft"));
        assert!(!bool_field(&v, "merged"));
        assert_eq!(strings(&v, "labels"), vec!["a", "b"]);
        assert_eq!(strings(&v, "none"), Vec::<String>::new());
        assert_eq!(nested_strings(&v, "assignees", "username"), vec!["me"]);
        assert_eq!(nested_strings(&v, "labels", "name"), Vec::<String>::new());
        let err = str_field(&v, "iid", "https://api/x").unwrap_err();
        assert_eq!(
            err,
            HttpError::Decode {
                url: "https://api/x".into(),
                message: "missing or invalid field `iid`".into(),
                excerpt: excerpt(&v.to_string())
            }
        );
        assert!(u64_field(&v, "title", "u").is_err());
        assert_eq!(
            project_and_number("https://gl/g/p/-/merge_requests/4", "u").unwrap(),
            ("g/p".to_owned(), 4)
        );
        assert_eq!(
            project_and_number("https://gl/g/p/-/pipelines/4", "u").unwrap_err(),
            HttpError::Decode {
                url: "u".into(),
                message:
                    "web URL \"https://gl/g/p/-/pipelines/4\" is not a merge request or issue URL"
                        .into(),
                excerpt: String::new()
            }
        );
    }

    #[test]
    fn short_references() {
        let mr = MergeRequest {
            project: "g/p".into(),
            number: 1,
            title: String::new(),
            url: String::new(),
            state: MrState::Open,
            draft: false,
            approved_by_me: false,
        };
        assert_eq!(mr.short(), "g/p!1");
        let item = WorkItem {
            project: "o/r".into(),
            number: 2,
            title: String::new(),
            url: String::new(),
            closed: false,
            assignees: Vec::new(),
            labels: Vec::new(),
        };
        assert_eq!(item.short(), "o/r#2");
    }
}
