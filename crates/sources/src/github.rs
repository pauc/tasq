//! GitHub REST API as a [`Forge`] (github.com or GitHub Enterprise).

use std::cell::RefCell;

use serde_json::Value;
use tasq_core::config::ForgeKind;

use crate::forge::{
    Forge, MergeRequest, MrState, User, WorkItem, bool_field, default_api_base, missing,
    nested_strings, project_and_number, str_field, u64_field,
};
use crate::http::{Client, HttpError, Transport};
use crate::url::ForgeRef;

/// A GitHub instance.
pub struct GitHub {
    client: Client,
    host: String,
    base: String,
    me: RefCell<Option<User>>,
}

impl std::fmt::Debug for GitHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHub")
            .field("host", &self.host)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl GitHub {
    /// The request headers for `token`.
    pub fn headers(token: &str) -> Vec<(String, String)> {
        vec![
            ("Authorization".to_owned(), format!("Bearer {token}")),
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
            ("User-Agent".to_owned(), "tasq".to_owned()),
        ]
    }

    /// A client for `host`, at `base` (else `https://api.github.com` for
    /// `github.com`, `https://<host>/api/v3` otherwise).
    pub fn new(transport: Box<dyn Transport>, token: &str, host: &str, base: Option<&str>) -> Self {
        Self {
            client: Client::new(transport, Self::headers(token)),
            host: host.to_owned(),
            base: base.map_or_else(
                || default_api_base(ForgeKind::Github, host),
                |b| b.trim_end_matches('/').to_owned(),
            ),
            me: RefCell::new(None),
        }
    }

    fn me(&self) -> Result<User, HttpError> {
        if let Some(me) = self.me.borrow().as_ref() {
            return Ok(me.clone());
        }
        let me = self.current_user()?;
        *self.me.borrow_mut() = Some(me.clone());
        Ok(me)
    }
}

/// A pull request from GitHub's JSON (search item or `pulls/:n` object).
pub fn parse_pull_request(value: &Value, api_url: &str) -> Result<MergeRequest, HttpError> {
    let url = str_field(value, "html_url", api_url)?;
    let (project, number) = project_and_number(url, api_url)?;
    let state = match str_field(value, "state", api_url)? {
        "open" => MrState::Open,
        "closed" if bool_field(value, "merged") => MrState::Merged,
        "closed" => MrState::Closed,
        _ => return Err(missing(api_url, "state", value)),
    };
    Ok(MergeRequest {
        project,
        number,
        title: str_field(value, "title", api_url)?.to_owned(),
        url: url.to_owned(),
        state,
        draft: bool_field(value, "draft"),
        approved_by_me: false,
    })
}

/// An issue from GitHub's JSON.
pub fn parse_issue(value: &Value, api_url: &str) -> Result<WorkItem, HttpError> {
    let url = str_field(value, "html_url", api_url)?;
    let (project, number) = project_and_number(url, api_url)?;
    Ok(WorkItem {
        project,
        number,
        title: str_field(value, "title", api_url)?.to_owned(),
        url: url.to_owned(),
        closed: str_field(value, "state", api_url)? == "closed",
        assignees: nested_strings(value, "assignees", "login"),
        labels: nested_strings(value, "labels", "name"),
    })
}

