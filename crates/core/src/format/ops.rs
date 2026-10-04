//! Edit operations on a [`Document`], one per command of the original
//! script. Each mirrors the corresponding `awk` pass line by line, so a file
//! edited by `tasq` and the same file edited by the script are identical.
//!
//! Every operation touches only the lines it has to and leaves everything
//! else verbatim. Like the script's `awk` rewrites, an operation that writes
//! leaves every line terminated; the line ending style is the document's.
//!
//! The idempotence checks the script performed before appending (`worktree
//! already tracked`, ...) live here too and are reported through the `bool`
//! return values: `true` means the document changed.

use std::path::Path;

use crate::model::{Link, Priority, ProgressEntry, Session, Status, Workflow, Worktree};

use super::document::{DONE_PREFIX, Document, OPEN_PREFIX, heading_name};
use super::entry;
use super::read::{TagKind, classify, section};

fn heading(name: &str) -> String {
    format!("## {name}")
}

/// `append_to_section`: appends `entry` to the list section `name`, creating
/// the section first when it is missing.
///
/// - A missing section other than Progress is inserted (heading plus a blank
///   line) right before `## Progress` when that exists; otherwise a blank line
///   and the heading are appended at the end of the file.
/// - The entry goes right after the last `- ` line of the section, or, when
///   the section has none, a blank line and the entry follow the heading.
///
/// With duplicate headings the awk's `h` and `last` keep being overwritten:
/// the entry goes after the last `- ` line of *any* section with that
/// heading, and only when there is none after the last heading.
pub fn append_to_section(doc: &mut Document, name: &str, entry: &str) {
    let hdr = heading(name);
    if doc.find_first(&hdr).is_none() {
        // When `name` is Progress itself this lookup is the one that just
        // failed, so a missing Progress section always lands at the end.
        match doc.find_first(&heading(section::PROGRESS)) {
            Some(progress) => doc.insert_all(progress, [hdr.as_str(), ""]),
            None => doc.append_block([hdr.as_str()]),
        }
    }
    let (h, last) = list_positions(doc, |line| line == hdr, |line| heading_name(line).is_some());
    let h = h.expect("heading was just ensured");
    match last {
        Some(last) => doc.insert(last + 1, entry),
        None => doc.insert_all(h + 1, ["", entry]),
    }
    doc.terminate();
}

/// One pass over the lines, as the script's awk did: returns the index of the
/// last heading matched by `is_heading` and the index of the last `- ` line
/// that follows such a heading before a line matched by `closes` (which also
/// matches the headings themselves).
fn list_positions(
    doc: &Document,
    is_heading: impl Fn(&str) -> bool,
    closes: impl Fn(&str) -> bool,
) -> (Option<usize>, Option<usize>) {
    let mut h = None;
    let mut last = None;
    let mut inside = false;
    for (i, line) in doc.lines().enumerate() {
        if is_heading(line) {
            h = Some(i);
            inside = true;
        } else if closes(line) {
            inside = false;
        } else if inside && line.starts_with("- ") {
            last = Some(i);
        }
    }
    (h, last)
}

/// `append_progress`: adds a progress entry (see [`append_to_section`]).
pub fn append_progress(doc: &mut Document, entry: &ProgressEntry) {
    append_to_section(doc, section::PROGRESS, &entry::format_progress(entry));
}

/// `cmd_worktree`: tracks a worktree unless a line already equals `- <path>`
/// or starts with `- <path> `. Returns whether a line was added.
pub fn append_worktree(doc: &mut Document, worktree: &Worktree) -> bool {
    let prefix = format!("- {}", worktree.path.display());
    let tracked = doc
        .lines()
        .any(|l| l == prefix || l.starts_with(&format!("{prefix} ")));
    if tracked {
        return false;
    }
    append_to_section(doc, section::WORKTREES, &entry::format_worktree(worktree));
    true
}

/// `cmd_session`: tracks a session unless `` `id` `` occurs anywhere in the
/// file. Returns whether a line was added.
pub fn append_session(doc: &mut Document, session: &Session) -> bool {
    let needle = format!("`{}`", session.id);
    if doc.lines().any(|l| l.contains(&needle)) {
        return false;
    }
    append_to_section(doc, section::SESSIONS, &entry::format_session(session));
    true
}

