//! Which editor to run: `$VISUAL`, else `$EDITOR`. `tasq ui` falls back
//! to `vi`; `tasq desc --edit` asks for one to be set instead.

use std::collections::BTreeMap;

/// `$VISUAL`, else `$EDITOR`, else `vi`, split like a shell would
/// (`EDITOR="code -w"` works); an unsplittable value is used whole.
pub fn editor_command(env: &BTreeMap<String, String>) -> Vec<String> {
    configured_editor(env).unwrap_or_else(|| vec!["vi".to_owned()])
}

/// [`editor_command`] without the `vi` fallback: `None` when neither
/// `$VISUAL` nor `$EDITOR` holds anything but blanks.
pub fn configured_editor(env: &BTreeMap<String, String>) -> Option<Vec<String>> {
    let chosen = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|name| env.get(*name))
        .map(|v| v.trim())
        .find(|v| !v.is_empty())?;
    Some(shell_words::split(chosen).unwrap_or_else(|_| vec![chosen.to_owned()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_selection() {
        let mut env = BTreeMap::new();
        assert_eq!(editor_command(&env), ["vi"]);
        assert_eq!(configured_editor(&env), None);
        env.insert("EDITOR".to_owned(), " ".to_owned());
        assert_eq!(configured_editor(&env), None);
        env.insert("EDITOR".to_owned(), "nvim".to_owned());
        assert_eq!(editor_command(&env), ["nvim"]);
        env.insert("VISUAL".to_owned(), "code -w".to_owned());
        assert_eq!(editor_command(&env), ["code", "-w"]);
        env.insert("VISUAL".to_owned(), "  ".to_owned());
        assert_eq!(editor_command(&env), ["nvim"]);
        env.insert("VISUAL".to_owned(), "it's broken".to_owned());
        assert_eq!(editor_command(&env), ["it's broken"]);
    }
}
