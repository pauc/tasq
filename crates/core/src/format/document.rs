//! The lossless layer: a task file as a line buffer with a structured view.
//!
//! [`Document`] keeps every byte of the file it was parsed from: the lines
//! (with their individual line endings), whether the file ended with a
//! newline, and the title line. Sections are not stored as separate blocks;
//! they are computed views over the lines ([`Document::sections`]), because
//! every edit the original script made was a line-level `awk` pass and the
//! operations in [`super::ops`] mirror those passes one line at a time.
//!
//! A file (ADR 0026) opens with a frontmatter block, `---`, `key: value`
//! lines, `---`, and the body that follows starts with `# Title`. The block
//! is kept as its own run of lines in front of the body, so the body lines
//! (what [`Document::lines`] and the sections index) are numbered from the
//! title line and the list-section operations never see the block.
//!
//! Only the first lines are validated here (the fences, the `id` key and
//! the title line, see [`FormatError`]); everything else is kept verbatim,
//! so `render(parse(x)) == x` holds for every string that parses.

use std::fmt;
use std::ops::Range;

use thiserror::Error;

use crate::model::TaskId;

/// Errors from parsing a task file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The first line is not `---`, so the file is not a task file but
    /// some other markdown file.
    #[error("not a task file: first line {first_line:?} is not '---'")]
    NotFrontmatter {
        /// The offending first line, without its line ending.
        first_line: String,
    },
    /// Line 1 is `---` and no later line closes the block.
    #[error("line 1: the frontmatter has no closing '---'")]
    UnterminatedFrontmatter,
    /// The block closes without an `id` key.
    #[error("line {line}: the frontmatter closes without an 'id' key")]
    MissingId {
        /// 1-based number of the closing `---` line.
        line: usize,
    },
    /// The `id` value is not a positive integer written plainly (`47`, not
    /// `047`, `+47` or `0`).
    #[error("line {line}: id {value:?} is not a positive integer")]
    InvalidId {
        /// 1-based number of the `id:` line.
        line: usize,
        /// The value after `id:`, trimmed.
        value: String,
    },
    /// The first non-blank line after the block is not `# Title`.
    #[error(
        "line {line}: expected the title line '# Title' after the frontmatter, found {found:?}"
    )]
    MissingTitle {
        /// 1-based number of the offending line (one past the last line
        /// when the file ends first).
        line: usize,
        /// That line, or the empty string at the end of the file.
        found: String,
    },
}

/// A line ending style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Newline {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    /// The bytes of this ending.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// One line of the file: its text and the ending that followed it.
///
/// The ending of the last line is only meaningful when
/// [`Document::ends_with_newline`] is true.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    text: String,
    ending: Newline,
}

/// Prefix of the title line; the done marker is `status: done`, not the
/// title.
pub const TITLE_PREFIX: &str = "# ";
/// The line that opens and closes the frontmatter block.
pub const FENCE: &str = "---";

/// Names of the frontmatter keys (ADR 0026).
pub mod key {
    /// `id`: the task's id, a positive integer; required.
    pub const ID: &str = "id";
    /// `status`: a workflow status name, or [`DONE_STATUS`].
    pub const STATUS: &str = "status";
    /// The `status` value of a done task.
    pub const DONE_STATUS: &str = "done";
    /// `priority`: `A`, `B` or `C`.
    pub const PRIORITY: &str = "priority";
    /// `rank`: an integer (ADR 0025).
    pub const RANK: &str = "rank";
    /// `due`: `YYYY-MM-DD`.
    pub const DUE: &str = "due";
    /// `closed`: `YYYY-MM-DD HH:MM`, read on done tasks only.
    pub const CLOSED: &str = "closed";
    /// `project`: a directory path.
    pub const PROJECT: &str = "project";
    /// `source`: the origin's source name.
    pub const SOURCE: &str = "source";
    /// `source_id`: the origin's external id.
    pub const SOURCE_ID: &str = "source_id";
    /// `source_url`: the origin's url.
    pub const SOURCE_URL: &str = "source_url";
    /// `tags`: the topic tags as a one-line list, `[a, b]`, without `#`.
    pub const TAGS: &str = "tags";
    /// The keys in canonical order: the order a fresh block is written in,
    /// and the place a key added to an existing block takes among the
    /// known keys present.
    pub const ORDER: [&str; 11] = [
        ID, STATUS, PRIORITY, RANK, DUE, CLOSED, PROJECT, SOURCE, SOURCE_ID, SOURCE_URL, TAGS,
    ];
}

