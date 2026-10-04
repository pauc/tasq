//! Cleaning the output of `nb`, which decorates it with ANSI escapes and
//! carriage returns in some environments. Mirrors the script's
//! `sed $'s/\033\\[[0-9;?]*[a-zA-Z]//g; s/\r//g'` plus trimming.

/// Removes CSI escape sequences (`ESC [ ... letter`), the two-byte
/// `ESC (B`-style charset selections nb emits, and every carriage return.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {}
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    // Parameter and intermediate bytes, then one final letter.
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some('(' | ')') => {
                    chars.next();
                    chars.next();
                }
                _ => {}
            },
            _ => out.push(c),
        }
    }
    out
}

/// The last non-blank line of `text`, cleaned and trimmed; what the script's
/// `tail -1 | sed ...` kept of `nb notebooks show --path`. Empty when there
/// is no such line.
pub fn last_line(text: &str) -> String {
    strip_ansi(text)
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_sequences_and_carriage_returns() {
        assert_eq!(strip_ansi("\u{1b}[32m/path\u{1b}[0m\r\n"), "/path\n");
        assert_eq!(strip_ansi("\u{1b}[38;5;69mnb\u{1b}[?7l"), "nb");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("a\rb\rc"), "abc");
    }

    #[test]
    fn strips_charset_selections() {
        assert_eq!(strip_ansi("\u{1b}(B\u{1b}[m[\u{1b}(0x"), "[x");
    }

    #[test]
    fn lone_or_unknown_escape_is_dropped_and_the_rest_kept() {
        assert_eq!(strip_ansi("a\u{1b}b"), "ab");
        assert_eq!(strip_ansi("a\u{1b}"), "a");
        assert_eq!(
            strip_ansi("a\u{1b}[12"),
            "a",
            "unterminated CSI eats to the end"
        );
    }

    #[test]
    fn last_line_takes_the_last_non_blank_trimmed_line() {
        assert_eq!(last_line("Welcome\r\n  /nb/home \r\n\n"), "/nb/home");
        assert_eq!(last_line("\u{1b}[1m/a\u{1b}[0m\n"), "/a");
        assert_eq!(last_line("single"), "single");
        assert_eq!(last_line(""), "");
        assert_eq!(last_line("\n  \n"), "");
    }
}
