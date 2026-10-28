# tasq

A todo manager over a directory of markdown task files, grouped by workflow status and worked on through launched sessions.

## Language

**Status group**:
The open tasks that share one workflow status, plus one trailing group for open tasks with no status.
_Avoid_: Bucket, column, list

**Rank**:
A task's manually set place among the tasks of its status group, as a number where the smallest comes first. Ranked tasks come before unranked ones; unranked tasks keep the natural order of priority, due date, id. A task keeps its rank when its status changes or it is closed.
_Avoid_: Order, position, sort index

**Move**:
Setting a task's rank so that it sits before or after another task, or at the top, of its status group.
_Avoid_: Reorder, drag

**Store**:
A directory owned by tasq that holds one task file per task. A store has a name; its directory is the name under the data directory.
_Avoid_: Repository, database

**Sync**:
Refreshing tasks from their sources: creating, updating and closing tasks for the merge requests, issues and inbox items the sources report.
_Avoid_: Sync when a store's history is meant (that is store sync)

**Store sync**:
Exchanging a store's history with a remote copy of it, so that every machine sees the same tasks. Opt-in; a store without it has no history at all.
_Avoid_: Sync, pull, push, replication

**Plugin**:
A separate program, enabled by name, that adds commands to tasq and is told about its events. Store sync is a plugin.
_Avoid_: Extension, add-on, integration

**Event**:
A moment in a tasq command that plugins and hooks are told about, such as the store being opened or a command having written tasks.
_Avoid_: Callback, trigger

**Hook**:
A command line configured by the user to run on one event.
_Avoid_: Plugin (a plugin is enabled, not configured per event)

**Config directory**:
The one directory tasq reads configuration from: the global config file and every profile. Nothing is read from the directory tasq runs in or from a repository.
_Avoid_: Project config, dotfile

**Profile**:
A named partial configuration, one file in the config directory, applied over the global config only when selected by name.
_Avoid_: Environment, context, workspace

**Repository config**:
Worktree settings for one repository, one file in the config directory naming that repository's main checkout. Applied whenever tasq works on that repository's worktrees, whichever profile is selected.
_Avoid_: Repo file, project config, `.tasq.toml`

**Main checkout**:
The working tree of a repository that holds its `.git` directory, as opposed to the worktrees added from it.
_Avoid_: Main worktree, original, repo root

**Task file**:
The markdown file that holds one task: a frontmatter block of scalar fields, then the title heading and the list sections. Its name is a ULID, from which the task's creation time is read; nothing else is read from it.
_Avoid_: Todo file, note

**Id**:
The short number written in a task's frontmatter that every command uses to name the task. Allocated once on create and never reused. Never the file name, never a position in an index.
_Avoid_: Number, line, index

**Frontmatter**:
The block of scalar fields (`key: value` lines between `---` lines) at the top of a task file: id, status, priority, rank, due, closed, project, source and tags.
_Avoid_: Header, metadata, YAML (it is a flat subset, not YAML)
