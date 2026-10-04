//! Issues and work items assigned to the user (plan T-504b), one source
//! per forge, with label and project filters.

use tasq_core::model::Origin;
use tasq_core::source::{ItemState, Source, SourceError, SourceItem, SourceItemState, SyncContext};

use crate::forge::{Forge, WorkItem};
use crate::review_requests::{Lookup, list_error, lookup, origin_ref, render_title};
use crate::url::RefKind;

/// Default title template.
pub const DEFAULT_TITLE: &str = "#{iid}: {title}";

/// Which items a source keeps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filters {
    /// Keep only items carrying at least one of these labels (empty: all).
    pub labels: Vec<String>,
    /// Drop items carrying any of these labels.
    pub exclude_labels: Vec<String>,
    /// Keep only items of these projects: exact `group/project`, or a group
    /// prefix ending in `/` (empty: all).
    pub projects: Vec<String>,
}

impl Filters {
    /// Whether `item` passes every filter.
    pub fn passes(&self, item: &WorkItem) -> bool {
        let labelled =
            self.labels.is_empty() || item.labels.iter().any(|l| self.labels.contains(l));
        let excluded = item.labels.iter().any(|l| self.exclude_labels.contains(l));
        let in_project = self.projects.is_empty()
            || self.projects.iter().any(|p| {
                if let Some(prefix) = p.strip_suffix('/') {
                    item.project == prefix || item.project.starts_with(p)
                } else {
                    item.project == *p
                }
            });
        labelled && !excluded && in_project
    }
}

/// Assigned work items on one forge.
pub struct WorkItems {
    name: String,
    forge: Box<dyn Forge>,
    title: String,
    filters: Filters,
}

impl std::fmt::Debug for WorkItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkItems")
            .field("name", &self.name)
            .field("title", &self.title)
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

impl WorkItems {
    /// A source called `name` over `forge`.
    pub fn new(name: &str, forge: Box<dyn Forge>, title: Option<&str>, filters: Filters) -> Self {
        Self {
            name: name.to_owned(),
            forge,
            title: title.unwrap_or(DEFAULT_TITLE).to_owned(),
            filters,
        }
    }
}

impl Source for WorkItems {
    fn name(&self) -> &str {
        &self.name
    }

    /// Open items assigned to the user that pass the filters, external id
    /// `project#number`.
    fn fetch(&self, _ctx: &SyncContext) -> Result<Vec<SourceItem>, SourceError> {
        let items = self.forge.assigned_work_items().map_err(list_error)?;
        Ok(items
            .iter()
            .filter(|item| !item.closed && self.filters.passes(item))
            .map(|item| {
                SourceItem::new(
                    item.short(),
                    render_title(&self.title, &item.title, item.number, &item.project),
                )
                .with_url(item.url.clone())
            })
            .collect())
    }

