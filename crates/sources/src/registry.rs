//! Building sources from `[[source]]` configuration.

use tasq_core::config::{Config, ForgeConfig, ForgeKind, SourceConfig, SourceKind};
use tasq_core::model::Tag;
use tasq_core::source::{Defaults, Policy, Source, SourceError};

use crate::auth::token;
use crate::forge::Forge;
use crate::github::GitHub;
use crate::gitlab::GitLab;
use crate::http::Transport;
use crate::llm_bridge::LlmBridge;
use crate::review_requests::ReviewRequests;
use crate::work_items::{Filters, WorkItems};

/// Makes a fresh transport per forge client.
pub type TransportFactory<'a> = &'a dyn Fn() -> Box<dyn Transport>;

/// A source ready to sync, with its reconciliation settings.
pub struct Built {
    /// `[[source]] name`.
    pub name: String,
    /// The source.
    pub source: Box<dyn Source>,
    /// What reconciliation may do.
    pub policy: Policy,
    /// Defaults for new tasks.
    pub defaults: Defaults,
}

impl std::fmt::Debug for Built {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Built")
            .field("name", &self.name)
            .field("policy", &self.policy)
            .field("defaults", &self.defaults)
            .finish_non_exhaustive()
    }
}

/// The reconciliation policy of a source config.
pub fn policy_for(cfg: &SourceConfig) -> Result<Policy, SourceError> {
    let flag =
        match &cfg.flag {
            Some(text) => Some(Tag::new(text.as_str()).map_err(|e| {
                SourceError::Unavailable(format!("source {:?}: flag: {e}", cfg.name))
            })?),
            None => None,
        };
    Ok(Policy {
        create_new: cfg.create_new,
        close_when_done: cfg.close_when_done,
        flag,
    })
}

