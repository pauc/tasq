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
//!
//! On a v2 document (ADR 0026) the list operations work unchanged, since
//! the body is the same, and every scalar operation edits the frontmatter
//! key instead of a section or a tag: [`set_key`] and [`clear_key`] are the
//! primitives, the typed operations (`set_status`, `set_due`, ...) call them
//! and the v1 rules above do not apply. [`set_origin`] and [`clear_origin`]
//! exist for v2 only.

use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};

use crate::clock::{format_date, format_timestamp};
use crate::model::{
    Link, Origin, Priority, ProgressEntry, Session, Status, Tag, Workflow, Worktree,
};

use super::document::{DONE_PREFIX, Document, OPEN_PREFIX, TITLE_PREFIX, heading_name, key};
use super::entry;
use super::read::{TagKind, classify, section};
use super::write::format_tag_list;

fn heading(name: &str) -> String {
    format!("## {name}")
}

// ---- the frontmatter of a v2 document (ADR 0026) ---------------------------

/// Writes `key: value` into the frontmatter block of a v2 document: the
/// line of a present key is replaced (a duplicated key is left with one
/// line), a missing key is added after the last line whose key precedes it
/// in [`key::ORDER`], and a key outside that order goes at the end of the
/// block. No other line changes: unknown keys, blank lines and the body
/// stay byte for byte. A v1 document has no block and is left alone.
pub fn set_key(doc: &mut Document, key: &str, value: &str) {
    doc.set_key(key, value);
}

/// Removes every line of `key` from the frontmatter block. Returns whether
/// there was one. `id` is never removed (a v2 document always has one), and
/// a v1 document has no block; both return `false`.
pub fn clear_key(doc: &mut Document, key: &str) -> bool {
    if key == key::ID {
        return false;
    }
    doc.clear_key(key)
}

/// Writes the origin as `source`, `source_id` and, when it has one,
/// `source_url` (a `source_url` line is removed when the origin has none).
/// v2 only: a v1 document is left alone (nothing rewrote `## Source`).
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
/// whether any was there. v2 only, like [`set_origin`].
pub fn clear_origin(doc: &mut Document) -> bool {
    let removed = [key::SOURCE, key::SOURCE_ID, key::SOURCE_URL].map(|k| doc.clear_key(k));
    removed.contains(&true)
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

/// `set_project`: replaces the body of `## Project` with a blank line and the
/// path, or inserts the section after `## Description` when that is the first
/// section, else right after the title line.
pub fn set_project(doc: &mut Document, path: &Path) {
    let path = path.display().to_string();
    if doc.is_v2() {
        doc.set_key(key::PROJECT, &path);
        return;
    }
    if replace_bodies(doc, section::PROJECT, &path) {
        return;
    }
    let first = (0..doc.line_count()).find(|&i| heading_name(doc.line(i)).is_some());
    let mut insafter = 1;
    if let Some(first) = first
        && doc.line(first) == heading(section::DESCRIPTION)
    {
        insafter = doc.section_end(first);
    }
    insert_section(doc, section::PROJECT, &[&path], insafter);
}

/// Gives every `## <name>` heading the body `["", value]` (the awk loop of
/// `set_project` never reset its match), followed by a blank line when
/// another `## ` heading follows. Going backwards keeps the earlier indices
/// valid while later sections change length. Returns whether a heading was
/// found.
fn replace_bodies(doc: &mut Document, name: &str, value: &str) -> bool {
    let headings = doc.find_all(&heading(name));
    for &h in headings.iter().rev() {
        doc.set_body(h, ["", value]);
        let next = doc.section_end(h);
        if next < doc.line_count() {
            doc.insert(next, "");
        }
    }
    !headings.is_empty()
}

/// Inserts a new `## <name>` section with the body `lines` so that
/// `insafter` lines stay before it (awk is 1-based): a blank line first when
/// the line before is not blank, then heading, blank line, the body, and a
/// blank line after when a non-blank line follows.
fn insert_section(doc: &mut Document, name: &str, lines: &[&str], insafter: usize) {
    let hdr = heading(name);
    let mut at = insafter;
    if !doc.is_blank(at - 1) {
        doc.insert(at, "");
        at += 1;
    }
    doc.insert_all(at, [hdr.as_str(), ""]);
    doc.insert_all(at + 2, lines.iter().copied());
    at += 2 + lines.len();
    if at < doc.line_count() && !doc.is_blank(at) {
        doc.insert(at, "");
    }
    doc.terminate();
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
    if headings.is_empty() {
        false
    } else {
        doc.terminate();
        true
    }
}

/// `set_title`: rewrites the title line with `title`, keeping the open or
/// done marker (v1) or the plain `# ` (v2). `tasq` only; `title` must not
/// be empty (callers check).
pub fn set_title(doc: &mut Document, title: &str) {
    let prefix = if doc.is_v2() {
        TITLE_PREFIX
    } else if doc.is_done() {
        DONE_PREFIX
    } else {
        OPEN_PREFIX
    };
    let line = format!("{prefix}{title}");
    doc.replace(0, line);
}

/// `set_due`: replaces the body of every `## Due` with a blank line and the
/// ISO date, like [`set_project`]; a missing section is inserted after
/// `## Project`, else after `## Description`, else right after the title
/// line (the order `cmd_create` wrote). `tasq` only.
pub fn set_due(doc: &mut Document, due: NaiveDate) {
    let value = format_date(due);
    if doc.is_v2() {
        doc.set_key(key::DUE, &value);
        return;
    }
    if replace_bodies(doc, section::DUE, &value) {
        return;
    }
    let insafter = [section::PROJECT, section::DESCRIPTION]
        .iter()
        .find_map(|name| doc.find_first(&heading(name)))
        .map_or(1, |h| doc.section_end(h));
    insert_section(doc, section::DUE, &[&value], insafter);
}

/// `set_description`: the first `## Description` gets a blank line, the
/// lines of `text` and a blank line before the next heading; any other
/// `## Description` section is removed (the projection joins them, so one
/// is all that can round-trip). Without one, the section is inserted right
/// after the title line, where `cmd_create` put it. `tasq` only; `text` is
/// non-empty with no blank lines at either end (callers trim, since the
/// projection does).
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
        None => insert_section(doc, section::DESCRIPTION, &lines, 1),
    }
}