/// A task file, byte for byte.
///
/// Construct one with [`Document::parse`] (or [`super::parse_v2`], which
/// also projects the typed [`crate::model::Task`]) and write it back with
/// [`Document::render`]. Edit it through the operations in [`super::ops`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The frontmatter region: the opening `---`, the lines of the block,
    /// the closing `---` and any blank lines before the title.
    front: Vec<Line>,
    /// The body: the title line first, then the sections.
    lines: Vec<Line>,
    /// Ending used for lines this document creates: the ending of the first
    /// line, or LF when the file was a single unterminated line.
    newline: Newline,
    trailing_newline: bool,
}

/// A level-2 section (`## Name`) as a view over the document's lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Heading level: always 2 for sections, 3 for subsections.
    pub level: u8,
    /// The text after `## ` (or `### `), exactly as written.
    pub name: &'a str,
    /// Index of the heading line in [`Document::lines`].
    pub heading: usize,
    /// The lines between this heading and the next one of the same or a
    /// higher level, verbatim (blank lines included).
    pub body: Vec<&'a str>,
}

impl<'a> Section<'a> {
    /// The body without leading and trailing blank lines.
    pub fn trimmed_body(&self) -> &[&'a str] {
        let start = self
            .body
            .iter()
            .position(|l| !l.trim().is_empty())
            .unwrap_or(self.body.len());
        let end = self
            .body
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(start, |i| i + 1);
        &self.body[start..end]
    }

    /// Level-3 subsections (`### Name`) inside this section, in order.
    ///
    /// A subsection ends at the next line starting with `##`, which is how
    /// the script closed `### Merge requests`.
    pub fn subsections(&self) -> Vec<Section<'a>> {
        // Written as bounded iteration over heading positions rather than a
        // manual index loop: no single operator change can make it spin
        // forever while allocating (that mutant once took a machine down).
        let headings = self
            .body
            .iter()
            .enumerate()
            .filter_map(|(i, line)| line.strip_prefix("### ").map(|name| (i, name)));
        let mut out = Vec::new();
        let mut covered_until = 0;
        for (i, name) in headings {
            if i < covered_until {
                // A `###` line inside a previous subsection's body cannot
                // happen (any `##` closes it), but keep the invariant explicit.
                continue;
            }
            let start = i + 1;
            let end = self.body[start..]
                .iter()
                .position(|l| l.starts_with("##"))
                .map_or(self.body.len(), |n| start + n);
            out.push(Section {
                level: 3,
                name,
                heading: self.heading + 1 + i,
                body: self.body[start..end].to_vec(),
            });
            covered_until = end;
        }
        out
    }

    /// The body lines that are not part of any subsection.
    pub fn own_body(&self) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut in_sub = false;
        for line in &self.body {
            if line.starts_with("### ") {
                in_sub = true;
            } else if line.starts_with("##") {
                in_sub = false;
            }
            if !in_sub {
                out.push(*line);
            }
        }
        out
    }
}

/// The frontmatter block: the lines between the two `---` fences, verbatim,
/// with the `key: value` reading over them.
///
/// A line is an entry when it has a `:` and the text before it, trimmed, is
/// a single word; the value is the text after the first `:`, trimmed. Other
/// lines (blank, or not of that shape) are kept and ignored. With a key on
/// two lines the first wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter<'a> {
    lines: Vec<&'a str>,
}

impl<'a> Frontmatter<'a> {
    /// The lines between the fences, without their endings.
    pub fn lines(&self) -> &[&'a str] {
        &self.lines
    }

