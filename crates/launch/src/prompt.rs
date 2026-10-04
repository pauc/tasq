//! The Claude prompt: a template with `{{name}}` placeholders and
//! `{{#name}}...{{/name}}` sections kept only when `name` is non-empty.
//!
//! The built-in template is `templates/claude.md`; `[launch.claude]
//! prompt_file` replaces it. Prompts are data (plan section 8), so the
//! engine is deliberately tiny and has no logic beyond substitution.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use tasq_core::format::{format_session, format_worktree};
use tasq_core::launch::{LaunchContext, LaunchError};

/// The built-in Claude prompt template.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/claude.md");

static SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\{\{#(\w+)\}\}(.*?)\{\{/(\w+)\}\}").unwrap());
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{(\w+)\}\}").unwrap());

/// Renders `template` with `vars`. A section whose variable is empty or
/// unset disappears with its markers; an unknown placeholder, or a section
/// whose closing tag names another variable, is an error.
pub fn render(template: &str, vars: &BTreeMap<&str, String>) -> Result<String, LaunchError> {
    let mut error: Option<String> = None;
    let sectioned = SECTION.replace_all(template, |c: &Captures<'_>| {
        if c[1] != c[3] {
            error.get_or_insert(format!(
                "section {{{{#{}}}}} is closed by {{{{/{}}}}}",
                &c[1], &c[3]
            ));
            return String::new();
        }
        match vars.get(&c[1]) {
            Some(value) if !value.is_empty() => c[2].to_owned(),
            _ => String::new(),
        }
    });
    let rendered = PLACEHOLDER.replace_all(&sectioned, |c: &Captures<'_>| {
        if let Some(value) = vars.get(&c[1]) {
            value.clone()
        } else {
            error.get_or_insert(format!("unknown placeholder {{{{{}}}}}", &c[1]));
            String::new()
        }
    });
    match error {
        Some(message) => Err(LaunchError::Template(message)),
        None => Ok(rendered.into_owned()),
    }
}

/// The variables the Claude prompt uses, from a launch context.
/// `in_herdr` adds the workspace-renaming instruction.
pub fn variables(ctx: &LaunchContext, in_herdr: bool) -> BTreeMap<&'static str, String> {
    let worktrees: Vec<String> = ctx.task.worktrees.iter().map(format_worktree).collect();
    let sessions: Vec<String> = ctx.task.sessions.iter().map(format_session).collect();
    BTreeMap::from([
        ("id", ctx.task.id.to_string()),
        ("title", ctx.task.title.clone()),
        ("file", ctx.file.display().to_string()),
        ("markdown", ctx.markdown.trim_end().to_owned()),
        ("workdir", ctx.workdir.display().to_string()),
        ("worktrees", worktrees.join("\n")),
        ("sessions", sessions.join("\n")),
        ("statuses", ctx.statuses.join("|")),
        (
            "herdr",
            if in_herdr {
                "1".to_owned()
            } else {
                String::new()
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, (*v).to_owned())).collect()
    }

    #[test]
    fn placeholders_and_sections() {
        let t = "Hi {{name}}.{{#extra}} Extra: {{extra}}.{{/extra}} Bye";
        assert_eq!(
            render(t, &vars(&[("name", "A"), ("extra", "x")])).unwrap(),
            "Hi A. Extra: x. Bye"
        );
        assert_eq!(
            render(t, &vars(&[("name", "A"), ("extra", "")])).unwrap(),
            "Hi A. Bye"
        );
        assert_eq!(render(t, &vars(&[("name", "A")])).unwrap(), "Hi A. Bye");
        let multi = "{{#s}}\nline\n{{/s}}end";
        assert_eq!(render(multi, &vars(&[("s", "1")])).unwrap(), "\nline\nend");
    }

    #[test]
    fn errors() {
        assert_eq!(
            render("{{nope}}", &vars(&[])).unwrap_err(),
            LaunchError::Template("unknown placeholder {{nope}}".into())
        );
        assert_eq!(
            render("{{#a}}x{{/b}}", &vars(&[("a", "1"), ("b", "1")])).unwrap_err(),
            LaunchError::Template("section {{#a}} is closed by {{/b}}".into())
        );
    }

    #[test]
    fn default_template_names_every_tasq_command_and_the_wrapup_skill() {
        for needle in [
            "tasq log {{id}}",
            "tasq set {{id}}",
            "tasq project {{id}}",
            "tasq worktree {{id}}",
            "tasq session {{id}}",
            "tasq mr {{id}}",
            "tasq done {{id}}",
            "/tasq:wrapup",
        ] {
            assert!(DEFAULT_TEMPLATE.contains(needle), "{needle}");
        }
        assert!(!DEFAULT_TEMPLATE.contains("nb todo do"));
    }
}