/// `clear_description`: removes every `## Description` section. Returns
/// whether there was one. `tasq` only.
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

/// `clear_due`: removes every `## Due` section. Returns whether there was
/// one. `tasq` only.
pub fn clear_due(doc: &mut Document) -> bool {
    if doc.is_v2() {
        return doc.clear_key(key::DUE);
    }
    remove_sections(doc, section::DUE)
}

/// `set_closed`: replaces the body of every `## Closed` with a blank line
/// and the `YYYY-MM-DD HH:MM` timestamp, like [`set_due`]; a missing section
/// is inserted after `## Source`, else `## Due`, else `## Project`, else
/// `## Description`, else right after the title line (where
/// [`Document::from_task`] puts it). `tasq` only (ADR 0020).
pub fn set_closed(doc: &mut Document, at: NaiveDateTime) {
    let value = format_timestamp(at);
    if doc.is_v2() {
        doc.set_key(key::CLOSED, &value);
        return;
    }
    if replace_bodies(doc, section::CLOSED, &value) {
        return;
    }
    let insafter = [
        section::SOURCE,
        section::DUE,
        section::PROJECT,
        section::DESCRIPTION,
    ]
    .iter()
    .find_map(|name| doc.find_first(&heading(name)))
    .map_or(1, |h| doc.section_end(h));
    insert_section(doc, section::CLOSED, &[&value], insafter);
}

/// `clear_closed`: removes every `## Closed` section. Returns whether there
/// was one. `tasq` only.
pub fn clear_closed(doc: &mut Document) -> bool {
    if doc.is_v2() {
        return doc.clear_key(key::CLOSED);
    }
    remove_sections(doc, section::CLOSED)
}

/// `set_rank`: replaces the body of every `## Rank` with a blank line and
/// the integer, like [`set_due`]; a missing section is inserted after
/// `## Closed`, else `## Source`, else `## Due`, else `## Project`, else
/// `## Description`, else right after the title line (where
/// [`Document::from_task`] puts it). `tasq` only (ADR 0025).
pub fn set_rank(doc: &mut Document, rank: i64) {
    let value = rank.to_string();
    if doc.is_v2() {
        doc.set_key(key::RANK, &value);
        return;
    }
    if replace_bodies(doc, section::RANK, &value) {
        return;
    }
    let insafter = [
        section::CLOSED,
        section::SOURCE,
        section::DUE,
        section::PROJECT,
        section::DESCRIPTION,
    ]
    .iter()
    .find_map(|name| doc.find_first(&heading(name)))
    .map_or(1, |h| doc.section_end(h));
    insert_section(doc, section::RANK, &[&value], insafter);
}

/// `clear_rank`: removes every `## Rank` section. Returns whether there
/// was one. `tasq` only.
pub fn clear_rank(doc: &mut Document) -> bool {
    if doc.is_v2() {
        return doc.clear_key(key::RANK);
    }
    remove_sections(doc, section::RANK)
}

