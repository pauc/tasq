Below are the raw progress notes from my task tracker for {{day}}, one top-level bullet per task (todo id in brackets, "(done)" marks completed todos). Rewrite them as a standup update I can read aloud or paste in Slack.

Split the bullets into two groups:
- First, with no heading, the tasks where I spent significant time or did relevant work that day: investigation, implementation, substantive reviews, production fixes. Judge this from the amount and depth of that day's notes, not from the task's importance.
- Then, under a single line "Also:", the small stuff that took almost no time — answering a question, a routine rebase, chasing or waiting on a review, a status check that found nothing new. One half-line bullet each, no sub-bullets. Omit the "Also:" line entirely if there is nothing small.

Rules:
- One bullet per task, leading with where it stands (merged, in review, waiting on X, blocked, done, investigating).
- Very short: aim for under 15 words per bullet; telegraphic style is fine. Drop implementation details, file paths, spec and mutation-test counts, commit shas, worktree/tooling notes, and step-by-step narration.
- Refer to MRs/issues by short reference (!1234, #1234), as a markdown link whenever the same reference appears linked anywhere in the notes — copy that URL verbatim. Never invent a URL: references that only ever appear without a link (e.g. "toolkit !438", "infra !5569") MUST stay plain text, since these live in different repos and a guessed URL points at the wrong MR.
- Drop the todo ids in brackets.
- Use sub-bullets only when a task had clearly separate pieces of work that each matter — still one short line each.
- Skip tasks with no real activity that day (notes like "created via tasks create" alone).
- Output plain markdown: no other headers, no preamble, no closing line — just the two groups of bullets.

The notes:

{{notes}}