    /// Every `key: value` entry, in file order, duplicates included.
    pub fn entries(&self) -> impl Iterator<Item = (&'a str, &'a str)> + '_ {
        self.lines.iter().filter_map(|line| key_value(line))
    }

    /// The value of the first entry for `key`.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.entries().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

/// Splits a `key: value` line; `None` for lines of any other shape.
pub(super) fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, value.trim()))
}

/// The id a frontmatter value denotes: a positive integer written plainly.
pub(super) fn parse_id(value: &str) -> Option<TaskId> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == value)
        .map(TaskId::from)
}

impl Document {
    /// Splits `text` into lines and checks the shape: line 1 is `---`, the
    /// block closes, carries a valid `id`, and `# Title` is on the first
    /// non-blank line after it.
    pub fn parse(text: &str) -> Result<Self, FormatError> {
        let (mut lines, trailing_newline) = split_lines(text);
        let first_line = lines.first().map(|l| l.text.as_str()).unwrap_or_default();
        // The first line, when it is the only line and unterminated, was
        // given `Lf` by `split_lines`; so its ending is always the style new
        // lines should use.
        let newline = lines.first().map_or(Newline::Lf, |l| l.ending);
        if first_line != FENCE {
            return Err(FormatError::NotFrontmatter {
                first_line: first_line.to_owned(),
            });
        }
        let close = closing_fence(&lines).ok_or(FormatError::UnterminatedFrontmatter)?;
        match lines[1..close].iter().enumerate().find_map(|(i, l)| {
            key_value(&l.text)
                .filter(|(k, _)| *k == key::ID)
                .map(|(_, v)| (i + 1, v))
        }) {
            None => return Err(FormatError::MissingId { line: close + 1 }),
            Some((i, value)) if parse_id(value).is_none() => {
                return Err(FormatError::InvalidId {
                    line: i + 1,
                    value: value.to_owned(),
                });
            }
            Some(_) => {}
        }
        let body_start = (close + 1..lines.len())
            .find(|&i| !lines[i].text.trim().is_empty())
            .unwrap_or(lines.len());
        let found = lines
            .get(body_start)
            .map(|l| l.text.as_str())
            .unwrap_or_default();
        if v2_title_of(found).is_none() {
            return Err(FormatError::MissingTitle {
                line: body_start + 1,
                found: found.to_owned(),
            });
        }
        let body = lines.split_off(body_start);
        Ok(Self {
            front: lines,
            lines: body,
            newline,
            trailing_newline,
        })
    }

