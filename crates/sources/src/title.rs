//! Merge request and issue titles for `tasq mr` (plan T-307), through the
//! forge configured for the URL's host.

use tasq_core::config::{Config, ForgeConfig};
use tasq_core::source::SourceError;

use crate::http::Transport;
use crate::registry::{TransportFactory, forge_client};
use crate::review_requests::{Lookup, lookup};
use crate::url::{RefKind, parse_ref};

/// The `[forge.<name>]` whose host is `host`.
pub fn forge_for_host<'a>(config: &'a Config, host: &str) -> Option<(&'a str, &'a ForgeConfig)> {
    config
        .forge
        .iter()
        .find(|(_, f)| f.host.as_deref() == Some(host))
        .map(|(name, f)| (name.as_str(), f))
}

/// The title of the merge request or issue at `url`: `Ok(None)` when the
/// URL is not a forge reference, no forge is configured for its host, or
/// the item no longer exists; `Err` when the lookup itself failed.
pub fn resolve_title(
    url: &str,
    config: &Config,
    env: &[(String, String)],
    transport: TransportFactory<'_>,
) -> Result<Option<String>, SourceError> {
    let Some(reference) = parse_ref(url) else {
        return Ok(None);
    };
    let Some((name, forge_cfg)) = forge_for_host(config, &reference.host) else {
        return Ok(None);
    };
    let forge = forge_client(name, forge_cfg, env, transport)?;
    let title = match reference.kind {
        RefKind::MergeRequest => lookup(forge.merge_request(&reference))?.map(|mr| mr.title),
        RefKind::Issue => lookup(forge.work_item(&reference))?.map(|item| item.title),
    };
    Ok(title)
}

impl<T> Lookup<T> {
    /// The found value mapped, `None` when gone.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Option<U> {
        match self {
            Self::Found(value) => Some(f(value)),
            Self::Gone => None,
        }
    }
}

/// The real transport, for callers that do not inject one.
pub fn real_transport() -> Box<dyn Transport> {
    Box::new(crate::http::UreqTransport::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, ScriptedTransport};
    use std::cell::RefCell;
    use tasq_core::config::LoadOptions;

    fn config() -> Config {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("c.toml");
        std::fs::write(
            &file,
            "[forge.gitlab]\nhost = \"gl.example.com\"\n\n[forge.gh]\nkind = \"github\"\n",
        )
        .unwrap();
        let mut opts = LoadOptions::new(dir.path());
        opts.explicit_file = Some(file);
        Config::load(&opts).unwrap().config
    }

    fn env() -> Vec<(String, String)> {
        vec![
            ("GITLAB_TOKEN".to_owned(), "t".to_owned()),
            ("GITHUB_TOKEN".to_owned(), "u".to_owned()),
        ]
    }

    #[test]
    fn forge_lookup_by_host() {
        let c = config();
        assert_eq!(
            forge_for_host(&c, "gl.example.com").map(|(n, _)| n),
            Some("gitlab")
        );
        assert_eq!(forge_for_host(&c, "github.com").map(|(n, _)| n), Some("gh"));
        assert_eq!(forge_for_host(&c, "other").map(|(n, _)| n), None);
        assert_eq!(Lookup::Found(2).map(|n| n * 2), Some(4));
        assert_eq!(Lookup::<u8>::Gone.map(|n| n * 2), None);
    }

    #[test]
    fn resolves_titles_or_says_none() {
        let c = config();
        let responses = RefCell::new(vec![vec![
            HttpResponse::new(
                200,
                "{\"title\":\"Add parser\",\"web_url\":\"https://gl.example.com/g/p/-/merge_requests/3\",\"state\":\"opened\"}",
            ),
            HttpResponse::new(200, "{\"id\":1,\"username\":\"me\"}"),
            HttpResponse::new(200, "{\"approved_by\":[]}"),
        ]]);
        let factory = || -> Box<dyn Transport> {
            Box::new(ScriptedTransport::new(responses.borrow_mut().remove(0)))
        };
        assert_eq!(
            resolve_title(
                "https://gl.example.com/g/p/-/merge_requests/3",
                &c,
                &env(),
                &factory
            )
            .unwrap(),
            Some("Add parser".into())
        );
        let issue = || -> Box<dyn Transport> {
            Box::new(ScriptedTransport::new(vec![HttpResponse::new(
                200,
                "{\"title\":\"Bug\",\"html_url\":\"https://github.com/o/r/issues/4\",\"state\":\"open\"}",
            )]))
        };
        assert_eq!(
            resolve_title("https://github.com/o/r/issues/4", &c, &env(), &issue).unwrap(),
            Some("Bug".into())
        );
        let gone = || -> Box<dyn Transport> {
            Box::new(ScriptedTransport::new(vec![HttpResponse::new(404, "{}")]))
        };
        assert_eq!(
            resolve_title("https://github.com/o/r/pull/9", &c, &env(), &gone).unwrap(),
            None
        );
        let unused = || -> Box<dyn Transport> { panic!("no forge for this host") };
        assert_eq!(
            resolve_title("https://example.com/x", &c, &env(), &unused).unwrap(),
            None
        );
        assert_eq!(
            resolve_title(
                "https://unknown.example.com/g/p/-/issues/1",
                &c,
                &env(),
                &unused
            )
            .unwrap(),
            None
        );
        let failing = || -> Box<dyn Transport> {
            Box::new(ScriptedTransport::new(vec![HttpResponse::new(500, "boom")]))
        };
        assert!(matches!(
            resolve_title("https://github.com/o/r/pull/9", &c, &env(), &failing).unwrap_err(),
            SourceError::Unavailable(_)
        ));
        assert!(matches!(
            resolve_title("https://github.com/o/r/pull/9", &c, &[], &failing).unwrap_err(),
            SourceError::Auth(_)
        ));
    }
}
