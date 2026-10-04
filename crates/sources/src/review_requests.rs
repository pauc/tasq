//! Merge requests or pull requests where the user is a requested reviewer
//! (plan T-504), one source per forge.

use tasq_core::config::ForgeKind;
use tasq_core::model::Origin;
use tasq_core::source::{ItemState, Source, SourceError, SourceItem, SourceItemState, SyncContext};

use crate::forge::{Forge, MrState};
use crate::http::HttpError;
use crate::url::{ForgeRef, RefKind, parse_ref};

/// Default title templates per forge.
pub fn default_title(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::Gitlab => "Review MR !{iid}: {title}",
        ForgeKind::Github => "Review PR #{iid}: {title}",
    }
}

/// Fills `{title}`, `{iid}` and `{project}` in a title template.
pub fn render_title(template: &str, title: &str, number: u64, project: &str) -> String {
    template
        .replace("{title}", title)
        .replace("{iid}", &number.to_string())
        .replace("{project}", project)
}

/// The forge reference a tracked origin points at: its URL, else the
/// short reference (`project!123` / `project#123`) on `host`.
pub fn origin_ref(origin: &Origin, host: &str, kind: RefKind) -> Option<ForgeRef> {
    if let Some(url) = &origin.url
        && let Some(reference) = parse_ref(url)
    {
        return Some(reference);
    }
    let marker = match kind {
        RefKind::MergeRequest => '!',
        RefKind::Issue => '#',
    };
    let (project, number) = origin.external_id.rsplit_once(marker)?;
    let number = number.parse().ok()?;
    (!project.is_empty()).then(|| ForgeRef {
        host: host.to_owned(),
        project: project.to_owned(),
        kind,
        number,
        gitlab_shaped: true,
    })
}

/// What a lookup of one tracked item ended as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    /// The item, as the forge has it.
    Found(T),
    /// HTTP 404: the item is gone.
    Gone,
}

/// Maps a forge error: 404 is [`Lookup::Gone`], 401/403 an auth error,
/// anything else unavailable.
pub fn lookup<T>(result: Result<T, HttpError>) -> Result<Lookup<T>, SourceError> {
    match result {
        Ok(value) => Ok(Lookup::Found(value)),
        Err(HttpError::Status { status: 404, .. }) => Ok(Lookup::Gone),
        Err(e @ HttpError::Auth { .. }) => Err(SourceError::Auth(e.to_string())),
        Err(e) => Err(SourceError::Unavailable(e.to_string())),
    }
}

/// A list call's error: like [`lookup`], but a 404 is unexpected too.
pub fn list_error(error: HttpError) -> SourceError {
    match error {
        HttpError::Auth { .. } => SourceError::Auth(error.to_string()),
        other => SourceError::Unavailable(other.to_string()),
    }
}

/// Review requests on one forge.
pub struct ReviewRequests {
    name: String,
    forge: Box<dyn Forge>,
    title: String,
}

impl std::fmt::Debug for ReviewRequests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReviewRequests")
            .field("name", &self.name)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl ReviewRequests {
    /// A source called `name` over `forge`, with `title` as the template
    /// (`None`: the forge's default).
    pub fn new(name: &str, forge: Box<dyn Forge>, title: Option<&str>) -> Self {
        let title = title
            .unwrap_or_else(|| default_title(forge.kind()))
            .to_owned();
        Self {
            name: name.to_owned(),
            forge,
            title,
        }
    }
}

impl Source for ReviewRequests {
    fn name(&self) -> &str {
        &self.name
    }

    /// Every open merge request awaiting the user's review, as an open item
    /// whose external id is `project!number`.
    fn fetch(&self, _ctx: &SyncContext) -> Result<Vec<SourceItem>, SourceError> {
        let mrs = self.forge.review_requests().map_err(list_error)?;
        Ok(mrs
            .iter()
            .map(|mr| {
                SourceItem::new(
                    mr.short(),
                    render_title(&self.title, &mr.title, mr.number, &mr.project),
                )
                .with_url(mr.url.clone())
            })
            .collect())
    }

