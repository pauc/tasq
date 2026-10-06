//! The status-bar prompts' recall (`Up`/`Down`): what was submitted
//! earlier in the session, like a shell's history. In memory only;
//! nothing is written to disk.
//!
//! The model keeps three, in [`Histories`]: filters, notes (`log` and
//! `done` share one, both are progress notes) and titles. Recalling a
//! filter while writing a note would be noise.

/// How many entries a history keeps; the oldest go first.
pub const CAP: usize = 100;

/// The three histories of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histories {
    /// Filters applied with `Enter` (`/`).
    pub filters: History,
    /// Notes written with `l`, and the non-empty final notes of `d`.
    pub notes: History,
    /// Titles of tasks created with `c`.
    pub titles: History,
}

/// The entries of one prompt kind, and where `Up`/`Down` are in them
/// while that prompt is open.
///
/// Navigation works like a shell's: [`History::up`] shows the previous
/// (older) entry, [`History::down`] the next (newer) one, and going down
/// past the newest brings back the draft: what the prompt held before the
/// first `Up`. An entry edited after it was recalled is not kept: moving
/// away from it shows the neighbour, and coming back shows the entry as
/// it was recorded. Only the draft survives navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// Oldest first, at most [`CAP`].
    entries: Vec<String>,
    /// The entry shown, or `None` for the draft.
    shown: Option<usize>,
    /// The prompt's text before the first `Up`.
    draft: String,
}

impl History {
    /// The entries, oldest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Records a submitted `text`, trimmed. Empty text and a repeat of
    /// the newest entry are not recorded; past [`CAP`] the oldest entry
    /// goes.
    pub fn record(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.entries.last().is_some_and(|last| last == text) {
            return;
        }
        if self.entries.len() == CAP {
            self.entries.remove(0);
        }
        self.entries.push(text.to_owned());
    }

    /// Back to the draft, for a prompt that just opened.
    pub fn reset(&mut self) {
        self.shown = None;
        self.draft.clear();
    }

    /// The entry before the one shown, or the newest when the draft is
    /// shown (`current` is then saved as the draft). `None` at the
    /// oldest entry or with no entries: nothing changes.
    pub fn up(&mut self, current: &str) -> Option<String> {
        let index = match self.shown {
            None => {
                let newest = self.entries.len().checked_sub(1)?;
                current.clone_into(&mut self.draft);
                newest
            }
            Some(index) => index.checked_sub(1)?,
        };
        self.shown = Some(index);
        Some(self.entries[index].clone())
    }

    /// The entry after the one shown, or the draft after the newest.
    /// `None` when the draft is shown: nothing changes.
    pub fn down(&mut self) -> Option<String> {
        let index = self.shown?;
        if index + 1 == self.entries.len() {
            self.shown = None;
            return Some(self.draft.clone());
        }
        self.shown = Some(index + 1);
        Some(self.entries[index + 1].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(entries: &[&str]) -> History {
        let mut h = History::default();
        for entry in entries {
            h.record(entry);
        }
        h
    }

    #[test]
    fn record_trims_and_skips_empty_and_repeated_entries() {
        let mut h = History::default();
        h.record("  one ");
        h.record("");
        h.record("   ");
        h.record("one");
        h.record(" one");
        h.record("two");
        h.record("one");
        assert_eq!(h.entries(), ["one", "two", "one"]);
    }

    #[test]
    fn record_keeps_the_newest_cap_entries() {
        let mut h = History::default();
        for n in 0..=CAP {
            h.record(&n.to_string());
        }
        assert_eq!(h.entries().len(), CAP);
        assert_eq!(h.entries()[0], "1");
        assert_eq!(h.entries()[CAP - 1], CAP.to_string());
        let mut h = History::default();
        for n in 0..CAP {
            h.record(&n.to_string());
        }
        assert_eq!(h.entries().len(), CAP);
        assert_eq!(h.entries()[0], "0", "nothing dropped at exactly CAP");
    }

    #[test]
    fn an_empty_history_recalls_nothing() {
        let mut h = History::default();
        assert_eq!(h.up("draft"), None);
        assert_eq!(h.down(), None);
        assert_eq!(h, History::default(), "the draft is not saved either");
    }

    #[test]
    fn up_goes_back_and_down_comes_forward_to_the_draft() {
        let mut h = history(&["one", "two", "three"]);
        assert_eq!(h.down(), None, "already at the draft");
        assert_eq!(h.up("dra").as_deref(), Some("three"));
        assert_eq!(h.up("ignored").as_deref(), Some("two"));
        assert_eq!(h.up("ignored").as_deref(), Some("one"));
        assert_eq!(h.up("ignored"), None, "at the oldest");
        assert_eq!(h.down().as_deref(), Some("two"));
        assert_eq!(h.down().as_deref(), Some("three"));
        assert_eq!(h.down().as_deref(), Some("dra"));
        assert_eq!(h.down(), None, "at the draft");
        assert_eq!(h.up("new draft").as_deref(), Some("three"));
        assert_eq!(h.down().as_deref(), Some("new draft"));
    }

    #[test]
    fn edits_of_a_recalled_entry_are_dropped_and_the_draft_kept() {
        let mut h = history(&["one", "two"]);
        assert_eq!(h.up("draft").as_deref(), Some("two"));
        // The prompt now holds an edited "two"; `up` ignores it.
        assert_eq!(h.up("two, edited").as_deref(), Some("one"));
        assert_eq!(h.down().as_deref(), Some("two"));
        assert_eq!(h.down().as_deref(), Some("draft"));
        assert_eq!(h.entries(), ["one", "two"]);
    }

    #[test]
    fn reset_goes_back_to_an_empty_draft() {
        let mut h = history(&["one", "two"]);
        assert_eq!(h.up("draft").as_deref(), Some("two"));
        h.reset();
        assert_eq!(h.down(), None);
        assert_eq!(h.up("").as_deref(), Some("two"), "from the newest again");
        h.reset();
        assert_eq!(h, history(&["one", "two"]));
    }
}