/// The new-task defaults of a source config.
pub fn defaults_for(cfg: &SourceConfig) -> Result<Defaults, SourceError> {
    let tags = cfg
        .tags
        .iter()
        .map(|t| {
            Tag::new(t.as_str())
                .map_err(|e| SourceError::Unavailable(format!("source {:?}: tags: {e}", cfg.name)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Defaults {
        status: cfg.status.clone(),
        tags,
    })
}

/// A forge client for `[forge.<name>]`.
pub fn forge_client(
    name: &str,
    forge: &ForgeConfig,
    env: &[(String, String)],
    transport: TransportFactory<'_>,
) -> Result<Box<dyn Forge>, SourceError> {
    let kind = forge.kind.unwrap_or(ForgeKind::Gitlab);
    let host = forge
        .host
        .clone()
        .unwrap_or_else(|| kind.default_host().to_owned());
    let token = token(name, forge, env)?;
    Ok(match kind {
        ForgeKind::Gitlab => Box::new(GitLab::new(
            transport(),
            &token,
            &host,
            forge.url.as_deref(),
        )),
        ForgeKind::Github => Box::new(GitHub::new(
            transport(),
            &token,
            &host,
            forge.url.as_deref(),
        )),
    })
}

/// Builds one source.
pub fn build_source(
    cfg: &SourceConfig,
    config: &Config,
    env: &[(String, String)],
    transport: TransportFactory<'_>,
) -> Result<Built, SourceError> {
    let policy = policy_for(cfg)?;
    let defaults = defaults_for(cfg)?;
    let forge = |cfg: &SourceConfig| -> Result<Box<dyn Forge>, SourceError> {
        let forge_name = cfg.forge.as_deref().ok_or_else(|| {
            SourceError::Unavailable(format!("source {:?} has no forge", cfg.name))
        })?;
        let forge_cfg = config.forge.get(forge_name).ok_or_else(|| {
            SourceError::Unavailable(format!(
                "source {:?}: no [forge.{forge_name}] block",
                cfg.name
            ))
        })?;
        forge_client(forge_name, forge_cfg, env, transport)
    };
    let source: Box<dyn Source> = match cfg.kind {
        SourceKind::GitlabReviewRequests | SourceKind::GithubReviewRequests => Box::new(
            ReviewRequests::new(&cfg.name, forge(cfg)?, cfg.title.as_deref()),
        ),
        SourceKind::GitlabWorkItems | SourceKind::GithubWorkItems => Box::new(WorkItems::new(
            &cfg.name,
            forge(cfg)?,
            cfg.title.as_deref(),
            Filters {
                labels: cfg.labels.clone(),
                exclude_labels: cfg.exclude_labels.clone(),
                projects: cfg.projects.clone(),
            },
        )),
        SourceKind::LlmBridge => Box::new(LlmBridge {
            name: cfg.name.clone(),
            command: cfg.command.clone().unwrap_or_default(),
            prompt_file: cfg.prompt_file.clone(),
            env: env.to_vec(),
        }),
    };
    Ok(Built {
        name: cfg.name.clone(),
        source,
        policy,
        defaults,
    })
}

/// Builds every enabled source, keeping each failure next to its name so
/// one broken source does not hide the others.
pub fn build_sources(
    config: &Config,
    env: &[(String, String)],
    transport: TransportFactory<'_>,
) -> Vec<(String, Result<Built, SourceError>)> {
    config
        .source
        .iter()
        .filter(|cfg| cfg.enabled)
        .map(|cfg| (cfg.name.clone(), build_source(cfg, config, env, transport)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::ScriptedTransport;
    use tasq_core::config::LoadOptions;
    use tasq_core::model::Status;

    fn load(toml: &str) -> Config {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, toml).unwrap();
        let mut opts = LoadOptions::new(dir.path());
        opts.explicit_file = Some(file);
        Config::load(&opts).unwrap().config
    }

    fn scripted() -> Box<dyn Transport> {
        Box::new(ScriptedTransport::default())
    }

    const CONFIG: &str = r#"
[forge.gitlab]
host = "gl.example.com"

[forge.gh]
kind = "github"

[[source]]
name = "reviews"
kind = "gitlab-review-requests"
forge = "gitlab"
tags = ["gitlab", "review-request"]
status = "ready"
flag = "review-request"

[[source]]
name = "issues"
kind = "github-work-items"
forge = "gh"
labels = ["bug"]
create_new = false

[[source]]
name = "inbox"
kind = "llm-bridge"
command = "claude -p"

[[source]]
name = "off"
kind = "llm-bridge"
command = "x"
enabled = false
"#;

    #[test]
    fn builds_every_enabled_source() {
        let config = load(CONFIG);
        let env = vec![
            ("GITLAB_TOKEN".to_owned(), "a".to_owned()),
            ("GITHUB_TOKEN".to_owned(), "b".to_owned()),
        ];
        let built = build_sources(&config, &env, &scripted);
        let names: Vec<&str> = built.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["reviews", "issues", "inbox"]);
        let reviews = built[0].1.as_ref().unwrap();
        assert_eq!(reviews.source.name(), "reviews");
        assert_eq!(
            reviews.policy.flag,
            Some(Tag::new("review-request").unwrap())
        );
        assert!(reviews.policy.create_new && reviews.policy.close_when_done);
        assert_eq!(reviews.defaults.status, Some(Status::READY));
        assert_eq!(reviews.defaults.tags.len(), 2);
        let issues = built[1].1.as_ref().unwrap();
        assert!(!issues.policy.create_new);
        assert_eq!(issues.defaults, Defaults::default());
        assert!(format!("{issues:?}").starts_with("Built { name: \"issues\""));
        let inbox = built[2].1.as_ref().unwrap();
        assert_eq!(inbox.source.name(), "inbox");
    }

    #[test]
    fn missing_tokens_fail_only_that_source() {
        let config = load(CONFIG);
        let built = build_sources(
            &config,
            &[("GITHUB_TOKEN".to_owned(), "b".to_owned())],
            &scripted,
        );
        assert!(matches!(&built[0].1, Err(SourceError::Auth(m)) if m.contains("GITLAB_TOKEN")));
        assert!(built[1].1.is_ok());
        assert!(built[2].1.is_ok());
    }

    #[test]
    fn invalid_tags_and_flags_are_reported() {
        let mut cfg = load(CONFIG).source[0].clone();
        cfg.flag = Some("#bad flag".into());
        assert!(
            matches!(policy_for(&cfg).unwrap_err(), SourceError::Unavailable(m) if m.contains("flag"))
        );
        cfg.flag = None;
        cfg.tags = vec!["ok".into(), "not ok".into()];
        assert!(
            matches!(defaults_for(&cfg).unwrap_err(), SourceError::Unavailable(m) if m.contains("tags"))
        );
        let config = load(CONFIG);
        let env = vec![("GITLAB_TOKEN".to_owned(), "a".to_owned())];
        let mut cfg = config.source[0].clone();
        cfg.forge = None;
        assert!(matches!(
            build_source(&cfg, &config, &env, &scripted).unwrap_err(),
            SourceError::Unavailable(m) if m.contains("has no forge")
        ));
        cfg.forge = Some("nope".into());
        assert!(matches!(
            build_source(&cfg, &config, &env, &scripted).unwrap_err(),
            SourceError::Unavailable(m) if m.contains("[forge.nope]")
        ));
    }

    #[test]
    fn forge_clients_by_kind() {
        let env = vec![
            ("GITLAB_TOKEN".to_owned(), "a".to_owned()),
            ("GITHUB_TOKEN".to_owned(), "b".to_owned()),
        ];
        let gl = forge_client(
            "gitlab",
            &ForgeConfig {
                kind: Some(ForgeKind::Gitlab),
                host: Some("gl.example.com".into()),
                token_cmd: None,
                url: None,
            },
            &env,
            &scripted,
        )
        .unwrap();
        assert_eq!(
            (gl.kind(), gl.host()),
            (ForgeKind::Gitlab, "gl.example.com")
        );
        let gh = forge_client(
            "gh",
            &ForgeConfig {
                kind: Some(ForgeKind::Github),
                host: None,
                token_cmd: None,
                url: Some("http://127.0.0.1:9/".into()),
            },
            &env,
            &scripted,
        )
        .unwrap();
        assert_eq!((gh.kind(), gh.host()), (ForgeKind::Github, "github.com"));
        let unknown_kind = forge_client(
            "x",
            &ForgeConfig {
                kind: None,
                host: None,
                token_cmd: None,
                url: None,
            },
            &env,
            &scripted,
        )
        .unwrap();
        assert_eq!(unknown_kind.kind(), ForgeKind::Gitlab);
        assert_eq!(unknown_kind.host(), "gitlab.com");
    }
}
