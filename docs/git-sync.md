# Store sync

A store is a directory of task files with no history. Store sync makes it a git repository:
every command that writes is committed, pushed to a remote and pulled on your other machines,
in the background, so no command and not `tasq ui` waits for git. It is the `tasq-git`
plugin (ADR 0030), opt-in, and a store that is not a repository behaves exactly as without it.

Commands, run in the store tasq resolves (`--profile`, `--set store.name=...` and the
environment apply, as for any command):

| Command | What it does |
|---|---|
| `tasq git init [<url>] [--branch <b>] [--name <n>] [--email <e>] [--ssh-key <path>]` | Make the store a repository, commit its task files, push them to `<url>` if given |
| `tasq git clone <url> [--branch <b>] [--store <name>] [--name <n>] [--email <e>] [--ssh-key <path>]` | Fill an empty store from a branch of `<url>` |
| `tasq git sync` | Commit, fetch, rebase and push now, printing each step |

## Install and enable

`tasq-git` ships next to `tasq`: in the release tarballs, the Homebrew formula, and
`scripts/install`. From source: `cargo install --path crates/git`. It must be on `PATH`
(`tasq doctor` says where it found it).

Enable it in `~/.config/tasq/config.toml`, with the identity your stores commit as:

```toml
[plugins]
enabled = ["git"]

[plugin.git]
name = "Me Personal"            # user.name of every store repository init or clone makes
email = "me@example.com"        # user.email
ssh_key = "~/.ssh/id_personal"  # the key pushes and fetches use (optional)
interval = 60                   # seconds between background fetches (default 60)
```

All four keys are optional. Enabling the plugin changes nothing for a store until it is a
repository: its events return at once and `tasq doctor` shows `OK git/sync store sync off:
<store> is not a git repository`.

## The first machine

Create an empty repository on your git host (no README, no commits), then:

```
$ tasq git init git@github.com:me/tasks.git
initialized /home/me/.local/share/tasq/stores/default as a git repository on branch default: 12 task files committed
pushed branch default to git@github.com:me/tasks.git
```

- The first commit, `tasq git init`, holds exactly the task files: the store's top-level `.md`
  files. `.tasq/` (the lock, plugin state) and anything else in the directory are never
  committed. `init` adds `.tasq/` to the repository's `.git/info/exclude` (not a committed
  `.gitignore`), so `git status` stays quiet and a manual `git add -A` leaves it out.
- The branch is named after the store (`store.name`, `default` unless configured), so several
  stores share one remote repository, one branch each: `tasq --set store.name=work git init
  <url>` pushes the `work` store to branch `work`. `--branch` picks another name.
- A branch that already exists on the remote is refused before anything is written: two task
  histories are never merged. Use `tasq git clone` for a store that is already there, or
  `--branch` for a new one.
- Without a URL, the store is a local repository: every write is committed, nothing is
  pushed, `tasq doctor` says `branch default has no upstream: commits stay local`. To add a
  remote later, use git itself: `git -C <store> remote add origin <url> && git -C <store> push
  -u origin <branch>`.
- A store that is already a repository is refused. Any failure after `git init` removes the
  new `.git`, so the command can simply be run again.

## A second machine

1. Install `tasq` and `tasq-git` and copy the `[plugins]` and `[plugin.git]` tables above into
   its `~/.config/tasq/config.toml`. The SSH key must be on that machine too. `clone` runs in
   the terminal and can ask for a passphrase or to trust the host key; the background job
   never can.
2. Clone the store's branch:

   ```
   $ tasq git clone git@github.com:me/tasks.git
   cloned branch default of git@github.com:me/tasks.git into /home/me/.local/share/tasq/stores/default: 12 task files
   ```

   The branch defaults to the store name, so the `default` store clones branch `default`.
   `--store work` fills the store `work` (and clones branch `work`); `--branch` names the
   branch when it differs from the store name.
3. `tasq list` shows the tasks. From now on, writes on either machine reach the other.

`clone` refuses, before touching anything, a store that already has task files, a branch the
remote does not have and a remote it cannot reach. Other files in the store directory
(`.tasq/`) are kept, and `.tasq/` goes into `.git/info/exclude` as with `init`. To join a store you already have locally, move its task files away first,
clone, then recreate the ones that were not on the remote.

## Identity

`--name`, `--email` and `--ssh-key` (each defaulting from `[plugin.git]`) are written into the
store repository's own config by `init` and `clone`:

```
user.name = Me Personal
user.email = me@example.com
core.sshCommand = ssh -i '~/.ssh/id_personal' -o IdentitiesOnly=yes
```

So the store commits as that identity and pushes with that key whatever directory or project
a command runs in, and even when your SSH agent holds another key (`IdentitiesOnly` offers
only this one). `git` run by hand in the store directory uses the same config. Without any of
the three, git's own resolution applies (your global config); `tasq doctor` shows what it
resolved to, and `git -C <store> config user.email ...` changes it later.