/// `append_mr_entry`: adds `- [title](url)` under `### Merge requests`,
/// creating `## Related` and the subsection where the script did. Returns
/// `false` when `(url)` already occurs inside the subsection.
///
/// Like the script's awk passes this works on the whole file, not on the
/// structured view: a `### Merge requests` subsection counts wherever it is,
/// it ends at any line starting with `##`, and with duplicates the last
/// heading and the last `- ` line win.
pub fn append_merge_request(doc: &mut Document, link: &Link) -> bool {
    let sub_hdr = format!("### {}", section::MERGE_REQUESTS);
    let needle = format!("({})", link.url);
    let mut inside = false;
    let tracked = doc.lines().any(|line| {
        if line.starts_with("##") {
            inside = line == sub_hdr;
        }
        inside && line.contains(&needle)
    });
    if tracked {
        return false;
    }
    if doc.find_first(&sub_hdr).is_none() {
        let related = heading(section::RELATED);
        if doc.find_first(&related).is_none() {
            match doc.find_first(&heading(section::PROGRESS)) {
                Some(progress) => doc.insert_all(progress, [related.as_str(), ""]),
                None => doc.append_block([related.as_str()]),
            }
        }
        // `awk '/^## Related$/ { inrel = 1 } inrel && /^## / { print sub; print "" }'`
        let related_at = doc.find_first(&related).expect("ensured above");
        let end = doc.section_end(related_at);
        if end < doc.line_count() {
            doc.insert_all(end, [sub_hdr.as_str(), ""]);
        } else {
            doc.insert_all(end, ["", sub_hdr.as_str()]);
        }
    }
    let (h, last) = list_positions(doc, |line| line == sub_hdr, |line| line.starts_with("##"));
    let h = h.expect("ensured above");
    let entry = entry::format_merge_request(link);
    match last {
        Some(last) => doc.insert(last + 1, &entry),
        None => doc.insert_all(h + 1, ["", entry.as_str()]),
    }
    true
}

/// Adds a link to the top-level `## Related` list. The script had no such
/// command (it only wrote related links on create), so this follows
/// [`append_to_section`] but never descends into `### Merge requests`: the
/// entry goes after the last `- ` line that precedes any `###` subsection.
/// Returns `false` when the url is already listed there.
pub fn append_related(doc: &mut Document, link: &Link) -> bool {
    let listed = doc
        .sections_named(section::RELATED)
        .iter()
        .flat_map(super::document::Section::own_body)
        .filter_map(entry::parse_link)
        .any(|l| l.url == link.url);
    if listed {
        return false;
    }
    let hdr = heading(section::RELATED);
    if doc.find_first(&hdr).is_none() {
        match doc.find_first(&heading(section::PROGRESS)) {
            Some(progress) => doc.insert_all(progress, [hdr.as_str(), ""]),
            None => doc.append_block([hdr.as_str()]),
        }
    }
    let h = doc.find_last(&hdr).expect("ensured above");
    let body = doc.body_range(h);
    let end = body
        .clone()
        .find(|&i| doc.line(i).starts_with("### "))
        .unwrap_or(body.end);
    let entry = entry::format_link(link);
    match (body.start..end)
        .rev()
        .find(|&i| doc.line(i).starts_with("- "))
    {
        Some(last) => doc.insert(last + 1, &entry),
        None => doc.insert_all(body.start, ["", entry.as_str()]),
    }
    true
}

/// `set_project`: replaces the body of `## Project` with a blank line and the
/// path, or inserts the section after `## Description` when that is the first
/// section, else right after the title line.
pub fn set_project(doc: &mut Document, path: &Path) {
    let hdr = heading(section::PROJECT);
    let path = path.display().to_string();
    // Every `## Project` heading gets the new body (the awk loop never reset
    // its match), followed by a blank line when another `## ` heading follows.
    // Going backwards keeps the earlier indices valid while later sections
    // change length.
    let headings = doc.find_all(&hdr);
    for &h in headings.iter().rev() {
        doc.set_body(h, ["", path.as_str()]);
        let next = doc.section_end(h);
        if next < doc.line_count() {
            doc.insert(next, "");
        }
    }
    if !headings.is_empty() {
        return;
    }
    let first = (0..doc.line_count()).find(|&i| heading_name(doc.line(i)).is_some());
    let mut insafter = 1;
    if let Some(first) = first
        && doc.line(first) == heading(section::DESCRIPTION)
    {
        let second = (first + 1..doc.line_count()).find(|&i| heading_name(doc.line(i)).is_some());
        insafter = second.unwrap_or(doc.line_count());
    }
    // `insafter` counts lines to keep before the new section (awk is 1-based).
    let mut at = insafter;
    if !doc.is_blank(at - 1) {
        doc.insert(at, "");
        at += 1;
    }
    doc.insert_all(at, [hdr.as_str(), "", path.as_str()]);
    at += 3;
    if at < doc.line_count() && !doc.is_blank(at) {
        doc.insert(at, "");
    }
    doc.terminate();
}