    /// Done when merged, closed, approved by the user, or gone; origins
    /// that are not merge request references are skipped.
    fn check(&self, origins: &[Origin]) -> Result<Vec<SourceItemState>, SourceError> {
        let mut out = Vec::new();
        for origin in origins {
            let Some(reference) = origin_ref(origin, self.forge.host(), RefKind::MergeRequest)
            else {
                continue;
            };
            if reference.kind != RefKind::MergeRequest {
                continue;
            }
            let (state, note) = match lookup(self.forge.merge_request(&reference))? {
                Lookup::Gone => (ItemState::Done, "no longer exists"),
                Lookup::Found(mr) => match mr.state {
                    MrState::Merged => (ItemState::Done, "merged"),
                    MrState::Closed => (ItemState::Done, "closed"),
                    MrState::Open if mr.approved_by_me => (ItemState::Done, "approved by you"),
                    MrState::Open => (ItemState::Open, "open"),
                },
            };
            out.push(SourceItemState {
                origin: origin.clone(),
                state,
                note: Some(note.to_owned()),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{MergeRequest, User, WorkItem};
    use std::cell::RefCell;

    /// A forge answering from fixed data, recording the references asked.
    #[derive(Default)]
    pub(crate) struct FakeForge {
        pub kind: Option<ForgeKind>,
        pub review: Vec<MergeRequest>,
        pub items: Vec<WorkItem>,
        pub by_ref: Vec<(ForgeRef, Result<MergeRequest, HttpError>)>,
        pub items_by_ref: Vec<(ForgeRef, Result<WorkItem, HttpError>)>,
        pub asked: RefCell<Vec<String>>,
        pub list_error: Option<HttpError>,
    }

    impl Forge for FakeForge {
        fn kind(&self) -> ForgeKind {
            self.kind.unwrap_or(ForgeKind::Gitlab)
        }
        fn host(&self) -> &'static str {
            "gl"
        }
        fn current_user(&self) -> Result<User, HttpError> {
            Ok(User {
                id: 1,
                username: "me".into(),
            })
        }
        fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError> {
            match &self.list_error {
                Some(e) => Err(e.clone()),
                None => Ok(self.review.clone()),
            }
        }
        fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError> {
            match &self.list_error {
                Some(e) => Err(e.clone()),
                None => Ok(self.items.clone()),
            }
        }
        fn merge_request(&self, reference: &ForgeRef) -> Result<MergeRequest, HttpError> {
            self.asked.borrow_mut().push(reference.short());
            match self
                .by_ref
                .iter()
                .find(|(r, _)| r.project == reference.project && r.number == reference.number)
            {
                Some((_, r)) => r.clone(),
                None => Err(HttpError::Status {
                    url: "u".into(),
                    status: 404,
                    excerpt: String::new(),
                }),
            }
        }
        fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError> {
            self.asked.borrow_mut().push(reference.short());
            match self
                .items_by_ref
                .iter()
                .find(|(r, _)| r.project == reference.project && r.number == reference.number)
            {
                Some((_, r)) => r.clone(),
                None => Err(HttpError::Status {
                    url: "u".into(),
                    status: 404,
                    excerpt: String::new(),
                }),
            }
        }
    }

    pub(crate) fn mr(project: &str, number: u64, state: MrState, approved: bool) -> MergeRequest {
        MergeRequest {
            project: project.into(),
            number,
            title: format!("Title {number}"),
            url: format!("https://gl/{project}/-/merge_requests/{number}"),
            state,
            draft: false,
            approved_by_me: approved,
        }
    }

    fn origin(external_id: &str, url: Option<&str>) -> Origin {
        Origin {
            source: "rr".into(),
            external_id: external_id.into(),
            url: url.map(str::to_owned),
        }
    }

    #[test]
    fn titles_and_refs() {
        assert_eq!(
            default_title(ForgeKind::Gitlab),
            "Review MR !{iid}: {title}"
        );
        assert_eq!(
            default_title(ForgeKind::Github),
            "Review PR #{iid}: {title}"
        );
        assert_eq!(
            render_title("[{project}] {iid} {title}", "T", 4, "g/p"),
            "[g/p] 4 T"
        );
        let from_url = origin_ref(
            &origin("x", Some("https://gl/g/p/-/merge_requests/3")),
            "gl",
            RefKind::MergeRequest,
        )
        .unwrap();
        assert_eq!((from_url.project.as_str(), from_url.number), ("g/p", 3));
        let from_id = origin_ref(&origin("g/sub/p!12", None), "gl", RefKind::MergeRequest).unwrap();
        assert_eq!(
            from_id,
            ForgeRef {
                host: "gl".into(),
                project: "g/sub/p".into(),
                kind: RefKind::MergeRequest,
                number: 12,
                gitlab_shaped: true
            }
        );
        let issue = origin_ref(&origin("g/p#5", None), "gl", RefKind::Issue).unwrap();
        assert_eq!((issue.kind, issue.number), (RefKind::Issue, 5));
        assert_eq!(
            origin_ref(&origin("g/p#5", None), "gl", RefKind::MergeRequest),
            None
        );
        assert_eq!(
            origin_ref(&origin("!5", None), "gl", RefKind::MergeRequest),
            None
        );
        assert_eq!(
            origin_ref(&origin("g/p!x", None), "gl", RefKind::MergeRequest),
            None
        );
        assert_eq!(
            origin_ref(
                &origin("nonsense", Some("https://x")),
                "gl",
                RefKind::MergeRequest
            ),
            None
        );
    }

    #[test]
    fn lookups_classify_errors() {
        assert_eq!(lookup::<u8>(Ok(1)).unwrap(), Lookup::Found(1));
        let gone = HttpError::Status {
            url: "u".into(),
            status: 404,
            excerpt: String::new(),
        };
        assert_eq!(lookup::<u8>(Err(gone.clone())).unwrap(), Lookup::Gone);
        let auth = HttpError::Auth {
            url: "u".into(),
            status: 401,
        };
        assert_eq!(
            lookup::<u8>(Err(auth.clone())).unwrap_err(),
            SourceError::Auth("u: HTTP 401, check the token".into())
        );
        let other = HttpError::Status {
            url: "u".into(),
            status: 500,
            excerpt: "e".into(),
        };
        assert_eq!(
            lookup::<u8>(Err(other.clone())).unwrap_err(),
            SourceError::Unavailable("u: HTTP 500 (starts with \"e\")".into())
        );
        assert!(matches!(list_error(auth), SourceError::Auth(_)));
        assert!(matches!(list_error(gone), SourceError::Unavailable(_)));
        assert!(matches!(list_error(other), SourceError::Unavailable(_)));
    }

    #[test]
    fn fetch_builds_items() {
        let forge = FakeForge {
            review: vec![mr("g/p", 7, MrState::Open, false)],
            ..FakeForge::default()
        };
        let source = ReviewRequests::new("rr", Box::new(forge), None);
        assert_eq!(source.name(), "rr");
        let items = source.fetch(&SyncContext::default()).unwrap();
        assert_eq!(
            items,
            vec![
                SourceItem::new("g/p!7", "Review MR !7: Title 7")
                    .with_url("https://gl/g/p/-/merge_requests/7")
            ]
        );
        let custom = ReviewRequests::new(
            "rr",
            Box::new(FakeForge {
                review: vec![mr("g/p", 7, MrState::Open, false)],
                ..FakeForge::default()
            }),
            Some("{project}!{iid}"),
        );
        assert_eq!(
            custom.fetch(&SyncContext::default()).unwrap()[0].title,
            "g/p!7"
        );
        assert!(format!("{custom:?}").contains("ReviewRequests"));
        let failing = ReviewRequests::new(
            "rr",
            Box::new(FakeForge {
                list_error: Some(HttpError::Auth {
                    url: "u".into(),
                    status: 403,
                }),
                ..FakeForge::default()
            }),
            None,
        );
        assert!(matches!(
            failing.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::Auth(_)
        ));
        let github = ReviewRequests::new(
            "gh",
            Box::new(FakeForge {
                kind: Some(ForgeKind::Github),
                ..FakeForge::default()
            }),
            None,
        );
        assert_eq!(github.title, "Review PR #{iid}: {title}");
    }

    #[test]
    fn check_states() {
        let r = |n: u64| parse_ref(&format!("https://gl/g/p/-/merge_requests/{n}")).unwrap();
        let forge = FakeForge {
            by_ref: vec![
                (r(1), Ok(mr("g/p", 1, MrState::Merged, false))),
                (r(2), Ok(mr("g/p", 2, MrState::Closed, false))),
                (r(3), Ok(mr("g/p", 3, MrState::Open, true))),
                (r(4), Ok(mr("g/p", 4, MrState::Open, false))),
                (
                    r(6),
                    Err(HttpError::Status {
                        url: "u".into(),
                        status: 500,
                        excerpt: String::new(),
                    }),
                ),
            ],
            ..FakeForge::default()
        };
        let source = ReviewRequests::new("rr", Box::new(forge), None);
        let origins: Vec<Origin> = (1..=5)
            .map(|n| {
                origin(
                    &format!("g/p!{n}"),
                    Some(&format!("https://gl/g/p/-/merge_requests/{n}")),
                )
            })
            .chain([origin("not-a-ref", None), origin("g/p#9", None)])
            .collect();
        let states = source.check(&origins).unwrap();
        let notes: Vec<(ItemState, &str)> = states
            .iter()
            .map(|s| (s.state, s.note.as_deref().unwrap()))
            .collect();
        assert_eq!(
            notes,
            vec![
                (ItemState::Done, "merged"),
                (ItemState::Done, "closed"),
                (ItemState::Done, "approved by you"),
                (ItemState::Open, "open"),
                (ItemState::Done, "no longer exists"),
            ]
        );
        assert_eq!(states[0].origin, origins[0]);
        let err = source.check(&[origin("g/p!6", None)]).unwrap_err();
        assert!(matches!(err, SourceError::Unavailable(_)), "{err}");
    }
}