The background job runs git with a cleared environment: `PATH`, `HOME`, `XDG_CONFIG_HOME`
(where git finds your global config), `SSH_AUTH_SOCK`, `XDG_RUNTIME_DIR` and
`DBUS_SESSION_BUS_ADDRESS` (for a credential helper that talks to your keyring), the proxy
variables git reads (`https_proxy`, `no_proxy`, ...) and its own settings. Exported `GIT_*`
variables (a direnv `GIT_AUTHOR_EMAIL`, a `GIT_DIR`, a `GIT_SSH_COMMAND`) never reach it.
`tasq git init`, `clone` and `sync` drop every `GIT_*` variable too. A git command you type
yourself does not: unset them first if your shell sets them.

Every git the plugin runs, in the background or not, does so without commit signing and
without hooks (`commit.gpgSign=false`, `core.hooksPath=/dev/null`, passed as
`GIT_CONFIG_COUNT` variables), whatever your global config says: a background job has no
terminal for a pinentry, and a global `pre-commit` or `pre-push` hook is about code, not task
files. This is not written into the store's config, so a commit you make by hand in the store
directory is signed and hooked as usual.

## What happens in the background

After every command that wrote (and every writing action in `tasq ui`), `tasq-git` starts a
detached job and returns at once. The job:

1. commits the task files, one commit for everything written since its last commit. The
   subject is the command, the ids and the first task's title: `log 57: Git sync as an opt-in
   feature for tasq stores`. When several writes are batched, the body lists each `<command>
   <ids>`. A commit with no tasq write behind it (a hand edit) is `update task files`;
2. if the branch has an upstream: fetches, rebases onto the upstream when it is behind, and
   pushes when it is ahead. A failed push is fetched, rebased and pushed once more, so two
   machines writing in turn need no manual step. The rebase holds the store lock (`tasq store
   lock`), so it never races a tasq write; a write that lands meanwhile waits for it;
3. goes round again if anything was written while it ran, then exits.

One job runs per store at a time: it holds an OS file lock on `.tasq/plugins/git/job.lock`,
which goes with it, so a job that died blocks nothing and one waiting on a slow remote is
never taken for dead. The job never prompts: a passphrase, an unknown host key or an HTTPS
password fails it at once with a lasting warning. Use an agent-loaded or passphrase-less key,
or run `tasq git sync`, which can prompt. A git command of the job that runs for two minutes
(a fetch or push hanging on an unreachable host) is killed and its step fails (`fetch failed:
git timed out after 120 s`); `tasq git sync` waits as long as git does.

### The interval

A machine you only read on must still pull. When any command opens the store (a read
included, and `tasq ui` at start and on `r`) and the last fetch is older than `[plugin.git]
interval` seconds (default 60), the job is started to fetch and rebase. A command that wrote
always fetches and pushes, whatever the interval. Set it per profile or per command like any
key (`--set plugin.git.interval=600`); `0` fetches whenever the store opens. A value that is
not a whole number of seconds is a warning on every command, and the default applies
meanwhile: writes are still committed and pushed.

`tasq ui` reloads by itself when the job pulls changes in (it checks the files every 2 s), so a
task edited on another machine shows without a keypress.

## Syncing by hand

`tasq git sync` runs the job's steps in the terminal, fetches whatever the interval, and prints
one line per step:

```
$ tasq git sync
committed: log 3: Answer the support ticket
fetched origin/default
pulled 1 commit from origin/default
pushed 1 commit to origin/default
```

```
$ tasq git sync
nothing to commit
fetched origin/default
up to date with origin/default
```

A failed step prints the lasting warning it leaves (below) and exits 1. git keeps the terminal,
so it can ask for a passphrase. A background job that is running is waited for first.

## When something fails

Problems are lasting warnings: shown on stderr by every command that opens the store, in the
status bar of `tasq ui` and as `WARN git/warning` in `tasq doctor`, until the job or `tasq git
sync` finds them fixed:

```
tasq: warning: git: push failed: ssh: Could not resolve hostname github.com: Temporary failure in name resolution
```

| Warning | Means | Clears |
|---|---|---|
| `commit failed: ...` | git could not commit (git's message follows) | on the next commit that works; the writes wait in `.tasq/plugins/git/writes` |
| `fetch failed: ...` | the remote was unreachable (offline, wrong URL, no key) | on the next fetch that works |
| `push failed: ...` | the push failed twice | on the next push, or when nothing is left to push |
| `rebase failed: ...` | the rebase failed without a conflict; it was undone, local commits kept | on the next rebase that works |
| `rebase conflict on task <id>: ...` | both machines changed the same task (below) | once resolved |
| `duplicate id: ...` | a pull brought in a task with an id this store already used (below) | once fixed by hand |

Being offline is not an error to act on: commits keep piling up locally, the warning says so,
and the first job that reaches the remote pushes them all and clears it.

### Conflicts

When both machines change the same task before either has pulled, the second rebase cannot
apply. The job aborts it: your local commits are kept, the task files stay as they were before
the rebase (never with conflict markers), and nothing is pushed until it is resolved:

```
tasq: warning: git: rebase conflict on task 2: local commits kept, nothing pushed until it is resolved: cd /home/me/.local/share/tasq/stores/default && git pull --rebase, fix the files, git add them, git rebase --continue
```

Resolve it with git in the store directory, as the warning says:

```sh
cd ~/.local/share/tasq/stores/default
git pull --rebase           # stops on the conflicting task file
$EDITOR 01K....md           # keep both sides' lines, drop the <<<<<<< ======= >>>>>>> markers
git add 01K....md
git rebase --continue
tasq git sync               # pushes and clears the warning (the next background job would too)
```

While your rebase is in progress the job leaves the store alone: it commits nothing (the writes
wait), fetches nothing, and `tasq git sync` refuses to run. `tasq doctor` shows `FAIL
git/rebase a rebase is in progress` until you finish with `git rebase --continue` or `git
rebase --abort`. A task file with markers in it fails to parse, so finish the rebase before
using tasq on that task.

### Duplicate ids

A new task takes the highest id in the store plus one. Two machines that create a task while
apart give both the same id; once one pulls the other's task, the id is claimed twice. tasq
never renumbers a task (worktree branches, merge request links and notes carry the id), so the
job reports it instead:

```
tasq: warning: git: duplicate id: id 3 is claimed by more than one file: /home/me/.local/share/tasq/stores/default/01K8G0RZ5M7Q2W9V3XKJ4T6B1C.md (created 2026-10-27 14:42, "Made on A"), /home/me/.local/share/tasq/stores/default/01K8G0S1A2B3C4D5E6F7G8H9J0.md (created 2026-10-27 14:42, "Made on B", newer); fix: give the newer file the next free id: write 'id: 4' in /home/me/.local/share/tasq/stores/default/01K8G0S1A2B3C4D5E6F7G8H9J0.md (or remove it if it is a copy)
```

Meanwhile `list` shows both tasks with a warning, and `tasq view 3` refuses the id. Fix it on one
machine by editing the `id:` line of the newer file as the warning says, then `tasq git sync`
(or any write): the edit is committed as `update task files`, pushed, and the warning clears on
each machine once it has pulled the fix. `tasq doctor` gives the same text as a `FAIL ids`
check, with or without the plugin.

## `tasq doctor`

For a store that is a repository, the plugin adds:

```
OK    git/identity commits as Me Personal <me@example.com>, pushes with ssh -i '~/.ssh/id_personal' -o IdentitiesOnly=yes
OK    git/exclude  .tasq/ is ignored
OK    git/remote   branch default tracks origin/default at git@github.com:me/tasks.git
WARN  git/push     2 commits not pushed
                   fix: tasq git sync