/// Which kind of tag `cmd_set` and `strip_status_tag` remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Status,
    Priority,
}

fn is_kind(token: &str, kind: Kind, workflow: &Workflow) -> bool {
    match classify(token, workflow) {
        Some(TagKind::Status(_)) => kind == Kind::Status,
        Some(TagKind::Priority(_)) => kind == Kind::Priority,
        _ => false,
    }
}

/// Rewrites the tag lines of every `## Tags` section: tokens of `kind` are
/// removed, the remaining tokens are re-joined with single spaces, and
/// `add` (if any) is appended to the first line that contained a `#`. A line
/// left empty is dropped when `drop_empty` is set (`strip_status_tag`), else
/// kept (`cmd_set` printed the bare new tag on it).
///
/// Returns whether a `#` line was found at all.
fn rewrite_tag_lines(
    doc: &mut Document,
    kind: Kind,
    workflow: &Workflow,
    add: Option<&str>,
    drop_empty: bool,
) -> bool {
    let hdr = heading(section::TAGS);
    // First the lines to rewrite: those with a `#` under a `## Tags` heading
    // (up to the next `## ` heading). Then rewrite them back to front, so a
    // dropped line never shifts an index still to be visited.
    let mut in_tags = false;
    let targets: Vec<usize> = (0..doc.line_count())
        .filter(|&i| {
            let line = doc.line(i);
            if heading_name(line).is_some() {
                in_tags = line == hdr;
                return false;
            }
            in_tags && line.contains('#')
        })
        .collect();
    let first = targets.first().copied();
    for &i in targets.iter().rev() {
        let mut out: Vec<&str> = doc
            .line(i)
            .split_whitespace()
            .filter(|t| !is_kind(t, kind, workflow))
            .collect();
        if let Some(tag) = add
            && first == Some(i)
        {
            out.push(tag);
        }
        if out.is_empty() && drop_empty {
            doc.remove(i);
        } else {
            let line = out.join(" ");
            doc.replace(i, line);
        }
    }
    doc.terminate();
    !targets.is_empty()
}

/// `cmd_set` with a status or priority: with a `## Tags` section, removes the
/// tags of the same kind from its lines and appends `tag` to the first line
/// that had a `#`; without one, appends `## Tags` with the tag at the end of
/// the file.
///
/// Deviation from the script: when `## Tags` exists but has no line with a
/// `#` (a done task whose tags line was dropped), the script lost the tag
/// silently; here a blank line and the tag are inserted after the heading.
fn set_tag(doc: &mut Document, kind: Kind, tag: &str, workflow: &Workflow) {
    let hdr = heading(section::TAGS);
    match doc.find_first(&hdr) {
        None => doc.append_block([hdr.as_str(), "", tag]),
        Some(h) => {
            if !rewrite_tag_lines(doc, kind, workflow, Some(tag), false) {
                doc.insert_all(h + 1, ["", tag]);
            }
        }
    }
}

/// Sets the status tag, keeping priority and topic tags in their order.
pub fn set_status(doc: &mut Document, status: &Status, workflow: &Workflow) {
    set_tag(doc, Kind::Status, &status.to_hash(), workflow);
}

/// Sets the priority tag, keeping status and topic tags in their order.
pub fn set_priority(doc: &mut Document, priority: Priority, workflow: &Workflow) {
    set_tag(doc, Kind::Priority, priority.to_hash(), workflow);
}

/// `strip_status_tag`: removes every status tag from the `## Tags` lines and
/// drops a line that becomes empty (the heading stays). No-op without a
/// `## Tags` section.
pub fn strip_status_tag(doc: &mut Document, workflow: &Workflow) {
    if doc.find_first(&heading(section::TAGS)).is_none() {
        return;
    }
    rewrite_tag_lines(doc, Kind::Status, workflow, None, true);
}

/// `cmd_done` (minus the optional note, which is [`append_progress`]): flips
/// the title to `# [x]` and strips the status tag.
pub fn set_done(doc: &mut Document, workflow: &Workflow) {
    if let Some(title) = doc.title_line().strip_prefix(OPEN_PREFIX) {
        let line = format!("{DONE_PREFIX}{title}");
        doc.replace(0, line);
    }
    strip_status_tag(doc, workflow);
}

/// `nb todo undo`: flips the title back to `# [ ]`. The status tag is not
/// restored (nothing remembers it); callers set one afterwards if they want.
pub fn set_open(doc: &mut Document) {
    if let Some(title) = doc.title_line().strip_prefix(DONE_PREFIX) {
        let line = format!("{OPEN_PREFIX}{title}");
        doc.replace(0, line);
    }
}
