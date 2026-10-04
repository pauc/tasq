//! GitLab REST API v4 as a [`Forge`].

use std::cell::RefCell;

use serde_json::Value;
use tasq_core::config::ForgeKind;

use crate::forge::{
    Forge, MergeRequest, MrState, User, WorkItem, bool_field, default_api_base, missing,
    nested_strings, project_and_number, str_field, strings, u64_field,
};
use crate::http::{Client, HttpError, Transport};
use crate::url::ForgeRef;

/// A GitLab instance.
pub struct GitLab {
    client: Client,
    host: String,
    base: String,
    me: RefCell<Option<User>>,
}

impl std::fmt::Debug for GitLab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitLab")
            .field("host", &self.host)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl GitLab {
    /// The request headers for `token`.
    pub fn headers(token: &str) -> Vec<(String, String)> {
        vec![("PRIVATE-TOKEN".to_owned(), token.to_owned())]
    }

    /// A client for `host`, at `base` (else the default `https://<host>/api/v4`).
    pub fn new(transport: Box<dyn Transport>, token: &str, host: &str, base: Option<&str>) -> Self {
        Self {
            client: Client::new(transport, Self::headers(token)),
            host: host.to_owned(),
            base: base.map_or_else(
                || default_api_base(ForgeKind::Gitlab, host),
                |b| b.trim_end_matches('/').to_owned(),
            ),
            me: RefCell::new(None),
        }
    }

    /// Replaces the client (tests inject a recorded sleep).
    #[must_use]
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    fn me(&self) -> Result<User, HttpError> {
        if let Some(me) = self.me.borrow().as_ref() {
            return Ok(me.clone());
        }
        let me = self.current_user()?;
        *self.me.borrow_mut() = Some(me.clone());
        Ok(me)
    }

    fn project_url(&self, reference: &ForgeRef) -> String {
        format!(
            "{}/projects/{}",
            self.base,
            encode_project(&reference.project)
        )
    }
}

/// A project path as a URL path segment (`group/project` → `group%2Fproject`).
pub fn encode_project(project: &str) -> String {
    project.replace('/', "%2F")
}

/// A merge request from GitLab's JSON (list entry or single object).
pub fn parse_merge_request(value: &Value, api_url: &str) -> Result<MergeRequest, HttpError> {
    let url = str_field(value, "web_url", api_url)?;
    let (project, number) = project_and_number(url, api_url)?;
    let state = match str_field(value, "state", api_url)? {
        "opened" => MrState::Open,
        "merged" => MrState::Merged,
        "closed" | "locked" => MrState::Closed,
        _ => return Err(missing(api_url, "state", value)),
    };
    Ok(MergeRequest {
        project,
        number,
        title: str_field(value, "title", api_url)?.to_owned(),
        url: url.to_owned(),
        state,
        draft: bool_field(value, "draft") || bool_field(value, "work_in_progress"),
        approved_by_me: false,
    })
}

/// An issue from GitLab's JSON.
pub fn parse_issue(value: &Value, api_url: &str) -> Result<WorkItem, HttpError> {
    let url = str_field(value, "web_url", api_url)?;
    let (project, number) = project_and_number(url, api_url)?;
    Ok(WorkItem {
        project,
        number,
        title: str_field(value, "title", api_url)?.to_owned(),
        url: url.to_owned(),
        closed: str_field(value, "state", api_url)? == "closed",
        assignees: nested_strings(value, "assignees", "username"),
        labels: strings(value, "labels"),
    })
}