impl Forge for GitHub {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Github
    }

    fn host(&self) -> &str {
        &self.host
    }

    fn current_user(&self) -> Result<User, HttpError> {
        let url = format!("{}/user", self.base);
        let value = self.client.get_json(&url)?;
        Ok(User {
            id: u64_field(&value, "id", &url)?,
            username: str_field(&value, "login", &url)?.to_owned(),
        })
    }

    /// The search API: `is:pr is:open review-requested:<login>` (one page
    /// of 100; the search endpoint is not paginated with `Link`).
    fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError> {
        let me = self.me()?;
        let url = format!(
            "{}/search/issues?q=is:pr+is:open+review-requested:{}&per_page=100",
            self.base, me.username
        );
        let value = self.client.get_json(&url)?;
        value
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| missing(&url, "items", &value))?
            .iter()
            .map(|v| parse_pull_request(v, &url))
            .collect()
    }

    /// `/issues?filter=assigned`, which also lists pull requests; those are
    /// skipped.
    fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError> {
        let url = format!(
            "{}/issues?filter=assigned&state=open&per_page=100",
            self.base
        );
        self.client
            .get_all(&url)?
            .iter()
            .filter(|v| v.get("pull_request").is_none())
            .map(|v| parse_issue(v, &url))
            .collect()
    }

    fn merge_request(&self, reference: &ForgeRef) -> Result<MergeRequest, HttpError> {
        let url = format!(
            "{}/repos/{}/pulls/{}",
            self.base, reference.project, reference.number
        );
        let mut pr = parse_pull_request(&self.client.get_json(&url)?, &url)?;
        let me = self.me()?;
        let reviews_url = format!("{url}/reviews");
        pr.approved_by_me = self.client.get_all(&reviews_url)?.iter().any(|review| {
            review.get("state").and_then(Value::as_str) == Some("APPROVED")
                && review
                    .get("user")
                    .and_then(|u| u.get("login"))
                    .and_then(Value::as_str)
                    == Some(me.username.as_str())
        });
        Ok(pr)
    }

    fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError> {
        let url = format!(
            "{}/repos/{}/issues/{}",
            self.base, reference.project, reference.number
        );
        parse_issue(&self.client.get_json(&url)?, &url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, ScriptedTransport};
    use crate::url::parse_ref;

    /// Shares a scripted transport between the client and the test.
    struct Shared(std::rc::Rc<ScriptedTransport>);

    impl Transport for Shared {
        fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String> {
            self.0.get(url, headers)
        }
    }

    const USER: &str = "{\"id\":9,\"login\":\"pau\"}";
    const PR: &str = "{\"number\":5,\"title\":\"Fix\",\"html_url\":\"https://github.com/owner/repo/pull/5\",\"state\":\"open\",\"draft\":true}";

    fn github(responses: Vec<HttpResponse>) -> GitHub {
        GitHub::new(
            Box::new(ScriptedTransport::new(responses)),
            "tok",
            "github.com",
            None,
        )
    }

    #[test]
    fn basics() {
        let gh = github(vec![]);
        assert_eq!(gh.kind(), ForgeKind::Github);
        assert_eq!(gh.host(), "github.com");
        assert_eq!(gh.base, "https://api.github.com");
        let ghe = GitHub::new(
            Box::new(ScriptedTransport::default()),
            "t",
            "ghe.example.com",
            None,
        );
        assert_eq!(ghe.base, "https://ghe.example.com/api/v3");
        let headers = GitHub::headers("t");
        assert_eq!(
            headers[0],
            ("Authorization".to_owned(), "Bearer t".to_owned())
        );
        assert_eq!(headers.len(), 4);
        assert!(format!("{gh:?}").starts_with("GitHub { host: \"github.com\""));
    }

    #[test]
    fn review_requests_through_search() {
        let gh = github(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, format!("{{\"total_count\":1,\"items\":[{PR}]}}")),
        ]);
        let prs = gh.review_requests().unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].short(), "owner/repo!5");
        assert!(prs[0].draft);
        assert_eq!(prs[0].state, MrState::Open);
        let gh = github(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "{}"),
        ]);
        assert!(
            matches!(gh.review_requests().unwrap_err(), HttpError::Decode { message, .. } if message.contains("`items`"))
        );
    }

    #[test]
    fn pull_request_states_and_approval() {
        let merged = PR.replace("\"state\":\"open\"", "\"state\":\"closed\",\"merged\":true");
        let gh = github(vec![
            HttpResponse::new(200, merged),
            HttpResponse::new(200, USER),
            HttpResponse::new(
                200,
                "[{\"state\":\"COMMENTED\",\"user\":{\"login\":\"pau\"}},{\"state\":\"APPROVED\",\"user\":{\"login\":\"pau\"}}]",
            ),
        ]);
        let r = parse_ref("https://github.com/owner/repo/pull/5").unwrap();
        let pr = gh.merge_request(&r).unwrap();
        assert_eq!(pr.state, MrState::Merged);
        assert!(pr.approved_by_me);
        let closed = PR.replace("\"state\":\"open\"", "\"state\":\"closed\"");
        let gh = github(vec![
            HttpResponse::new(200, closed),
            HttpResponse::new(200, USER),
            HttpResponse::new(
                200,
                "[{\"state\":\"APPROVED\",\"user\":{\"login\":\"someone\"}}]",
            ),
        ]);
        let pr = gh.merge_request(&r).unwrap();
        assert_eq!(pr.state, MrState::Closed);
        assert!(!pr.approved_by_me);
        // My own non-approving review does not count.
        let gh = github(vec![
            HttpResponse::new(200, PR),
            HttpResponse::new(200, USER),
            HttpResponse::new(
                200,
                "[{\"state\":\"COMMENTED\",\"user\":{\"login\":\"pau\"}}]",
            ),
        ]);
        assert!(!gh.merge_request(&r).unwrap().approved_by_me);
        let weird: Value = serde_json::from_str(&PR.replace("\"open\"", "\"odd\"")).unwrap();
        assert!(parse_pull_request(&weird, "u").is_err());
    }

    #[test]
    fn issues_skip_pull_requests() {
        let issue = "{\"number\":2,\"title\":\"Bug\",\"html_url\":\"https://github.com/owner/repo/issues/2\",\"state\":\"open\",\"assignees\":[{\"login\":\"pau\"}],\"labels\":[{\"name\":\"bug\"}]}";
        let pr_as_issue = "{\"number\":3,\"title\":\"PR\",\"html_url\":\"https://github.com/owner/repo/pull/3\",\"state\":\"open\",\"pull_request\":{}}";
        let gh = github(vec![
            HttpResponse::new(200, format!("[{issue},{pr_as_issue}]")),
            HttpResponse::new(200, issue.replace("\"open\"", "\"closed\"")),
        ]);
        let items = gh.assigned_work_items().unwrap();
        assert_eq!(
            items,
            vec![WorkItem {
                project: "owner/repo".into(),
                number: 2,
                title: "Bug".into(),
                url: "https://github.com/owner/repo/issues/2".into(),
                closed: false,
                assignees: vec!["pau".into()],
                labels: vec!["bug".into()],
            }]
        );
        let r = parse_ref("https://github.com/owner/repo/issues/2").unwrap();
        assert!(gh.work_item(&r).unwrap().closed);
    }

    #[test]
    fn requested_urls() {
        let transport = std::rc::Rc::new(ScriptedTransport::new(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "{\"items\":[]}"),
            HttpResponse::new(200, PR),
            HttpResponse::new(200, "[]"),
        ]));
        let gh = GitHub {
            client: Client::new(
                Box::new(Shared(std::rc::Rc::clone(&transport))),
                GitHub::headers("t"),
            ),
            host: "github.com".into(),
            base: "https://api.github.com".into(),
            me: RefCell::new(None),
        };
        gh.review_requests().unwrap();
        gh.merge_request(&parse_ref("https://github.com/owner/repo/pull/5").unwrap())
            .unwrap();
        assert_eq!(
            transport.urls(),
            vec![
                "https://api.github.com/user",
                "https://api.github.com/search/issues?q=is:pr+is:open+review-requested:pau&per_page=100",
                "https://api.github.com/repos/owner/repo/pulls/5",
                "https://api.github.com/repos/owner/repo/pulls/5/reviews",
            ]
        );
        assert_eq!(transport.headers(0)[0].1, "Bearer t");
        let gh = github(vec![HttpResponse::new(200, "{\"id\":1}")]);
        assert!(gh.current_user().is_err());
    }
}
