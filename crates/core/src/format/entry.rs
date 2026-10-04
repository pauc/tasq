//! Line grammars for list entries: progress notes, worktrees, sessions,
//! links and origins. Each has a `parse_*` that returns `None` for lines that
//! do not match (they stay in the [`super::Document`] untouched) and a
//! `format_*` that writes exactly what the script wrote.

use std::path::PathBuf;

use crate::clock::{When, format_timestamp, parse_timestamp};
use crate::model::{Link, Origin, ProgressEntry, Session, Worktree};

/// Separator between a session id and its description: space, EM DASH, space.
pub const SESSION_SEPARATOR: &str = " \u{2014} ";

/// Strips the `- ` list marker.
fn item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
}

/// Splits `YYYY-MM-DD HH:MM: rest` or `YYYY-MM-DD: rest` at the colon that
/// follows the timestamp.
fn split_stamp(text: &str) -> Option<(When, &str)> {
    for len in [TIMESTAMP_FORMAT_LEN, DATE_LEN] {
        if text.len() > len
            && text.is_char_boundary(len)
            && text.as_bytes()[len] == b':'
            && let Ok(when) = parse_timestamp(&text[..len])
        {
            return Some((when, &text[len + 1..]));
        }
    }
    None
}

const TIMESTAMP_FORMAT_LEN: usize = "2026-10-04 10:15".len();
const DATE_LEN: usize = "2026-10-04".len();

/// `- YYYY-MM-DD HH:MM: note` or legacy `- YYYY-MM-DD: note`.
///
/// Leading whitespace of the note is dropped (the script's `summary_raw`
/// did the same); the note may be empty.
pub fn parse_progress(line: &str) -> Option<ProgressEntry> {
    let (at, note) = split_stamp(item(line)?)?;
    Some(ProgressEntry {
        at,
        note: note.trim_start().to_owned(),
    })
}

/// The line `append_progress` wrote: `- YYYY-MM-DD HH:MM: note`.
pub fn format_progress(entry: &ProgressEntry) -> String {
    format!("- {}: {}", entry.at, entry.note)
}

/// ``- /path (`branch`)`` or `- /path`. The path is the first
/// whitespace-separated field after the marker, as the script read it.
pub fn parse_worktree(line: &str) -> Option<Worktree> {
    let rest = item(line)?.trim_start();
    let path = rest.split_whitespace().next()?;
    let tail = rest[path.len()..].trim_start();
    let branch = tail
        .strip_prefix("(`")
        .and_then(|t| t.strip_suffix("`)"))
        .filter(|b| !b.is_empty())
        .map(str::to_owned);
    Some(Worktree {
        path: PathBuf::from(path),
        branch,
    })
}

/// The line `cmd_worktree` wrote.
pub fn format_worktree(worktree: &Worktree) -> String {
    let path = worktree.path.display();
    match &worktree.branch {
        Some(branch) => format!("- {path} (`{branch}`)"),
        None => format!("- {path}"),
    }
}

/// ``- YYYY-MM-DD HH:MM: `id` — description`` with the description optional.
///
/// Sessions always carry a time; a date-only stamp does not match.
pub fn parse_session(line: &str) -> Option<Session> {
    let (at, rest) = split_stamp(item(line)?)?;
    let When::DateTime(at) = at else { return None };
    let rest = rest.strip_prefix(" `")?;
    let (id, tail) = rest.split_once('`')?;
    if id.is_empty() {
        return None;
    }
    let description = if tail.is_empty() {
        None
    } else {
        Some(tail.strip_prefix(SESSION_SEPARATOR)?.to_owned())
    };
    Some(Session {
        at,
        id: id.to_owned(),
        launcher: None,
        description,
    })
}

/// The line `cmd_session` wrote. An empty description is omitted, as the
/// script's `${desc:+ — $desc}` did.
pub fn format_session(session: &Session) -> String {
    let mut line = format!("- {}: `{}`", format_timestamp(session.at), session.id);
    if let Some(desc) = session.description.as_deref().filter(|d| !d.is_empty()) {
        line.push_str(SESSION_SEPARATOR);
        line.push_str(desc);
    }
    line
}

/// `- [label](url)` or `- url`.
pub fn parse_link(line: &str) -> Option<Link> {
    let rest = item(line)?.trim();
    if rest.is_empty() {
        return None;
    }
    if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(')'))
        && let Some((label, url)) = inner.rsplit_once("](")
    {
        return Some(Link {
            url: url.to_owned(),
            label: Some(label.to_owned()),
        });
    }
    Some(Link::new(rest))
}

/// `- [label](url)` when labelled, else `- url`.
pub fn format_link(link: &Link) -> String {
    match &link.label {
        Some(label) => format!("- [{label}]({})", link.url),
        None => format!("- {}", link.url),
    }
}

/// `- [title](url)`, the only shape `cmd_mr` wrote; a missing title falls
/// back to the url so the `(url)` idempotence check keeps working.
pub fn format_merge_request(link: &Link) -> String {
    let label = link.label.as_deref().unwrap_or(&link.url);
    format!("- [{label}]({})", link.url)
}

/// `source: external-id` or `source: external-id url`. When only one value
/// follows the source and it looks like a URL, it is both the id and the url.
pub fn parse_origin(line: &str) -> Option<Origin> {
    let (source, rest) = line.split_once(':')?;
    let source = source.trim();
    if source.is_empty() || source.contains(char::is_whitespace) {
        return None;
    }
    let mut fields = rest.split_whitespace();
    let external_id = fields.next()?;
    let url = match fields.next() {
        Some(url) => Some(url.to_owned()),
        None if is_url(external_id) => Some(external_id.to_owned()),
        None => None,
    };
    Some(Origin {
        source: source.to_owned(),
        external_id: external_id.to_owned(),
        url,
    })
}

fn is_url(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

/// The `## Source` line for an origin: `source: id` when the url is absent or
/// equal to the id, else `source: id url`.
pub fn format_origin(origin: &Origin) -> String {
    match &origin.url {
        Some(url) if url != &origin.external_id => {
            format!("{}: {} {url}", origin.source, origin.external_id)
        }
        _ => format!("{}: {}", origin.source, origin.external_id),
    }
}
