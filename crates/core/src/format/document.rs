//! The lossless layer: a task file as a line buffer with a structured view.
//!
//! [`Document`] keeps every byte of the file it was parsed from: the lines
//! (with their individual line endings), whether the file ended with a
//! newline, and the title line. Sections are not stored as separate blocks;
//! they are computed views over the lines ([`Document::sections`]), because
//! every edit the original script made was a line-level `awk` pass and the
//! operations in [`super::ops`] mirror those passes one line at a time.
//!
//! Only the first line is validated here (see [`FormatError::NotATask`]);
//! everything else is kept verbatim, so `render(parse(x)) == x` holds for
//! every string that parses.

use std::fmt;

use thiserror::Error;

/// Errors from parsing a task file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The first line is not `# [ ] Title` or `# [x] Title`.
    ///
    /// The original script lists only files whose first line starts with
    /// `# [ ] ` (and recognises `# [x] ` in reports), so anything else is
    /// some other markdown file in the notebook, not a task.
    #[error("not a task: first line {first_line:?} is not '# [ ] Title' or '# [x] Title'")]
    NotATask {
        /// The offending first line, without its line ending.
        first_line: String,
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

/// Prefix of an open task's title line.
pub const OPEN_PREFIX: &str = "# [ ] ";
/// Prefix of a done task's title line.
pub const DONE_PREFIX: &str = "# [x] ";

/// A task file, byte for byte.
///
/// Construct one with [`Document::parse`] (or [`super::parse`], which also
/// projects the typed [`crate::model::Task`]) and write it back with
/// [`Document::render`]. Edit it through the operations in [`super::ops`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
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
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.body.len() {
            if let Some(name) = self.body[i].strip_prefix("### ") {
                let start = i + 1;
                let mut end = start;
                while end < self.body.len() && !self.body[end].starts_with("##") {
                    end += 1;
                }
                out.push(Section {
                    level: 3,
                    name,
                    heading: self.heading + 1 + i,
                    body: self.body[start..end].to_vec(),
                });
                i = end;
            } else {
                i += 1;
            }
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

impl Document {
    /// Splits `text` into lines and checks the title line.
    pub fn parse(text: &str) -> Result<Self, FormatError> {
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
        let first_line = lines.first().map(|l| l.text.as_str()).unwrap_or_default();
        if title_of(first_line).is_none() {
            return Err(FormatError::NotATask {
                first_line: first_line.to_owned(),
            });
        }
        let newline = if trailing_newline || lines.len() > 1 {
            lines[0].ending
        } else {
            Newline::Lf
        };
        Ok(Self {
            lines,
            newline,
            trailing_newline,
        })
    }

    /// Writes the document back, byte for byte if it was not edited.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let last = self.lines.len().saturating_sub(1);
        for (i, line) in self.lines.iter().enumerate() {
            out.push_str(&line.text);
            if i != last || self.trailing_newline {
                out.push_str(line.ending.as_str());
            }
        }
        out
    }

    /// The ending used for lines this document creates.
    pub fn newline(&self) -> Newline {
        self.newline
    }

    /// Whether the last line is terminated.
    pub fn ends_with_newline(&self) -> bool {
        self.trailing_newline
    }

    /// The lines without their endings.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|l| l.text.as_str())
    }

    /// Number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The first line, without its ending.
    pub fn title_line(&self) -> &str {
        &self.lines[0].text
    }

    /// The title text after the `# [ ] ` / `# [x] ` marker.
    pub fn title(&self) -> &str {
        title_of(self.title_line()).map_or("", |(_, t)| t)
    }

    /// Whether the title line carries the done marker `# [x]`.
    pub fn is_done(&self) -> bool {
        title_of(self.title_line()).is_some_and(|(done, _)| done)
    }

    /// Every level-2 section, in file order. A section runs from its heading
    /// to the next line starting with `## ` (so `### ` lines belong to the
    /// body of the enclosing section).
    pub fn sections(&self) -> Vec<Section<'_>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            if let Some(name) = heading_name(&self.lines[i].text) {
                let end = self.section_end(i);
                out.push(Section {
                    level: 2,
                    name,
                    heading: i,
                    body: self.lines[i + 1..end]
                        .iter()
                        .map(|l| l.text.as_str())
                        .collect(),
                });
                i = end;
            } else {
                i += 1;
            }
        }
        out
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

    /// Marks every line as terminated, as any `awk` rewrite of the file does.
    pub(super) fn terminate(&mut self) {
        self.trailing_newline = true;
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Splits a title line into `(done, title)`, or `None` when it is not one.
///
/// The title must be non-empty: the script treated `# [ ]` alone as not a
/// task (its `sub` left an empty title and it bailed out).
pub(super) fn title_of(line: &str) -> Option<(bool, &str)> {
    let (done, title) = match line.strip_prefix(OPEN_PREFIX) {
        Some(t) => (false, t),
        None => (true, line.strip_prefix(DONE_PREFIX)?),
    };
    (!title.is_empty()).then_some((done, title))
}

/// The name of a level-2 heading line (`## Name`), or `None`.
pub(super) fn heading_name(line: &str) -> Option<&str> {
    line.strip_prefix("## ")
}