    /// Writes the document back, byte for byte if it was not edited.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.front {
            out.push_str(&line.text);
            out.push_str(line.ending.as_str());
        }
        let last = self.lines.len().saturating_sub(1);
        for (i, line) in self.lines.iter().enumerate() {
            out.push_str(&line.text);
            if i != last || self.trailing_newline {
                out.push_str(line.ending.as_str());
            }
        }
        out
    }

    /// Index in `front` of the closing fence.
    fn close_index(&self) -> usize {
        closing_fence(&self.front).expect("a parsed document has a closed frontmatter block")
    }

    /// The frontmatter block.
    pub fn frontmatter(&self) -> Frontmatter<'_> {
        Frontmatter {
            lines: self.front[1..self.close_index()]
                .iter()
                .map(|l| l.text.as_str())
                .collect(),
        }
    }

    /// The id written in the frontmatter (the parse checked it is there and
    /// valid).
    pub fn id(&self) -> TaskId {
        self.frontmatter()
            .get(key::ID)
            .and_then(parse_id)
            .expect("a parsed document has a valid id key")
    }

    /// The ending used for lines this document creates.
    pub fn newline(&self) -> Newline {
        self.newline
    }

    /// Whether the last line is terminated.
    pub fn ends_with_newline(&self) -> bool {
        self.trailing_newline
    }

    /// The body lines without their endings: everything after the
    /// frontmatter, the title line first.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|l| l.text.as_str())
    }

    /// Number of body lines (see [`Self::lines`]).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The title line, without its ending: the first body line.
    pub fn title_line(&self) -> &str {
        &self.lines[0].text
    }

    /// The title text after `# `.
    pub fn title(&self) -> &str {
        v2_title_of(self.title_line()).unwrap_or_default()
    }

    /// Whether the task is done: the frontmatter says `status: done`.
    pub fn is_done(&self) -> bool {
        self.frontmatter().get(key::STATUS) == Some(key::DONE_STATUS)
    }

    /// Every level-2 section, in file order. A section runs from its heading
    /// to the next line starting with `## ` (so `### ` lines belong to the
    /// body of the enclosing section).
    pub fn sections(&self) -> Vec<Section<'_>> {
        // Every `## ` line opens a section and the body of one ends at the
        // next `## ` line, so the sections are exactly the heading lines.
        (0..self.lines.len())
            .filter_map(|i| {
                let name = heading_name(&self.lines[i].text)?;
                Some(Section {
                    level: 2,
                    name,
                    heading: i,
                    body: self.lines[self.body_range(i)]
                        .iter()
                        .map(|l| l.text.as_str())
                        .collect(),
                })
            })
            .collect()
    }

    /// The sections named exactly `name`, in file order.
    pub fn sections_named<'a>(&'a self, name: &str) -> Vec<Section<'a>> {
        self.sections()
            .into_iter()
            .filter(|s| s.name == name)
            .collect()
    }

    /// The first section named exactly `name`.
    pub fn section(&self, name: &str) -> Option<Section<'_>> {
        self.sections().into_iter().find(|s| s.name == name)
    }

    // ---- line-level editing used by `ops`; kept crate-private -------------

    /// Index of the first line after `heading` that starts with `## `, or the
    /// line count.
    pub(super) fn section_end(&self, heading: usize) -> usize {
        (heading + 1..self.lines.len())
            .find(|&i| heading_name(&self.lines[i].text).is_some())
            .unwrap_or(self.lines.len())
    }

    /// The line indices of the body of the section whose heading is at
    /// `heading`: from the line after it up to [`Self::section_end`].
    pub(super) fn body_range(&self, heading: usize) -> Range<usize> {
        heading + 1..self.section_end(heading)
    }

    /// Replaces the body of the section whose heading is at `heading` with
    /// `texts`; the heading stays, every line ends up terminated.
    pub(super) fn set_body<S: Into<String>>(
        &mut self,
        heading: usize,
        texts: impl IntoIterator<Item = S>,
    ) {
        self.lines.drain(self.body_range(heading));
        self.insert_all(heading + 1, texts);
        self.trailing_newline = true;
    }

    /// Indices of every line equal to `text`.
    pub(super) fn find_all(&self, text: &str) -> Vec<usize> {
        (0..self.lines.len())
            .filter(|&i| self.lines[i].text == text)
            .collect()
    }

    /// Index of the last line equal to `text`.
    pub(super) fn find_last(&self, text: &str) -> Option<usize> {
        self.find_all(text).pop()
    }

    /// Index of the first line equal to `text`.
    pub(super) fn find_first(&self, text: &str) -> Option<usize> {
        self.find_all(text).first().copied()
    }

    pub(super) fn line(&self, i: usize) -> &str {
        &self.lines[i].text
    }

    pub(super) fn is_blank(&self, i: usize) -> bool {
        self.lines[i].text.is_empty()
    }

    /// Inserts `text` so that it becomes line `at`. Like an `awk` rewrite, this
    /// leaves every line terminated.
    pub(super) fn insert(&mut self, at: usize, text: impl Into<String>) {
        self.lines.insert(
            at,
            Line {
                text: text.into(),
                ending: self.newline,
            },
        );
        self.trailing_newline = true;
    }

    /// Inserts several lines starting at `at`.
    pub(super) fn insert_all<S: Into<String>>(
        &mut self,
        at: usize,
        texts: impl IntoIterator<Item = S>,
    ) {
        for (k, text) in texts.into_iter().enumerate() {
            self.insert(at + k, text);
        }
    }

    pub(super) fn remove(&mut self, at: usize) {
        self.lines.remove(at);
        self.trailing_newline = true;
    }

    pub(super) fn replace(&mut self, at: usize, text: impl Into<String>) {
        self.lines[at].text = text.into();
        self.trailing_newline = true;
    }

    /// Mirrors `printf "\n...\n" >> file`: the leading newline terminates an
    /// unterminated last line, otherwise it produces a blank line; then the
    /// given lines follow, each terminated.
    pub(super) fn append_block<S: Into<String>>(&mut self, texts: impl IntoIterator<Item = S>) {
        if self.trailing_newline {
            self.lines.push(Line {
                text: String::new(),
                ending: self.newline,
            });
        }
        for text in texts {
            self.lines.push(Line {
                text: text.into(),
                ending: self.newline,
            });
        }
        self.trailing_newline = true;
    }

    // ---- frontmatter editing used by `ops` --------------------------------

    /// Indices in `front` of the entry lines for `key`, in order; `close` is
    /// the closing fence.
    fn key_lines(&self, close: usize, key: &str) -> Vec<usize> {
        (1..close)
            .filter(|&i| key_value(&self.front[i].text).is_some_and(|(k, _)| k == key))
            .collect()
    }

    /// Writes `key: value` into the block: the first line of `key` is
    /// replaced and any later line of it removed; a missing key is added
    /// after the last line whose key precedes it in [`key::ORDER`] (`id`,
    /// always present, precedes every other known key), or, for a key not
    /// in that order, right before the closing fence. No other line
    /// changes, the body included.
    pub(super) fn set_key(&mut self, key: &str, value: &str) {
        let close = self.close_index();
        let text = format!("{key}: {value}");
        let found = self.key_lines(close, key);
        if let Some((&first, rest)) = found.split_first() {
            self.front[first].text = text;
            for &i in rest.iter().rev() {
                self.front.remove(i);
            }
            return;
        }
        let at = match key::ORDER.iter().position(|k| *k == key) {
            None => close,
            Some(rank) => {
                let before = &key::ORDER[..rank];
                (1..close)
                    .rev()
                    .find(|&i| {
                        key_value(&self.front[i].text).is_some_and(|(k, _)| before.contains(&k))
                    })
                    .map_or(1, |i| i + 1)
            }
        };
        self.front.insert(
            at,
            Line {
                text,
                ending: self.newline,
            },
        );
    }

    /// Removes every line of `key` from the block. Returns whether there
    /// was one.
    pub(super) fn clear_key(&mut self, key: &str) -> bool {
        let close = self.close_index();
        let found = self.key_lines(close, key);
        for &i in found.iter().rev() {
            self.front.remove(i);
        }
        !found.is_empty()
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Index of the line that closes a block opened on line 0: the first
/// `---` after it.
fn closing_fence(lines: &[Line]) -> Option<usize> {
    lines
        .iter()
        .skip(1)
        .position(|l| l.text == FENCE)
        .map(|i| i + 1)
}

/// The title of a title line (`# Title`), or `None`. The title must be
/// non-empty: `#` alone is not a title line.
pub(super) fn v2_title_of(line: &str) -> Option<&str> {
    line.strip_prefix(TITLE_PREFIX).filter(|t| !t.is_empty())
}

/// The name of a level-2 heading line (`## Name`), or `None`.
pub(super) fn heading_name(line: &str) -> Option<&str> {
    line.strip_prefix("## ")
}

/// Splits `text` into lines with their endings; the second value says
/// whether the last line was terminated. An unterminated last line gets
/// `Lf`, so an empty text yields no lines.
fn split_lines(text: &str) -> (Vec<Line>, bool) {
    let mut lines = Vec::new();
    let mut rest = text;
    let mut trailing_newline = false;
    while !rest.is_empty() {
        if let Some(i) = rest.find('\n') {
            let (line, ending) = match rest[..i].strip_suffix('\r') {
                Some(line) => (line, Newline::CrLf),
                None => (&rest[..i], Newline::Lf),
            };
            lines.push(Line {
                text: line.to_owned(),
                ending,
            });
            rest = &rest[i + 1..];
            trailing_newline = true;
        } else {
            lines.push(Line {
                text: rest.to_owned(),
                ending: Newline::Lf,
            });
            rest = "";
            trailing_newline = false;
        }
    }
    (lines, trailing_newline)
}