    /// Done when closed, reassigned away from the user, or gone.
    fn check(&self, origins: &[Origin]) -> Result<Vec<SourceItemState>, SourceError> {
        let mut out = Vec::new();
        let mut me: Option<String> = None;
        for origin in origins {
            let Some(reference) = origin_ref(origin, self.forge.host(), RefKind::Issue) else {
                continue;
            };
            if reference.kind != RefKind::Issue {
                continue;
            }
            let (state, note) = match lookup(self.forge.work_item(&reference))? {
                Lookup::Gone => (ItemState::Done, "no longer exists"),
                Lookup::Found(item) if item.closed => (ItemState::Done, "closed"),
                Lookup::Found(item) => {
                    if me.is_none() {
                        me = Some(lookup(self.forge.current_user())?.username_or_unavailable()?);
                    }
                    let mine = me
                        .as_deref()
                        .is_some_and(|u| item.assignees.iter().any(|a| a == u));
                    if mine {
                        (ItemState::Open, "open")
                    } else {
                        (ItemState::Done, "no longer assigned to you")
                    }
                }
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

/// The username out of a user lookup; a 404 on `/user` is unavailable.
trait UsernameOrUnavailable {
    fn username_or_unavailable(self) -> Result<String, SourceError>;
}

impl UsernameOrUnavailable for Lookup<crate::forge::User> {
    fn username_or_unavailable(self) -> Result<String, SourceError> {
        match self {
            Lookup::Found(user) => Ok(user.username),
            Lookup::Gone => Err(SourceError::Unavailable(
                "the current user could not be looked up".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{MergeRequest, User};
    use crate::http::HttpError;
    use crate::url::{ForgeRef, parse_ref};
    use std::cell::RefCell;
    use tasq_core::config::ForgeKind;

    fn item(
        project: &str,
        number: u64,
        closed: bool,
        assignees: &[&str],
        labels: &[&str],
    ) -> WorkItem {
        WorkItem {
            project: project.into(),
            number,
            title: format!("Issue {number}"),
            url: format!("https://gl/{project}/-/issues/{number}"),
            closed,
            assignees: assignees.iter().map(|s| (*s).to_owned()).collect(),
            labels: labels.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn filters() {
        let f = Filters::default();
        assert!(f.passes(&item("g/p", 1, false, &[], &[])));
        let f = Filters {
            labels: vec!["team::core".into()],
            exclude_labels: vec!["wontfix".into()],
            projects: vec!["group/".into(), "other/exact".into()],
        };
        assert!(f.passes(&item("group/p", 1, false, &[], &["team::core"])));
        assert!(
            f.passes(&item("group", 1, false, &[], &["team::core"])),
            "the group itself"
        );
        assert!(f.passes(&item("other/exact", 1, false, &[], &["team::core", "x"])));
        assert!(!f.passes(&item("other/exact-not", 1, false, &[], &["team::core"])));
        assert!(
            !f.passes(&item("group/p", 1, false, &[], &["other"])),
            "label missing"
        );
        assert!(
            !f.passes(&item("group/p", 1, false, &[], &["team::core", "wontfix"])),
            "excluded"
        );
        assert!(
            !f.passes(&item("elsewhere/p", 1, false, &[], &["team::core"])),
            "project"
        );
        assert!(
            !f.passes(&item("groupx/p", 1, false, &[], &["team::core"])),
            "prefix needs the slash"
        );
    }

    /// Counts current-user lookups on top of a fake forge.
    struct Counting(FakeForge, std::rc::Rc<RefCell<u32>>);
    impl Forge for Counting {
        fn kind(&self) -> ForgeKind {
            self.0.kind()
        }
        fn host(&self) -> &str {
            self.0.host()
        }
        fn current_user(&self) -> Result<User, HttpError> {
            *self.1.borrow_mut() += 1;
            self.0.current_user()
        }
        fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError> {
            self.0.review_requests()
        }
        fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError> {
            self.0.assigned_work_items()
        }
        fn merge_request(&self, reference: &ForgeRef) -> Result<MergeRequest, HttpError> {
            self.0.merge_request(reference)
        }
        fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError> {
            self.0.work_item(reference)
        }
    }

    /// A forge with fixed items and a current user.
    struct FakeForge {
        items: Vec<WorkItem>,
        by_ref: Vec<(ForgeRef, Result<WorkItem, HttpError>)>,
        user: Result<User, HttpError>,
        asked_user: RefCell<u32>,
    }

    impl FakeForge {
        fn new(items: Vec<WorkItem>) -> Self {
            Self {
                items,
                by_ref: Vec::new(),
                user: Ok(User {
                    id: 1,
                    username: "me".into(),
                }),
                asked_user: RefCell::new(0),
            }
        }
    }

    impl Forge for FakeForge {
        fn kind(&self) -> ForgeKind {
            ForgeKind::Gitlab
        }
        fn host(&self) -> &'static str {
            "gl"
        }
        fn current_user(&self) -> Result<User, HttpError> {
            *self.asked_user.borrow_mut() += 1;
            self.user.clone()
        }
        fn review_requests(&self) -> Result<Vec<MergeRequest>, HttpError> {
            unreachable!()
        }
        fn assigned_work_items(&self) -> Result<Vec<WorkItem>, HttpError> {
            Ok(self.items.clone())
        }
        fn merge_request(&self, _reference: &ForgeRef) -> Result<MergeRequest, HttpError> {
            unreachable!()
        }
        fn work_item(&self, reference: &ForgeRef) -> Result<WorkItem, HttpError> {
            match self
                .by_ref
                .iter()
                .find(|(r, _)| r.number == reference.number)
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

    #[test]
    fn fetch_filters_and_titles() {
        let forge = FakeForge::new(vec![
            item("g/p", 1, false, &["me"], &["bug"]),
            item("g/p", 2, true, &["me"], &["bug"]),
            item("g/p", 3, false, &["me"], &["wontfix"]),
        ]);
        let source = WorkItems::new(
            "wi",
            Box::new(forge),
            None,
            Filters {
                exclude_labels: vec!["wontfix".into()],
                ..Filters::default()
            },
        );
        assert_eq!(source.name(), "wi");
        let items = source.fetch(&SyncContext::default()).unwrap();
        assert_eq!(
            items,
            vec![SourceItem::new("g/p#1", "#1: Issue 1").with_url("https://gl/g/p/-/issues/1")]
        );
        assert!(format!("{source:?}").contains("WorkItems"));
        let custom = WorkItems::new(
            "wi",
            Box::new(FakeForge::new(vec![item("g/p", 1, false, &[], &[])])),
            Some("{project} {iid}"),
            Filters::default(),
        );
        assert_eq!(
            custom.fetch(&SyncContext::default()).unwrap()[0].title,
            "g/p 1"
        );
    }

    #[test]
    fn check_states_and_user_lookup_once() {
        let r = |n: u64| parse_ref(&format!("https://gl/g/p/-/issues/{n}")).unwrap();
        let mut forge = FakeForge::new(Vec::new());
        forge.by_ref = vec![
            (r(1), Ok(item("g/p", 1, true, &["me"], &[]))),
            (r(2), Ok(item("g/p", 2, false, &["someone"], &[]))),
            (r(3), Ok(item("g/p", 3, false, &["someone", "me"], &[]))),
            (
                r(5),
                Err(HttpError::Auth {
                    url: "u".into(),
                    status: 401,
                }),
            ),
        ];
        let source = WorkItems::new("wi", Box::new(forge), None, Filters::default());
        let origin = |id: &str| Origin {
            source: "wi".into(),
            external_id: id.into(),
            url: None,
        };
        let states = source
            .check(&[
                origin("g/p#1"),
                origin("g/p#2"),
                origin("g/p#3"),
                origin("g/p#4"),
                origin("g/p!9"),
                origin("x"),
            ])
            .unwrap();
        let notes: Vec<(ItemState, &str)> = states
            .iter()
            .map(|s| (s.state, s.note.as_deref().unwrap()))
            .collect();
        assert_eq!(
            notes,
            vec![
                (ItemState::Done, "closed"),
                (ItemState::Done, "no longer assigned to you"),
                (ItemState::Open, "open"),
                (ItemState::Done, "no longer exists"),
            ]
        );
        assert!(matches!(
            source.check(&[origin("g/p#5")]).unwrap_err(),
            SourceError::Auth(_)
        ));
        // The user lookup failing is an error too.
        let mut failing = FakeForge::new(Vec::new());
        failing.by_ref = vec![(r(2), Ok(item("g/p", 2, false, &["someone"], &[])))];
        failing.user = Err(HttpError::Status {
            url: "u".into(),
            status: 404,
            excerpt: String::new(),
        });
        let source = WorkItems::new("wi", Box::new(failing), None, Filters::default());
        assert_eq!(
            source.check(&[origin("g/p#2")]).unwrap_err(),
            SourceError::Unavailable("the current user could not be looked up".into())
        );
    }

    #[test]
    fn user_is_fetched_once_per_check() {
        let r = |n: u64| parse_ref(&format!("https://gl/g/p/-/issues/{n}")).unwrap();
        let mut forge = FakeForge::new(Vec::new());
        forge.by_ref = vec![
            (r(1), Ok(item("g/p", 1, false, &["me"], &[]))),
            (r(2), Ok(item("g/p", 2, false, &["me"], &[]))),
        ];
        let counter = std::rc::Rc::new(RefCell::new(0));
        let source = WorkItems::new(
            "wi",
            Box::new(Counting(forge, std::rc::Rc::clone(&counter))),
            None,
            Filters::default(),
        );
        let origin = |id: &str| Origin {
            source: "wi".into(),
            external_id: id.into(),
            url: None,
        };
        let states = source.check(&[origin("g/p#1"), origin("g/p#2")]).unwrap();
        assert_eq!(states.len(), 2);
        assert_eq!(*counter.borrow(), 1);
    }
}
