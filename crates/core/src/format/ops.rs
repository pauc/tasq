//! Edit operations on a [`Document`], one per field or list of the task.
//!
//! A scalar operation (`set_status`, `set_due`, `set_origin`, ...) writes or
//! removes one frontmatter key through the primitives [`set_key`] and
//! [`clear_key`]; nothing else in the file changes. A list operation
//! (`append_progress`, `append_worktree`, `set_description`, ...) edits the
//! body line by line, mirroring the `awk` pass the original script made for
//! the same command, so the insertion rules are the ones the body always
//! had (new sections before `## Progress`, merge requests nested in
//! `## Related`).
//!
//! Every operation touches only the lines it has to and leaves everything
//! else verbatim. Like the script's `awk` rewrites, a list operation that
//! writes leaves every line terminated; the line ending style is the
//! document's.
//!
//! The idempotence checks the script performed before appending (`worktree
//! already tracked`, ...) live here too and are reported through the `bool`
//! return values: `true` means the document changed.

use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};

use crate::clock::{format_date, format_timestamp};
use crate::model::{Link, Origin, Priority, ProgressEntry, Session, Status, Tag, Worktree};

use super::document::{Document, TITLE_PREFIX, heading_name, key};
use super::entry;
use super::read::section;

fn heading(name: &str) -> String {
    format!("## {name}")
}

// ---- the frontmatter ---------------------------------------------------------

/// Writes `key: value` into the frontmatter block: the line of a present
/// key is replaced (a duplicated key is left with one line), a missing key
/// is added after the last line whose key precedes it in [`key::ORDER`],
/// and a key outside that order goes at the end of the block. No other
/// line changes: unknown keys, blank lines and the body stay byte for byte.
/// `id` is written once, when the file is created, and never changed: the
/// document is left alone.
///
/// The value is written as given: like [`set_title`] with an empty title,
/// a value with a line break makes a file the parser refuses (callers
/// check; the typed operations never pass one).
pub fn set_key(doc: &mut Document, key: &str, value: &str) {
    if key == key::ID {
        return;
    }
    doc.set_key(key, value);
}

/// Removes every line of `key` from the frontmatter block. Returns whether
/// there was one. `id` is never removed (a document always has one) and
/// returns `false`.
pub fn clear_key(doc: &mut Document, key: &str) -> bool {
    if key == key::ID {
        return false;
    }
    doc.clear_key(key)
}

/// Writes the origin as `source`, `source_id` and, when it has one,
/// `source_url` (a `source_url` line is removed when the origin has none).
pub fn set_origin(doc: &mut Document, origin: &Origin) {
    doc.set_key(key::SOURCE, &origin.source);
    doc.set_key(key::SOURCE_ID, &origin.external_id);
    match &origin.url {
        Some(url) => doc.set_key(key::SOURCE_URL, url),
        None => {
            doc.clear_key(key::SOURCE_URL);
        }
    }
}

/// Removes the `source`, `source_id` and `source_url` lines. Returns
/// whether any was there.
pub fn clear_origin(doc: &mut Document) -> bool {
    let removed = [key::SOURCE, key::SOURCE_ID, key::SOURCE_URL].map(|k| doc.clear_key(k));
    removed.contains(&true)
}

// ---- the list sections of the body ------------------------------------------

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

/// Whether any `## name` section has a body line for which `found` holds.
/// The duplicate checks below look only where [`super::read`] reads the
/// entries from, so a check never refuses an entry the reader would not see.
fn section_has(doc: &Document, name: &str, found: impl Fn(&str) -> bool) -> bool {
    doc.sections_named(name)
        .iter()
        .any(|s| s.body.iter().any(|line| found(line)))
}

/// `cmd_worktree`: tracks a worktree unless `## Worktrees` already lists its
/// path. Returns whether a line was added.
///
/// The script matched `- <path>` against every line of the file; a list
/// item elsewhere (a description bullet starting with the path) then hid
/// the worktree from the task, so the check is scoped to the section.
pub fn append_worktree(doc: &mut Document, worktree: &Worktree) -> bool {
    let tracked = section_has(doc, section::WORKTREES, |line| {
        entry::parse_worktree(line).is_some_and(|w| w.path == worktree.path)
    });
    if tracked {
        return false;
    }
    append_to_section(doc, section::WORKTREES, &entry::format_worktree(worktree));
    true
}

