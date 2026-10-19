# ADR-0022: Worktrees as an opt-in core feature

- **Status:** Proposed
- **Date:** 2026-10-07

## Context

Tasks track git worktrees (`## Worktrees`), sessions start in the first one that exists, and
`tasq worktree <id> --create <branch>` makes one through `work.worktree_manager` (`git`, or a
`command` template whose only real user is `gwm`). Provisioning a new worktree (symlinks, copies,
a rendered `.envrc`, `direnv allow`, per-branch databases) lives in `gwm`, a separate zsh script
with its own `gwm.yml` per workspace. The author wants `tasq` to replace `gwm` completely and to
do better than it (nb todo 44, `ruli/features/worktrees/PLAN.md`).

Forces:

- Users who do not use worktrees should not see them anywhere: help, JSON, TUI, Claude prompt,
  task files.
- The worktree is tied to the task in several places that live in the binary: `resolve_workdir`
  in `next`/`pick`, recreating a missing worktree before a launch, the Claude prompt, the TUI
  detail pane, `tasq done`.
- External plugins (ADR 0006) cannot add a built-in command's name while the built-in exists,
  cannot pass data to `tasq create`, and cannot store fields on a task (nb todo 46).
- The core has no I/O (`docs/architecture.md`); the provisioning logic must still be cheap to mutation-test.

## Decision

Worktrees are a core feature behind `worktree.enabled` (default `false`).

- **Off** hides every surface: the `tasq worktree` commands and `tasq create --worktree` are
  hidden from help and refused with the key to set, task JSON leaves out `"worktrees"`, the TUI
  never shows the section, the default Claude prompt drops its worktree line, and
  `resolve_workdir` ignores tracked worktrees. Files are never rewritten because of the toggle;
  an existing `## Worktrees` section is preserved byte for byte and used again when re-enabled.
- **On** provides the command family of the plan (`tasq worktree set <id> [BRANCH]` to declare, `up`,
  `release`, `ls`, `prune`, `switch`, `path`, `shell-init`, `trust`) and a provisioning model like gwm's (symlinks, copies,
  templates, variables, hooks) configured under `[worktree]` in the existing layered config.
  Worktree commands load that config from the task's project directory, so a `.tasq.toml` next
  to a repo or in its workspace directory plays the role of `gwm.yml`.
- A `## Worktrees` entry is a declaration, never removed by tasq, and the file format does not
  change. Declaring (`tasq create --worktree [BRANCH]`, or automatically from the `{{id}}-{{slug}}`
  template) writes the path and branch only. Launching the task creates the worktree when it does
  not exist and provisions it. Closing the task removes the worktree when it is clean and deletes
  its branch when that loses nothing (integrated into the default branch, or pushed); dirty work
  and unsafe branches are kept with a warning. Launching again recreates it from the kept branch,
  the remote branch, or the base.
- `tasq-core::worktree` holds the pure part: templates and variables, naming, path resolution,
  the provisioning plan (a list of steps), merged-check decisions and prune selection. A new
  `tasq-worktree` crate executes plans (git, filesystem, hooks) and replaces
  `tasq_launch::worktree`.
- gwm stops being a dependency in any form. `work.worktree_manager`, `work.worktree_command`
  and the `CommandManager` are removed (a config that sets them fails with a pointer to
  `[worktree]` in `docs/config.md`); there is no `gwm.yml` reader or importer; no test, example
  or current doc mentions gwm, and a CI grep keeps it that way. Existing workspaces are moved to
  `.tasq.toml` by hand.
- Hooks declared in a `.tasq.toml` run only after the user approves their exact command text;
  approvals live in `~/.local/state/tasq/trust.toml`. Hooks in the global config are trusted.

## Consequences

### Positive

- One tool for tasks and their worktrees; names, listing and cleanup use task data (id, title,
  status, close events), which no standalone worktree manager has.
- Users without worktrees get a smaller CLI and cleaner JSON than today.
- Provisioning plans are pure data, testable without git, in line with the mutation-testing bar.
- Repo-provided hooks are no longer run blindly (gwm runs whatever `gwm.yml` says).

### Negative

- More code in the main binary and a seventh crate. Mitigated by the core/executor split and by
  keeping the feature off by default.
- `docs/json.md` changes: `"worktrees"` becomes conditional. `tasq apply` keeps accepting
  documents with or without it, and an absent key never drops stored worktrees.
- Removing `work.worktree_*` breaks configs that set them; the error names the replacement.
- Existing sessions call `tasq worktree <id> <path>` and `--create` (a hidden alias of
  declare-then-`up` for one release). Both moved under `tasq worktree set` on 2026-10-19: a
  positional `<ID>` next to the subcommands broke shell completion of the subcommand names.

## Alternatives considered

- **External `tasq-worktree` plugin (ADR 0006).** Keeps the binary small and the feature
  invisible by default, but needs three new plugin capabilities first (retiring the built-in
  name, passing arguments from `tasq create` to `post-create`, plugin-owned task fields) and
  still could not affect `resolve_workdir`, the prompt or the TUI without more hooks. Rejected
  for this feature; plugin task fields are pursued separately (nb todo 46).
- **Keep delegating to gwm through `work.worktree_command`.** No improvement possible on cleanup,
  listing or naming, and a second config file per workspace. Rejected: replacing gwm is the goal.
- **Keep the `command` manager as a generic escape hatch, or ship an `import-gwm` converter.**
  Either keeps gwm (or its config format) as something tasq supports. Rejected: the author wants
  gwm gone; three config files are converted once by hand.
- **Create the worktree at task creation and untrack it on removal.** Creates directories for
  tasks that may never be worked on, and loses the declaration when the worktree is cleaned up,
  so relaunching a closed or reopened task cannot recreate it. Rejected for the declare /
  materialize-at-launch / release-at-close lifecycle.
- **A separate config file (`.tasq-worktree.toml`).** A second loader, a second error format.
  Rejected in favour of a `[worktree]` table in the existing layers.

## References

- `ruli/features/worktrees/PLAN.md` (tasks WT-101 to WT-304, prior-art survey)
- ADR 0003 (core traits), ADR 0006 (plugin mechanism), ADR 0009 (TUI acts through the CLI)
- `crates/core/src/work.rs`, `crates/launch/src/worktree.rs`, `crates/cli/src/commands/worktree.rs`
