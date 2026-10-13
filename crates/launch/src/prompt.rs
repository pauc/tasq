//! The Claude prompt: a template with `{{name}}` placeholders and
//! `{{#name}}...{{/name}}` sections kept only when `name` is non-empty,
//! rendered by [`tasq_core::template`].
//!
//! The built-in template is `templates/claude.md`; `[launch.claude]
//! prompt_file` replaces it. Prompts are data (plan section 8), so the
//! engine is deliberately tiny and has no logic beyond substitution.

use std::collections::BTreeMap;

use tasq_core::format::{format_session, format_worktree};
use tasq_core::launch::{LaunchContext, LaunchError};

/// The built-in Claude prompt template.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/claude.md");

/// Renders `template` with `vars` ([`tasq_core::template::render`]); a
/// template error is a [`LaunchError::Template`].
pub fn render(template: &str, vars: &BTreeMap<&str, String>) -> Result<String, LaunchError> {
    tasq_core::template::render(template, vars).map_err(|e| LaunchError::Template(e.to_string()))
}

/// The variables the Claude prompt uses, from a launch context.
/// `in_herdr` adds the workspace-renaming instruction. With worktrees
/// disabled, `worktrees` and `worktrees_enabled` are empty, so their
/// sections drop out.
pub fn variables(ctx: &LaunchContext, in_herdr: bool) -> BTreeMap<&'static str, String> {
    let worktrees: Vec<String> = if ctx.worktrees {
        ctx.task.worktrees.iter().map(format_worktree).collect()
    } else {
        Vec::new()
    };
    let sessions: Vec<String> = ctx.task.sessions.iter().map(format_session).collect();
    BTreeMap::from([
        ("id", ctx.task.id.to_string()),
        ("title", ctx.task.title.clone()),
        ("file", ctx.file.display().to_string()),
        ("markdown", ctx.markdown.trim_end().to_owned()),
        ("workdir", ctx.workdir.display().to_string()),
        ("worktrees", worktrees.join("\n")),
        ("worktrees_enabled", flag(ctx.worktrees)),
        ("sessions", sessions.join("\n")),
        ("statuses", ctx.statuses.join("|")),
        ("herdr", flag(in_herdr)),
    ])
}

/// A section switch: `"1"` keeps `{{#name}}...{{/name}}`, empty drops it.
fn flag(on: bool) -> String {
    if on { "1".to_owned() } else { String::new() }
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