OK    git/fetch    last fetch 4 min ago
OK    git/rebase   no rebase in progress
```

| Check | Shows |
|---|---|
| `git/identity` | who the store commits as and the `ssh` command it pushes with; `FAIL` when git cannot tell, with the `git config` commands to set it |
| `git/exclude` | whether git ignores `.tasq/`; `WARN` when not (a store set up before `init` and `clone` excluded it), with the line to add to `.git/info/exclude` |
| `git/remote` | the branch, its upstream and the remote URL, or `has no upstream: commits stay local` |
| `git/push` | commits not pushed yet, counted against the last fetch (doctor never asks the remote) |
| `git/fetch` | how long ago the last fetch (or attempt) was; `WARN never fetched` |
| `git/rebase` | `FAIL` while a rebase is in progress, with the commands to finish it |
| `git/warning` | each lasting warning |

## Files

The plugin keeps its state in the store, under `.tasq/plugins/git/`, never committed:

| File | |
|---|---|
| `job.lock` | locked by the running job (an OS file lock, released when it exits); the file stays |
| `pending` | a write or store open that came while the job ran; the job goes round again |
| `writes` | one `<command> <ids>` line per write not committed yet, for the commit message |
| `last-fetch` | when the remote was last asked (Unix seconds) |
| `warnings` | the lasting warnings, one per line |

## Turning it off

Remove `git` from `[plugins] enabled`: nothing is committed or pulled any more, and the store
stays a repository. Enabling it again, the next write commits everything changed meanwhile. To stop syncing one store only, delete its `.git` directory: the plugin then
leaves it alone (`store sync off`).
