//! The `{{name}}` template engine shared by the Claude prompt, the summary
//! prompt and the worktree templates (paths, branch names, files, hooks).
//!
//! Two constructs, no logic: `{{name}}` is replaced by the variable's value,
//! and `{{#name}}...{{/name}}` keeps its body only when the variable is
//! non-empty (sections do not nest). A name is one or more letters, digits
//! or underscores. Anything else between braces is left as it is.

use std::borrow::Borrow;
use std::collections::BTreeMap;

use thiserror::Error;

/// Why a template cannot be rendered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    /// `{{name}}` with no such variable.
    #[error("unknown placeholder {{{{{0}}}}}")]
    UnknownPlaceholder(String),
    /// `{{#open}}` closed by `{{/close}}` with another name.
    #[error("section {{{{#{open}}}}} is closed by {{{{/{close}}}}}")]
    MismatchedSection {
        /// The name in the opening tag.
        open: String,
        /// The name in the closing tag.
        close: String,
    },
}

/// Renders `template` with `vars`: sections first (a section whose variable
/// is empty or unset disappears with its markers), then placeholders.
///
/// # Errors
///
/// The first [`TemplateError`] met, sections before placeholders.
pub fn render<K: Borrow<str> + Ord>(
    template: &str,
    vars: &BTreeMap<K, String>,
) -> Result<String, TemplateError> {
    let sectioned = sections(template, |name| {
        vars.get(name).is_some_and(|value| !value.is_empty())
    })?;
    let mut out = String::with_capacity(sectioned.len());
    let mut rest = sectioned.as_str();
    while let Some((before, tag, after)) = next_tag(rest, "") {
        out.push_str(before);
        let value = vars
            .get(tag)
            .ok_or_else(|| TemplateError::UnknownPlaceholder(tag.to_owned()))?;
        out.push_str(value);
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}

/// Every variable `template` refers to, in placeholders and section tags,
/// sorted and without duplicates.
pub fn names(template: &str) -> Vec<&str> {
    let mut found: Vec<&str> = ["", "#", "/"]
        .into_iter()
        .flat_map(|sigil| {
            let mut names = Vec::new();
            let mut rest = template;
            while let Some((_, name, after)) = next_tag(rest, sigil) {
                names.push(name);
                rest = after;
            }
            names
        })
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// Keeps or drops every `{{#name}}...{{/name}}` section by `keep(name)`.
/// An opening tag with no closing tag after it stays as text.
fn sections(template: &str, keep: impl Fn(&str) -> bool) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, open, after_open)) = next_tag(rest, "#") {
        let Some((body, close, after_close)) = next_tag(after_open, "/") else {
            break;
        };
        if open != close {
            return Err(TemplateError::MismatchedSection {
                open: open.to_owned(),
                close: close.to_owned(),
            });
        }
        out.push_str(before);
        if keep(open) {
            out.push_str(body);
        }
        rest = after_close;
    }
    out.push_str(rest);
    Ok(out)
}

/// The first `{{<sigil>name}}` in `text`: the text before it, the name and
/// the text after it.
fn next_tag<'a>(text: &'a str, sigil: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let opener = format!("{{{{{sigil}");
    text.char_indices()
        .map(|(start, _)| start)
        .filter(|&start| text[start..].starts_with(&opener))
        .find_map(|start| {
            let name_start = start + opener.len();
            let name_len = text[name_start..]
                .find(|c: char| !is_name_char(c))
                .unwrap_or(text.len() - name_start);
            let name_end = name_start + name_len;
            (name_len > 0 && text[name_end..].starts_with("}}")).then(|| {
                (
                    &text[..start],
                    &text[name_start..name_end],
                    &text[name_end + 2..],
                )
            })
        })
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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
        assert_eq!(
            render(
                "{{#a}}1{{/a}}{{#b}}2{{/b}}{{#a}}3{{/a}}",
                &vars(&[("a", "y")])
            )
            .unwrap(),
            "13"
        );
    }

    #[test]
    fn values_are_not_rendered_again() {
        assert_eq!(
            render("{{a}}{{b}}", &vars(&[("a", "{{b}}"), ("b", "x")])).unwrap(),
            "{{b}}x"
        );
    }

    #[test]
    fn what_is_not_a_tag_stays_as_text() {
        let none = vars(&[]);
        for text in [
            "plain",
            "{{ spaced }}",
            "{{}}",
            "{{a-b}}",
            "{{#}}x{{/}}",
            "{a}",
            "{{a}",
            "{{{",
            "}}{{",
            "{{#open}} never closed",
            "only {{/close}}",
        ] {
            assert_eq!(render(text, &none).as_deref(), Ok(text), "{text:?}");
        }
        assert_eq!(
            render("{{{a}}}", &vars(&[("a", "x")])).as_deref(),
            Ok("{x}")
        );
        assert_eq!(
            render("{{ a }}{{a}}", &vars(&[("a", "x")])).as_deref(),
            Ok("{{ a }}x")
        );
        assert_eq!(
            render("é{{été_1}}ü", &vars(&[("été_1", "x")])).as_deref(),
            Ok("éxü")
        );
    }

    #[test]
    fn errors() {
        assert_eq!(
            render("a {{nope}} b {{other}}", &vars(&[])),
            Err(TemplateError::UnknownPlaceholder("nope".into()))
        );
        assert_eq!(
            render("{{#a}}x{{/b}}", &vars(&[("a", "1"), ("b", "1")])),
            Err(TemplateError::MismatchedSection {
                open: "a".into(),
                close: "b".into()
            })
        );
        // Sections are checked before placeholders.
        assert_eq!(
            render("{{nope}}{{#a}}{{/b}}", &vars(&[])),
            Err(TemplateError::MismatchedSection {
                open: "a".into(),
                close: "b".into()
            })
        );
        assert_eq!(
            TemplateError::UnknownPlaceholder("nope".into()).to_string(),
            "unknown placeholder {{nope}}"
        );
        assert_eq!(
            TemplateError::MismatchedSection {
                open: "a".into(),
                close: "b".into()
            }
            .to_string(),
            "section {{#a}} is closed by {{/b}}"
        );
    }

    #[test]
    fn keys_can_be_owned_strings() {
        let owned: BTreeMap<String, String> = BTreeMap::from([("db".to_owned(), "app".to_owned())]);
        assert_eq!(render("{{db}}_test", &owned).as_deref(), Ok("app_test"));
    }

    #[test]
    fn names_lists_every_variable_once() {
        assert_eq!(
            names("{{b}} {{#a}}{{c}}{{/a}} {{b}} {{ x }} {{#}} {{/d}}"),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(names("none"), Vec::<&str>::new());
    }
}