/// `cmd_session`: tracks a session unless `## Sessions` already lists its
/// id. Returns whether a line was added.
///
/// The script skipped a session whenever `` `id` `` occurred anywhere in
/// the file, so a worktree on branch `x` (``- /path (`x`)``) or a note
/// mentioning `` `x` `` silently swallowed session `x`; the check is scoped
/// to the section.
pub fn append_session(doc: &mut Document, session: &Session) -> bool {
    let tracked = section_has(doc, section::SESSIONS, |line| {
        entry::parse_session(line).is_some_and(|s| s.id == session.id)
    });
    if tracked {
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

/// Inserts a new `## Description` with the body `lines` right after the
/// title line, where `cmd_create` put it: a blank line, the heading, a blank
/// line, the body, and a blank line after when a non-blank line follows.
fn insert_description(doc: &mut Document, lines: &[&str]) {
    doc.insert_all(1, ["", &heading(section::DESCRIPTION), ""]);
    doc.insert_all(4, lines.iter().copied());
    let after = 4 + lines.len();
    if after < doc.line_count() && !doc.is_blank(after) {
        doc.insert(after, "");
    }
}

/// Removes every `## <name>` section, heading and body, so the line before
/// the heading is followed by what came after the section. When the removed
/// section was last in the file, the blank lines left at the end go too.
/// Returns whether the document changed. `tasq` only (the script never
/// removed a section).
fn remove_sections(doc: &mut Document, name: &str) -> bool {
    let headings = doc.find_all(&heading(name));
    for &h in headings.iter().rev() {
        let end = doc.section_end(h);
        for i in (h..end).rev() {
            doc.remove(i);
        }
        // The last section took the blank line before it along; nothing
        // should end a file with blank lines.
        if h == doc.line_count() {
            trim_trailing_blank_lines(doc);
        }
    }
    !headings.is_empty()
}

/// `set_title`: rewrites the title line as `# <title>`. `title` must not be
/// empty (callers check).
pub fn set_title(doc: &mut Document, title: &str) {
    let line = format!("{TITLE_PREFIX}{title}");
    doc.replace(0, line);
}

/// `set_due`: the `due` key, as an ISO date.
pub fn set_due(doc: &mut Document, due: NaiveDate) {
    doc.set_key(key::DUE, &format_date(due));
}

/// `set_description`: the first `## Description` gets a blank line, the
/// lines of `text` and a blank line before the next heading; any other
/// `## Description` section is removed (the projection joins them, so one
/// is all that can round-trip). Without one, the section is inserted right
/// after the title line, where `cmd_create` put it. `text` is non-empty
/// with no blank lines at either end (callers trim, since the projection
/// does).
pub fn set_description(doc: &mut Document, text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    let hdr = heading(section::DESCRIPTION);
    let headings = doc.find_all(&hdr);
    for &h in headings.iter().skip(1).rev() {
        let end = doc.section_end(h);
        for i in (h..end).rev() {
            doc.remove(i);
        }
    }
    match headings.first() {
        Some(&h) => {
            doc.set_body(h, [""].into_iter().chain(lines.iter().copied()));
            // A blank line before the next heading; at the end of the file
            // the trim takes it back.
            let next = doc.section_end(h);
            doc.insert(next, "");
            trim_trailing_blank_lines(doc);
        }
        None => insert_description(doc, &lines),
    }
}

/// `clear_description`: removes every `## Description` section. Returns
/// whether there was one.
pub fn clear_description(doc: &mut Document) -> bool {
    remove_sections(doc, section::DESCRIPTION)
}

/// Drops blank lines at the end of the file (the title line is never
/// blank, so this stops before it).
fn trim_trailing_blank_lines(doc: &mut Document) {
    while doc.is_blank(doc.line_count() - 1) {
        let last = doc.line_count() - 1;
        doc.remove(last);
    }
}

/// `clear_due`: removes the `due` key. Returns whether there was one.
pub fn clear_due(doc: &mut Document) -> bool {
    doc.clear_key(key::DUE)
}

/// `set_closed`: the `closed` key, as a `YYYY-MM-DD HH:MM` timestamp
/// (ADR 0020).
pub fn set_closed(doc: &mut Document, at: NaiveDateTime) {
    doc.set_key(key::CLOSED, &format_timestamp(at));
}

/// `clear_closed`: removes the `closed` key. Returns whether there was one.
pub fn clear_closed(doc: &mut Document) -> bool {
    doc.clear_key(key::CLOSED)
}

/// `set_rank`: the `rank` key (ADR 0025).
pub fn set_rank(doc: &mut Document, rank: i64) {
    doc.set_key(key::RANK, &rank.to_string());
}

/// `clear_rank`: removes the `rank` key. Returns whether there was one.
pub fn clear_rank(doc: &mut Document) -> bool {
    doc.clear_key(key::RANK)
}

/// `set_project`: the `project` key.
pub fn set_project(doc: &mut Document, path: &Path) {
    doc.set_key(key::PROJECT, &path.display().to_string());
}

/// `clear_project`: removes the `project` key. Returns whether there was
/// one.
pub fn clear_project(doc: &mut Document) -> bool {
    doc.clear_key(key::PROJECT)
}

/// `set_tags`: the `tags` key as `[a, b]`, in the caller's order; no tags
/// removes the key.
pub fn set_tags(doc: &mut Document, tags: &[Tag]) {
    if tags.is_empty() {
        doc.clear_key(key::TAGS);
    } else {
        doc.set_key(key::TAGS, &entry::format_tag_list(tags));
    }
}

/// Sets the `status` key.
pub fn set_status(doc: &mut Document, status: &Status) {
    doc.set_key(key::STATUS, status.as_str());
}

/// Sets the `priority` key.
pub fn set_priority(doc: &mut Document, priority: Priority) {
    doc.set_key(key::PRIORITY, priority.as_str());
}

/// Removes the `status` key, whatever it says (`done` included). Returns
/// whether there was one.
pub fn clear_status(doc: &mut Document) -> bool {
    doc.clear_key(key::STATUS)
}

/// `cmd_done` (minus the optional note, which is [`append_progress`]):
/// `status: done`, the only done marker; the title line is not touched.
pub fn set_done(doc: &mut Document) {
    doc.set_key(key::STATUS, key::DONE_STATUS);
}

/// Reopens a done task: removes a `status: done` line; any other status
/// stays. The status is not restored (nothing remembers it); callers set
/// one afterwards if they want.
pub fn set_open(doc: &mut Document) {
    if doc.is_done() {
        doc.clear_key(key::STATUS);
    }
}