/// `clear_project`: removes every `## Project` section. Returns whether
/// there was one. `tasq` only.
pub fn clear_project(doc: &mut Document) -> bool {
    if doc.is_v2() {
        return doc.clear_key(key::PROJECT);
    }
    remove_sections(doc, section::PROJECT)
}

/// `set_tags`: makes `tags` the topic tags, keeping the status and priority
/// tags where they are. On every `#` line of `## Tags` the topic tags are
/// removed; the new ones go in front of the first such line (the order
/// `cmd_create` wrote: topics, priority, status). A line left empty is
/// dropped. Without a `#` line the tags go on a new line after the heading;
/// without a `## Tags` section one is appended at the end of the file, as
/// `cmd_set` did. No tags and no section is a no-op. `tasq` only.
///
/// On a v2 document: `tags: [a, b]`, and no tags removes the key.
pub fn set_tags(doc: &mut Document, tags: &[Tag], workflow: &Workflow) {
    if doc.is_v2() {
        if tags.is_empty() {
            doc.clear_key(key::TAGS);
        } else {
            doc.set_key(key::TAGS, &format_tag_list(tags));
        }
        return;
    }
    let hdr = heading(section::TAGS);
    let topics: Vec<String> = tags.iter().map(|t| format!("#{t}")).collect();
    let Some(h) = doc.find_first(&hdr) else {
        if !topics.is_empty() {
            doc.append_block([hdr.as_str(), "", &topics.join(" ")]);
        }
        return;
    };
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
        let mut out: Vec<String> = Vec::new();
        if first == Some(i) {
            out.extend(topics.iter().cloned());
        }
        out.extend(
            doc.line(i)
                .split_whitespace()
                .filter(|t| !matches!(classify(t, workflow), Some(TagKind::Topic(_))))
                .map(str::to_owned),
        );
        if out.is_empty() {
            doc.remove(i);
        } else {
            doc.replace(i, out.join(" "));
        }
    }
    if first.is_none() && !topics.is_empty() {
        doc.insert_all(h + 1, ["", topics.join(" ").as_str()]);
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
/// On a v2 document: the `status` key.
pub fn set_status(doc: &mut Document, status: &Status, workflow: &Workflow) {
    if doc.is_v2() {
        doc.set_key(key::STATUS, status.as_str());
        return;
    }
    set_tag(doc, Kind::Status, &status.to_hash(), workflow);
}

/// Sets the priority tag, keeping status and topic tags in their order.
/// On a v2 document: the `priority` key.
pub fn set_priority(doc: &mut Document, priority: Priority, workflow: &Workflow) {
    if doc.is_v2() {
        doc.set_key(key::PRIORITY, priority.as_str());
        return;
    }
    set_tag(doc, Kind::Priority, priority.to_hash(), workflow);
}

/// `strip_status_tag`: removes every status tag from the `## Tags` lines and
/// drops a line that becomes empty (the heading stays). No-op without a
/// `## Tags` section. On a v2 document: removes the `status` key, whatever
/// it says (`done` included).
pub fn strip_status_tag(doc: &mut Document, workflow: &Workflow) {
    if doc.is_v2() {
        doc.clear_key(key::STATUS);
        return;
    }
    if doc.find_first(&heading(section::TAGS)).is_none() {
        return;
    }
    rewrite_tag_lines(doc, Kind::Status, workflow, None, true);
}

/// `cmd_done` (minus the optional note, which is [`append_progress`]): flips
/// the title to `# [x]` and strips the status tag. On a v2 document:
/// `status: done`, the only done marker; the title line is not touched.
pub fn set_done(doc: &mut Document, workflow: &Workflow) {
    if doc.is_v2() {
        doc.set_key(key::STATUS, key::DONE_STATUS);
        return;
    }
    if let Some(title) = doc.title_line().strip_prefix(OPEN_PREFIX) {
        let line = format!("{DONE_PREFIX}{title}");
        doc.replace(0, line);
    }
    strip_status_tag(doc, workflow);
}

/// `nb todo undo`: flips the title back to `# [ ]`. The status tag is not
/// restored (nothing remembers it); callers set one afterwards if they want.
/// On a v2 document: removes a `status: done` line; any other status stays.
pub fn set_open(doc: &mut Document) {
    if doc.is_v2() {
        if doc.is_done() {
            doc.clear_key(key::STATUS);
        }
        return;
    }
    if let Some(title) = doc.title_line().strip_prefix(DONE_PREFIX) {
        let line = format!("{OPEN_PREFIX}{title}");
        doc.replace(0, line);
    }
}
