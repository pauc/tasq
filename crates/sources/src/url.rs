//! Merge request, pull request and issue URLs on any GitLab or GitHub host.

use std::fmt;

/// What a URL points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// A GitLab merge request or a GitHub pull request.
    MergeRequest,
    /// A GitLab issue or work item, or a GitHub issue.
    Issue,
}

/// A parsed forge URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeRef {
    /// Host without scheme or port handling (`gitlab.example.com`).
    pub host: String,
    /// `group/sub/project` or `owner/repo`.
    pub project: String,
    /// Merge request or issue.
    pub kind: RefKind,
    /// The iid / number.
    pub number: u64,
    /// Whether the URL has GitLab's `/-/` shape (else GitHub's).
    pub gitlab_shaped: bool,
}

impl ForgeRef {
    /// `project!123` for merge requests, `project#123` for issues.
    pub fn short(&self) -> String {
        format!("{}{}{}", self.project, self.marker(), self.number)
    }

    /// `!` or `#`.
    pub fn marker(&self) -> char {
        match self.kind {
            RefKind::MergeRequest => '!',
            RefKind::Issue => '#',
        }
    }
}

impl fmt::Display for ForgeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short())
    }
}

/// Parses `https://host/group/project/-/merge_requests/123`,
/// `.../-/issues/123`, `.../-/work_items/123` (GitLab) and
/// `https://host/owner/repo/pull/123`, `.../issues/123` (GitHub), with any
/// trailing path, query or fragment ignored.
pub fn parse_ref(url: &str) -> Option<ForgeRef> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    if let Some((project, tail)) = path.split_once("/-/") {
        let mut parts = tail.split('/');
        let kind = match parts.next()? {
            "merge_requests" => RefKind::MergeRequest,
            "issues" | "work_items" => RefKind::Issue,
            _ => return None,
        };
        let number = parts.next()?.parse().ok()?;
        return (!project.is_empty()).then(|| ForgeRef {
            host: host.to_owned(),
            project: project.to_owned(),
            kind,
            number,
            gitlab_shaped: true,
        });
    }
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let kind = match parts.next()? {
        "pull" => RefKind::MergeRequest,
        "issues" => RefKind::Issue,
        _ => return None,
    };
    let number = parts.next()?.parse().ok()?;
    (!owner.is_empty() && !repo.is_empty()).then(|| ForgeRef {
        host: host.to_owned(),
        project: format!("{owner}/{repo}"),
        kind,
        number,
        gitlab_shaped: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_urls() {
        let mr = parse_ref(
            "https://gitlab.example.com/group/sub/project/-/merge_requests/123/diffs?x=1#note_2",
        )
        .unwrap();
        assert_eq!(
            mr,
            ForgeRef {
                host: "gitlab.example.com".into(),
                project: "group/sub/project".into(),
                kind: RefKind::MergeRequest,
                number: 123,
                gitlab_shaped: true,
            }
        );
        assert_eq!(mr.short(), "group/sub/project!123");
        assert_eq!(mr.to_string(), "group/sub/project!123");
        let issue = parse_ref("http://gl/g/p/-/issues/4/").unwrap();
        assert_eq!((issue.kind, issue.number), (RefKind::Issue, 4));
        assert_eq!(issue.short(), "g/p#4");
        let work_item = parse_ref("https://gl/g/-/work_items/9").unwrap();
        assert_eq!(
            (work_item.kind, work_item.project.as_str()),
            (RefKind::Issue, "g")
        );
    }

    #[test]
    fn github_urls() {
        let pr = parse_ref("https://github.com/owner/repo/pull/7/files").unwrap();
        assert_eq!(
            pr,
            ForgeRef {
                host: "github.com".into(),
                project: "owner/repo".into(),
                kind: RefKind::MergeRequest,
                number: 7,
                gitlab_shaped: false,
            }
        );
        assert_eq!(pr.short(), "owner/repo!7");
        let issue = parse_ref("https://ghe.example.com/owner/repo/issues/12").unwrap();
        assert_eq!(
            (issue.kind, issue.number, issue.host.as_str()),
            (RefKind::Issue, 12, "ghe.example.com")
        );
    }

    #[test]
    fn rejects_everything_else() {
        for url in [
            "not a url",
            "https://",
            "https://host",
            "https://host/",
            "https://gl/g/p/-/pipelines/3",
            "https://gl/g/p/-/merge_requests/x",
            "https://gl//-/merge_requests/3",
            "https://github.com/owner/repo/commit/abc",
            "https://github.com/owner/repo/pull/",
            "https://github.com//repo/pull/3",
            "https://github.com/owner//pull/3",
            "https://github.com/owner",
            "ftp://gl/g/p/-/issues/1",
        ] {
            assert_eq!(parse_ref(url), None, "{url}");
        }
    }
}
