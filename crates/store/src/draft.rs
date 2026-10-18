//! What both stores do with a [`TaskDraft`] before writing it: the check
//! that refuses a draft the task file cannot hold faithfully, and the
//! defaults a draft gets when it becomes a task.

use tasq_core::clock::Clock;
use tasq_core::model::{Task, TaskDraft, TaskId};
use tasq_core::store::StoreError;

/// The progress note a draft without one gets. The script wrote
/// `created via tasks create`; the binary is `tasq`.
pub const DEFAULT_NOTE: &str = "created via tasq create";

/// Rejects a draft the store cannot write faithfully. Today: merge requests
/// without a label. The file format for a merge request is `- [title](url)`,
/// and the title is the CLI's to resolve (plan T-307); writing the url as
/// its own title would persist a placeholder the user would have to fix.
pub fn check_draft(draft: &TaskDraft) -> Result<(), StoreError> {
    if let Some(mr) = draft
        .merge_requests
        .iter()
        .find(|mr| mr.label.as_deref().is_none_or(str::is_empty))
    {
        return Err(StoreError::Unsupported {
            operation: format!(
                "creating a task with the merge request {} without a title (resolve the title first)",
                mr.url
            ),
        });
    }
    Ok(())
}

/// `draft` as the task with `id`: a draft without a note gets
/// [`DEFAULT_NOTE`], stamped at `clock.now()` like every note; a `done`
/// draft is closed at `clock.now()` with no status
/// ([`TaskDraft::into_task`]).
pub fn materialize(mut draft: TaskDraft, id: TaskId, clock: &dyn Clock) -> Task {
    if draft.note.is_none() {
        draft.note = Some(DEFAULT_NOTE.to_owned());
    }
    draft.into_task(id, clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::clock::FixedClock;
    use tasq_core::model::Link;

    #[test]
    fn drafts_need_labelled_merge_requests() {
        assert!(check_draft(&TaskDraft::new("t")).is_ok());
        let ok = TaskDraft::new("t").with_merge_request(Link::labelled("https://x/1", "One"));
        assert!(check_draft(&ok).is_ok());
        let bare = TaskDraft::new("t")
            .with_merge_request(Link::labelled("https://x/1", "One"))
            .with_merge_request(Link::new("https://x/2"));
        assert_eq!(
            check_draft(&bare).unwrap_err().to_string(),
            "unsupported: creating a task with the merge request https://x/2 without a title (resolve the title first)"
        );
        let empty = TaskDraft::new("t").with_merge_request(Link::labelled("https://x/3", ""));
        assert!(
            check_draft(&empty)
                .unwrap_err()
                .to_string()
                .contains("https://x/3 without a title")
        );
    }

    #[test]
    fn a_draft_without_a_note_gets_the_default_one() {
        let clock = FixedClock::at("2026-10-04 12:34");
        let task = materialize(TaskDraft::new("Plain"), TaskId::from(7), &clock);
        assert_eq!(task.id, TaskId::from(7));
        assert_eq!(task.progress.len(), 1);
        assert_eq!(task.progress[0].note, DEFAULT_NOTE);
        assert_eq!(task.progress[0].at, clock.0.into());
        let noted = materialize(
            TaskDraft::new("Noted").with_note("from the issue"),
            TaskId::from(8),
            &clock,
        );
        assert_eq!(noted.progress[0].note, "from the issue");
        assert_eq!(noted.progress.len(), 1);
    }
}