impl Forge for GitLab {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Gitlab
    }

    fn host(&self) -> &str {
        &self.host
    }

    fn current_user(&self) -> Result<User, HttpError> {
        let url = format!("{}/user", self.base);
        let value = self.client.get_json(&url)?;
        Ok(User {
            id: u64_field(&value, "id", &url)?,
            username: str_field(&value, "username", &url)?.to_owned(),
        })
    }

    fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError> {
        let me = self.me()?;
        let url = format!(
            "{}/merge_requests?scope=all&state=opened&reviewer_id={}&per_page=100",
            self.base, me.id
        );
        self.client
            .get_all(&url)?
            .iter()
            .map(|v| parse_merge_request(v, &url))
            .collect()
    }

    fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError> {
        let url = format!(
            "{}/issues?scope=assigned_to_me&state=opened&per_page=100",
            self.base
        );
        self.client
            .get_all(&url)?
            .iter()
            .map(|v| parse_issue(v, &url))
            .collect()
    }

    fn merge_request(&self, reference: &ForgeRef) -> Result<MergeRequest, HttpError> {
        let url = format!(
            "{}/merge_requests/{}",
            self.project_url(reference),
            reference.number
        );
        let mut mr = parse_merge_request(&self.client.get_json(&url)?, &url)?;
        let me = self.me()?;
        let approvals_url = format!("{url}/approvals");
        let approvals = self.client.get_json(&approvals_url)?;
        mr.approved_by_me = approvals
            .get("approved_by")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|e| {
                    e.get("user")
                        .and_then(|u| u.get("id"))
                        .and_then(Value::as_u64)
                        == Some(me.id)
                })
            });
        Ok(mr)
    }

    fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError> {
        let url = format!(
            "{}/issues/{}",
            self.project_url(reference),
            reference.number
        );
        parse_issue(&self.client.get_json(&url)?, &url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, ScriptedTransport};
    use crate::url::parse_ref;

    const USER: &str = "{\"id\":42,\"username\":\"pau\"}";
    const MR: &str = "{\"iid\":7,\"title\":\"Add parser\",\"web_url\":\"https://gl.example.com/group/project/-/merge_requests/7\",\"state\":\"opened\",\"draft\":false}";

    fn gitlab(responses: Vec<HttpResponse>) -> GitLab {
        GitLab::new(
            Box::new(ScriptedTransport::new(responses)),
            "tok",
            "gl.example.com",
            None,
        )
    }

    #[test]
    fn basics() {
        let gl = gitlab(vec![]);
        assert_eq!(gl.kind(), ForgeKind::Gitlab);
        assert_eq!(gl.host(), "gl.example.com");
        assert_eq!(gl.base, "https://gl.example.com/api/v4");
        assert_eq!(
            GitLab::headers("t"),
            vec![("PRIVATE-TOKEN".to_owned(), "t".to_owned())]
        );
        assert_eq!(encode_project("group/sub/project"), "group%2Fsub%2Fproject");
        let custom = GitLab::new(
            Box::new(ScriptedTransport::default()),
            "t",
            "h",
            Some("http://127.0.0.1:1/api/v4/"),
        );
        assert_eq!(custom.base, "http://127.0.0.1:1/api/v4");
        assert!(format!("{gl:?}").starts_with("GitLab { host: \"gl.example.com\""));
    }

    #[test]
    fn current_user_and_review_requests() {
        let gl = gitlab(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, format!("[{MR}]")),
        ]);
        let mrs = gl.review_requests().unwrap();
        assert_eq!(
            mrs,
            vec![MergeRequest {
                project: "group/project".into(),
                number: 7,
                title: "Add parser".into(),
                url: "https://gl.example.com/group/project/-/merge_requests/7".into(),
                state: MrState::Open,
                draft: false,
                approved_by_me: false,
            }]
        );
        // The user is cached: a second sweep asks only for the list.
        let gl = gitlab(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "[]"),
            HttpResponse::new(200, "[]"),
        ]);
        gl.review_requests().unwrap();
        gl.review_requests().unwrap();
        assert_eq!(gl.me().unwrap().username, "pau");
    }

    #[test]
    fn merge_request_with_approvals() {
        let gl = gitlab(vec![
            HttpResponse::new(200, MR.replace("\"opened\"", "\"merged\"")),
            HttpResponse::new(200, USER),
            HttpResponse::new(
                200,
                "{\"approved_by\":[{\"user\":{\"id\":1}},{\"user\":{\"id\":42}}]}",
            ),
        ]);
        let r = parse_ref("https://gl.example.com/group/project/-/merge_requests/7").unwrap();
        let mr = gl.merge_request(&r).unwrap();
        assert_eq!(mr.state, MrState::Merged);
        assert!(mr.approved_by_me);
        let gl = gitlab(vec![
            HttpResponse::new(
                200,
                MR.replace("\"draft\":false", "\"work_in_progress\":true"),
            ),
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "{\"approved_by\":[]}"),
        ]);
        let mr = gl.merge_request(&r).unwrap();
        assert!(mr.draft);
        assert!(!mr.approved_by_me);
        assert_eq!(mr.short(), "group/project!7");
        // Approved by someone else only.
        let gl = gitlab(vec![
            HttpResponse::new(200, MR.replace("\"opened\"", "\"closed\"")),
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "{\"approved_by\":[{\"user\":{\"id\":1}}]}"),
        ]);
        let mr = gl.merge_request(&r).unwrap();
        assert_eq!(mr.state, MrState::Closed);
        assert!(!mr.approved_by_me);
        let locked: Value = serde_json::from_str(&MR.replace("opened", "locked")).unwrap();
        assert_eq!(
            parse_merge_request(&locked, "u").unwrap().state,
            MrState::Closed
        );
    }

    #[test]
    fn issues() {
        let issue = "{\"iid\":3,\"title\":\"Bug\",\"web_url\":\"https://gl.example.com/g/p/-/issues/3\",\"state\":\"closed\",\"assignees\":[{\"username\":\"pau\"}],\"labels\":[\"team::core\"]}";
        let gl = gitlab(vec![
            HttpResponse::new(200, format!("[{issue}]")),
            HttpResponse::new(200, issue),
        ]);
        let items = gl.assigned_work_items().unwrap();
        assert_eq!(
            items,
            vec![WorkItem {
                project: "g/p".into(),
                number: 3,
                title: "Bug".into(),
                url: "https://gl.example.com/g/p/-/issues/3".into(),
                closed: true,
                assignees: vec!["pau".into()],
                labels: vec!["team::core".into()],
            }]
        );
        let r = parse_ref("https://gl.example.com/g/p/-/issues/3").unwrap();
        assert_eq!(gl.work_item(&r).unwrap().short(), "g/p#3");
    }

    #[test]
    fn requested_urls_and_decode_errors() {
        let transport = std::rc::Rc::new(ScriptedTransport::new(vec![
            HttpResponse::new(200, USER),
            HttpResponse::new(200, "[]"),
            HttpResponse::new(200, MR),
            HttpResponse::new(200, "{\"approved_by\":[]}"),
            HttpResponse::new(
                200,
                "{\"iid\":2,\"title\":\"I\",\"web_url\":\"https://h/g/p/-/issues/2\",\"state\":\"opened\"}",
            ),
        ]));
        let urls = {
            let gl = GitLab {
                client: Client::new(
                    Box::new(Shared(std::rc::Rc::clone(&transport))),
                    GitLab::headers("t"),
                ),
                host: "h".into(),
                base: "https://h/api/v4".into(),
                me: RefCell::new(None),
            };
            gl.review_requests().unwrap();
            gl.merge_request(&parse_ref("https://h/group/sub/project/-/merge_requests/7").unwrap())
                .unwrap();
            gl.work_item(&parse_ref("https://h/g/p/-/issues/2").unwrap())
                .unwrap();
            transport.urls()
        };
        assert_eq!(
            urls,
            vec![
                "https://h/api/v4/user",
                "https://h/api/v4/merge_requests?scope=all&state=opened&reviewer_id=42&per_page=100",
                "https://h/api/v4/projects/group%2Fsub%2Fproject/merge_requests/7",
                "https://h/api/v4/projects/group%2Fsub%2Fproject/merge_requests/7/approvals",
                "https://h/api/v4/projects/g%2Fp/issues/2",
            ]
        );
        let bad_state = parse_merge_request(
            &serde_json::from_str(&MR.replace("opened", "weird")).unwrap(),
            "u",
        )
        .unwrap_err();
        assert!(
            matches!(bad_state, HttpError::Decode { message, .. } if message.contains("`state`"))
        );
        assert!(parse_issue(&serde_json::json!({"title": "x"}), "u").is_err());
        let gl = gitlab(vec![HttpResponse::new(200, "{\"username\":\"x\"}")]);
        assert!(gl.current_user().is_err());
    }

    /// Lets a test keep the transport to read its recorded URLs.
    struct Shared(std::rc::Rc<ScriptedTransport>);

    impl Transport for Shared {
        fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String> {
            self.0.get(url, headers)
        }
    }
}
