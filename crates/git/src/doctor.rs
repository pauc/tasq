//! The plugin's `doctor` checks for a store that is a repository: who it
//! commits as, its remote and branch, the commits not pushed yet, the last
//! fetch and a rebase left in progress. All of them read the repository
//! and the plugin's state; none asks the remote.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::git::Git;
use crate::hook::Check;
use crate::job::{commits, rebasing};
use crate::state::State;

/// What to run when commits wait or no fetch was made.
const SYNC: &str = "tasq git sync";

/// The checks of the repository at `store` at `now`. While a rebase is
/// stopped, only the identity and the rebase: the branch is detached.
pub fn checks(git: &Git, store: &Path, now: SystemTime) -> Vec<Check> {
    let mut out = vec![identity(git, store)];
    if rebasing(git, store) {
        let at = store.display();
        out.push(Check {
            name: "rebase",
            status: "fail",
            message: format!("a rebase is in progress in {at}"),
            fix: Some(format!(
                "cd {at}, fix the files, git add them, git rebase --continue (or git rebase \
                 --abort)"
            )),
        });
        return out;
    }
    let upstream = git
        .check(store, &["rev-parse", "--abbrev-ref", "@{upstream}"])
        .ok();
    let branch = git
        .check(store, &["branch", "--show-current"])
        .unwrap_or_default();
    let branch = branch.trim();
    match upstream.as_deref().map(str::trim) {
        Some(upstream) => {
            let url = git
                .check(store, &["ls-remote", "--get-url"])
                .unwrap_or_default();
            out.push(ok(
                "remote",
                format!("branch {branch} tracks {upstream} at {}", url.trim()),
            ));
            out.push(push(git, store));
            out.push(fetch(&State::of(store), now));
        }
        None => out.push(ok(
            "remote",
            format!("branch {branch} has no upstream: commits stay local"),
        )),
    }
    out.push(ok("rebase", "no rebase in progress".to_owned()));
    out
}

fn ok(name: &'static str, message: String) -> Check {
    Check {
        name,
        status: "ok",
        message,
        fix: None,
    }
}

/// The name and email git resolves for a commit, and the `ssh` command
/// the store pushes with, if it has its own.
fn identity(git: &Git, store: &Path) -> Check {
    let ident = match git.check(store, &["var", "GIT_COMMITTER_IDENT"]) {
        Ok(ident) => ident,
        Err(err) => {
            let at = store.display();
            // git's advice above it names `--global`; the fix below does not.
            let last = err.message.lines().last().unwrap_or_default().trim();
            return Check {
                name: "identity",
                status: "fail",
                message: format!("git cannot tell who commits: {last}"),
                fix: Some(format!(
                    "git -C {at} config user.name '<name>' && git -C {at} config user.email \
                     '<email>'"
                )),
            };
        }
    };
    // `Name <email> <seconds> <zone>`.
    let who = ident
        .trim()
        .rsplitn(3, ' ')
        .nth(2)
        .unwrap_or_default()
        .to_owned();
    let mut message = format!("commits as {who}");
    if let Ok(ssh) = git.check(store, &["config", "--get", "core.sshCommand"]) {
        message.push_str(", pushes with ");
        message.push_str(ssh.trim());
    }
    ok("identity", message)
}

/// The commits `HEAD` has that its upstream had at the last fetch.
fn push(git: &Git, store: &Path) -> Check {
    let ahead = git
        .check(store, &["rev-list", "--count", "@{upstream}..HEAD"])
        .ok()
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or_default();
    if ahead == 0 {
        return ok("push", "every commit is pushed".to_owned());
    }
    Check {
        name: "push",
        status: "warn",
        message: format!("{} not pushed", commits(ahead)),
        fix: Some(SYNC.to_owned()),
    }
}

/// How long ago the job last looked at the remote.
fn fetch(state: &State, now: SystemTime) -> Check {
    match state.last_fetch() {
        Some(at) => ok(
            "fetch",
            format!(
                "last fetch {}",
                ago(now.duration_since(at).unwrap_or_default())
            ),
        ),
        None => Check {
            name: "fetch",
            status: "warn",
            message: "never fetched".to_owned(),
            fix: Some(SYNC.to_owned()),
        },
    }
}

/// `12 s ago`, `5 min ago`, `3 h ago`, `2 d ago`: the largest whole unit.
pub fn ago(age: Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..60 => format!("{secs} s ago"),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_age_is_its_largest_whole_unit() {
        let s = Duration::from_secs;
        assert_eq!(ago(s(0)), "0 s ago");
        assert_eq!(ago(s(59)), "59 s ago");
        assert_eq!(ago(s(60)), "1 min ago");
        assert_eq!(ago(s(3599)), "59 min ago");
        assert_eq!(ago(s(3600)), "1 h ago");
        assert_eq!(ago(s(86_399)), "23 h ago");
        assert_eq!(ago(s(86_400)), "1 d ago");
        assert_eq!(ago(s(3 * 86_400 + 5)), "3 d ago");
    }
}
