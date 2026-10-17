# tasq

A todo manager over nb notebooks: tasks are markdown files, grouped by workflow status and worked on through launched sessions.

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
