//! Cleaning the output of `nb` (the parity tests run the real one), which decorates it with ANSI escapes and
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
