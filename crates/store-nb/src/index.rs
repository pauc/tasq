//! nb's `.index`: one filename per line, id = line number (1-based).

use tasq_core::model::TaskId;

/// Suffix of a todo file in an nb notebook.
pub const TODO_SUFFIX: &str = ".todo.md";

/// The parsed index of a notebook.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    entries: Vec<String>,
}

impl Index {
    /// Splits the file into entries, one per line; `\r\n` endings are
    /// tolerated. A trailing newline does not add an empty entry, but an
    /// empty line in the middle does keep its line number (nb leaves such
    /// lines when a file is deleted).
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect();
        Self { entries }
    }

    /// `(id, filename)` for every line, including non-todo and empty ones.
    pub fn entries(&self) -> impl Iterator<Item = (TaskId, &str)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, name)| (TaskId::from(i as u64 + 1), name.as_str()))
    }

    /// The filename on line `id`, when `id` is a line number of this index
    /// and the line is not empty.
    pub fn file_for(&self, id: &TaskId) -> Option<&str> {
        let line: usize = id.as_str().parse().ok()?;
        let name = self.entries.get(line.checked_sub(1)?)?;
        (!name.is_empty()).then_some(name.as_str())
    }

    /// Number of lines.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index has no lines.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Whether `name` is a todo file (`*.todo.md`), as `all_open` checked.
pub fn is_todo(name: &str) -> bool {
    name.len() > TODO_SUFFIX.len() && name.ends_with(TODO_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_line_numbers() {
        let index = Index::parse("a.todo.md\nnotes.md\n\nb.todo.md\n");
        let entries: Vec<(String, &str)> = index
            .entries()
            .map(|(id, name)| (id.to_string(), name))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("1".to_owned(), "a.todo.md"),
                ("2".to_owned(), "notes.md"),
                ("3".to_owned(), ""),
                ("4".to_owned(), "b.todo.md"),
            ]
        );
        assert_eq!(index.len(), 4);
        assert!(!index.is_empty());
    }

    #[test]
    fn file_for_resolves_numeric_ids_only() {
        let index = Index::parse("a.todo.md\r\n\nb.todo.md");
        assert_eq!(index.file_for(&TaskId::from(1)), Some("a.todo.md"));
        assert_eq!(index.file_for(&TaskId::from(2)), None, "empty line");
        assert_eq!(index.file_for(&TaskId::from(3)), Some("b.todo.md"));
        assert_eq!(index.file_for(&TaskId::from(4)), None);
        assert_eq!(index.file_for(&TaskId::from(0)), None);
        assert_eq!(index.file_for(&TaskId::new("x").unwrap()), None);
        assert_eq!(index.file_for(&TaskId::new("-1").unwrap()), None);
    }

    #[test]
    fn empty_index() {
        let index = Index::parse("");
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert_eq!(index.entries().count(), 0);
        assert_eq!(index, Index::default());
    }

    #[test]
    fn todo_files_end_with_the_suffix() {
        assert!(is_todo("20260901090000.todo.md"));
        assert!(is_todo("x.todo.md"));
        assert!(!is_todo(".todo.md"), "a bare suffix is not a file name");
        assert!(!is_todo("notes.md"));
        assert!(!is_todo("todo.md"));
        assert!(!is_todo(""));
    }
}
