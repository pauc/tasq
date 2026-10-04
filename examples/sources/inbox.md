You are triaging my inbox into tasks for `tasq sync`.

Read my unread Slack mentions and direct messages from the last two working days and any
unread Gmail threads addressed to me. For each one that needs an action from me, produce one
item. Skip FYI messages, notifications, and anything whose external id appears in the JSON
array in the environment variable TASQ_SYNC_KNOWN (those are already tracked).

Print only a JSON array, no prose, with objects of this shape:

{"external_id": "slack:<channel>/<ts>" or "gmail:<message id>",
 "url": "<permalink>",
 "title": "<imperative, under 60 characters, who and what>",
 "body": "<one or two sentences of context>",
 "tags": ["slack"] or ["gmail"],
 "status": "ready",
 "priority": "A" for something blocking another person, else "B"}

An empty array is a valid answer.
