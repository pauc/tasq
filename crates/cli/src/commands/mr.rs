//! Merge request links: turning a URL (and maybe a title) into the
//! `- [title](url)` entry the file wants.
//!
//! Title lookup through the forge clients is plan T-503/T-307. Until then,
//! a GitLab or GitHub URL gets the short reference the script's `view`
//! showed for it (`group/project!123`, `owner/repo#123`) as its label, and
//! any other URL needs an explicit title.

use tasq_core::model::Link;

use crate::error::{CliError, Result};

/// The link for `url`, labelled `title` when given, else with
/// [`fallback_label`]; an error when neither yields a title.
pub fn link_for(url: &str, title: Option<&str>) -> Result<Link> {
    if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
        return Ok(Link::labelled(url, title));
    }
    fallback_label(url)
        .map(|label| Link::labelled(url, label))
        .ok_or_else(|| {
            CliError::user(format!(
                "could not resolve the merge request title for {url}; pass it explicitly: tasq mr <id> {url} \"<title>\""
            ))
        })
}

/// `group/project!123` for a GitLab merge request URL
/// (`https://host/group/project/-/merge_requests/123`), `owner/repo#123`
/// for a GitHub pull request (`https://github.com/owner/repo/pull/123`);
/// `None` for anything else.
pub fn fallback_label(url: &str) -> Option<String> {
    let path = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = path.split_once('/')?;
    let path = path.trim_end_matches('/');
    let path = path.split(['?', '#']).next()?;
    if let Some((project, rest)) = path.split_once("/-/merge_requests/") {
        let iid = rest.split('/').next()?;
        return (!project.is_empty() && is_number(iid)).then(|| format!("{project}!{iid}"));
    }
    if host == "github.com" {
        let mut parts = path.split('/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        let kind = parts.next()?;
        let number = parts.next()?;
        if kind == "pull" && !owner.is_empty() && !repo.is_empty() && is_number(number) {
            return Some(format!("{owner}/{repo}#{number}"));
        }
    }
    None
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_and_github_short_references() {
        assert_eq!(
            fallback_label("https://gitlab.example.com/group/sub/project/-/merge_requests/123"),
            Some("group/sub/project!123".to_owned())
        );
        assert_eq!(
            fallback_label("https://gitlab.example.com/g/p/-/merge_requests/123/diffs?x=1"),
            Some("g/p!123".to_owned())
        );
        assert_eq!(
            fallback_label("https://github.com/owner/repo/pull/7"),
            Some("owner/repo#7".to_owned())
        );
        assert_eq!(
            fallback_label("https://github.com/owner/repo/pull/7/files#diff"),
            Some("owner/repo#7".to_owned())
        );
    }

    #[test]
    fn other_urls_have_no_label() {
        assert_eq!(
            fallback_label("https://github.com/owner/repo/issues/7"),
            None
        );
        assert_eq!(
            fallback_label("https://gitlab.example.com/g/p/-/issues/3"),
            None
        );
        assert_eq!(
            fallback_label("https://gitlab.example.com/-/merge_requests/3"),
            None
        );
        assert_eq!(
            fallback_label("https://gitlab.example.com/g/p/-/merge_requests/x"),
            None
        );
        assert_eq!(fallback_label("not a url"), None);
        assert_eq!(fallback_label("https://host"), None);
    }

    #[test]
    fn explicit_title_wins_and_missing_title_is_an_error() {
        let link = link_for("https://x/y/-/merge_requests/1", Some(" Real title ")).unwrap();
        assert_eq!(
            link,
            Link::labelled("https://x/y/-/merge_requests/1", "Real title")
        );
        let link = link_for("https://x/y/-/merge_requests/1", Some("  ")).unwrap();
        assert_eq!(link.label.as_deref(), Some("y!1"));
        let err = link_for("https://example.com/mr", None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "could not resolve the merge request title for https://example.com/mr; pass it explicitly: tasq mr <id> https://example.com/mr \"<title>\""
        );
    }
}
