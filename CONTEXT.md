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

**Task file**:
The markdown file that holds one task: a frontmatter block of scalar fields, then the title heading and the list sections. Its name is a ULID, from which the task's creation time is read; nothing else is read from it.
_Avoid_: Todo file, note

**Id**:
The short number written in a task's frontmatter that every command uses to name the task. Allocated once on create and never reused. Never the file name, never a position in an index.
_Avoid_: Number, line, index

**Frontmatter**:
The block of scalar fields (`key: value` lines between `---` lines) at the top of a task file: id, status, priority, rank, due, closed, project, source and tags.
_Avoid_: Header, metadata, YAML (it is a flat subset, not YAML)
