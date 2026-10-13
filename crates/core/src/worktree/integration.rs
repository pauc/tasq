//! What "nothing is lost" means for a branch (plan 4.2 step 3, WT-205).
//!
//! A branch may be deleted when its tip is integrated into the default
//! branch, or when its upstream on the remote contains it. Integration is a
//! list of checks, cheapest first, that stops at the first one passing; the
//! later ones catch squash and rebase merges, which leave no common commit.
//! The caller runs git for each check and hands the outputs here.

use super::lifecycle::BranchSeen;

/// One way a branch's tip can be integrated into a target branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The branch and the target are the same commit.
    SameCommit,
    /// The branch's tip is an ancestor of the target (a merge or a
    /// fast-forward).
    Ancestor,
    /// The branch changes nothing since it left the target (`git diff
    /// target...branch` is empty).
    NoAddedChanges,
    /// The branch's tree is the target's tree.
    SameTree,
    /// Merging the branch into the target would not change the target's
    /// tree (a squash merge, maybe followed by other work).
    MergeAddsNothing,
    /// Every commit of the branch has an equivalent patch in the target
    /// (`git cherry`; a rebase merge, or a squash of one commit).
    PatchesUpstream,
}

impl Check {
    /// Every check, cheapest first: the order [`first_passing`] runs them.
    pub const ALL: [Check; 6] = [
        Check::SameCommit,
        Check::Ancestor,
        Check::NoAddedChanges,
        Check::SameTree,
        Check::MergeAddsNothing,
        Check::PatchesUpstream,
    ];

    /// How the branch got into the target, for messages
    /// (`deleted feat/x (squash-merged into origin/main)`).
    pub fn describe(self) -> &'static str {
        match self {
            Self::SameCommit => "same commit as",
            Self::Ancestor => "merged into",
            Self::NoAddedChanges => "no changes beyond",
            Self::SameTree => "same content as",
            Self::MergeAddsNothing => "squash-merged into",
            Self::PatchesUpstream => "rebased onto",
        }
    }
}

/// The first check, in [`Check::ALL`] order, that `passes`; the later ones
/// are not asked.
pub fn first_passing(mut passes: impl FnMut(Check) -> bool) -> Option<Check> {
    Check::ALL.into_iter().find(|&check| passes(check))
}

/// The refs a branch is checked against: the default branch on the remote,
/// as `refs/remotes/origin/HEAD` points to it (`origin_head`, the output of
/// `git symbolic-ref`), then the local branch of the same name. Without an
/// `origin/HEAD`, the local `main`, else `master`. `has_ref` tells which
/// local branches exist; `branch` itself is never a target.
pub fn targets(
    origin_head: Option<&str>,
    branch: &str,
    has_ref: impl Fn(&str) -> bool,
) -> Vec<String> {
    let own = format!("refs/heads/{branch}");
    let remote = origin_head
        .map(str::trim)
        .filter(|r| r.starts_with("refs/remotes/origin/") && *r != "refs/remotes/origin/HEAD");
    let mut found: Vec<String> = Vec::new();
    match remote {
        Some(remote) => {
            found.push(remote.to_owned());
            let name = &remote["refs/remotes/origin/".len()..];
            found.push(format!("refs/heads/{name}"));
        }
        None => {
            if let Some(local) = ["refs/heads/main", "refs/heads/master"]
                .into_iter()
                .find(|r| has_ref(r))
            {
                found.push(local.to_owned());
            }
        }
    }
    found.retain(|t| *t != own && (t.starts_with("refs/remotes/") || has_ref(t)));
    found
}

/// The remote ref whose containing the branch's tip means it is pushed:
/// its upstream (`git rev-parse --symbolic-full-name <branch>@{upstream}`)
/// when that is a remote-tracking ref, else `refs/remotes/origin/<branch>`.
/// An upstream that is a local branch does not count: it is not on the
/// remote.
pub fn pushed_ref(upstream: Option<&str>, branch: &str) -> String {
    upstream
        .map(str::trim)
        .filter(|u| u.starts_with("refs/remotes/"))
        .map_or_else(|| format!("refs/remotes/origin/{branch}"), str::to_owned)
}

/// Whether `git cherry <target> <branch>` says every commit of the branch
/// has an equivalent in the target: no line starts with `+`. No commits at
/// all passes too.
pub fn cherry_all_upstream(output: &str) -> bool {
    !output.lines().any(|line| line.starts_with('+'))
}

/// The tree `git merge-tree --write-tree` wrote: the first line of a clean
/// merge's output.
pub fn merge_tree(output: &str) -> Option<&str> {
    output
        .lines()
        .next()
        .map(str::trim)
        .filter(|oid| !oid.is_empty())
}

/// Where a branch's tip is safe, as the checks found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Safety {
    /// The first target it is integrated into, and the check that passed.
    pub integrated: Option<(String, Check)>,
    /// The remote ref that contains it.
    pub pushed: Option<String>,
}

impl Safety {
    /// The observation [`super::lifecycle::on_close`] takes for a local
    /// branch.
    pub fn seen(&self) -> BranchSeen {
        BranchSeen::Present {
            integrated: self.integrated.is_some(),
            pushed: self.pushed.is_some(),
        }
    }

    /// Why deleting the branch loses nothing, for messages: `squash-merged
    /// into origin/main`, `pushed to origin/feat/x`; `None` when it would.
    pub fn reason(&self) -> Option<String> {
        if let Some((target, check)) = &self.integrated {
            return Some(format!("{} {}", check.describe(), short_ref(target)));
        }
        self.pushed
            .as_deref()
            .map(|r| format!("pushed to {}", short_ref(r)))
    }
}

/// `refs/remotes/origin/main` as `origin/main`, `refs/heads/main` as `main`.
pub fn short_ref(reference: &str) -> &str {
    reference
        .strip_prefix("refs/remotes/")
        .or_else(|| reference.strip_prefix("refs/heads/"))
        .unwrap_or(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_run_cheapest_first_and_stop_at_the_first_pass() {
        for (i, wanted) in Check::ALL.into_iter().enumerate() {
            let mut asked = Vec::new();
            let found = first_passing(|c| {
                asked.push(c);
                c == wanted
            });
            assert_eq!(found, Some(wanted));
            assert_eq!(asked, Check::ALL[..=i].to_vec());
        }
        let mut asked = Vec::new();
        assert_eq!(
            first_passing(|c| {
                asked.push(c);
                false
            }),
            None
        );
        assert_eq!(asked, Check::ALL.to_vec());
    }

    #[test]
    fn targets_follow_origin_head() {
        let all = |_: &str| true;
        assert_eq!(
            targets(Some("refs/remotes/origin/main\n"), "feat/x", all),
            ["refs/remotes/origin/main", "refs/heads/main"]
        );
        assert_eq!(
            targets(Some("refs/remotes/origin/develop"), "x", |r| r
                != "refs/heads/develop"),
            ["refs/remotes/origin/develop"],
            "no local develop"
        );
        assert_eq!(
            targets(Some("refs/remotes/origin/main"), "main", all),
            ["refs/remotes/origin/main"],
            "never the branch itself"
        );
    }

    #[test]
    fn targets_without_origin_head_are_the_local_main_or_master() {
        let has = |refs: &'static [&'static str]| move |r: &str| refs.contains(&r);
        assert_eq!(
            targets(None, "x", has(&["refs/heads/main", "refs/heads/master"])),
            ["refs/heads/main"]
        );
        assert_eq!(
            targets(None, "x", has(&["refs/heads/master"])),
            ["refs/heads/master"]
        );
        assert_eq!(targets(None, "x", has(&[])), Vec::<String>::new());
        assert_eq!(
            targets(None, "main", has(&["refs/heads/main"])),
            Vec::<String>::new()
        );
        for odd in [
            "",
            "refs/heads/main",
            "refs/remotes/origin/HEAD",
            "refs/remotes/up/main",
        ] {
            assert_eq!(
                targets(Some(odd), "x", has(&["refs/heads/master"])),
                ["refs/heads/master"],
                "{odd:?}"
            );
        }
    }

    #[test]
    fn pushed_ref_is_the_remote_upstream_or_origin() {
        assert_eq!(
            pushed_ref(Some("refs/remotes/fork/feat\n"), "feat/x"),
            "refs/remotes/fork/feat"
        );
        assert_eq!(pushed_ref(None, "feat/x"), "refs/remotes/origin/feat/x");
        assert_eq!(
            pushed_ref(Some("refs/heads/main"), "feat/x"),
            "refs/remotes/origin/feat/x",
            "a local upstream is not the remote"
        );
    }

    #[test]
    fn cherry_output() {
        assert!(cherry_all_upstream(""));
        assert!(cherry_all_upstream("- abc\n- def\n"));
        assert!(!cherry_all_upstream("- abc\n+ def\n"));
        assert!(!cherry_all_upstream("+ abc"));
    }

    #[test]
    fn merge_tree_output() {
        assert_eq!(merge_tree("abc123\n"), Some("abc123"));
        assert_eq!(merge_tree("abc\n100644 x 1\tf\n"), Some("abc"));
        assert_eq!(merge_tree(""), None);
        assert_eq!(merge_tree("  \nabc"), None);
    }

    #[test]
    fn safety_reasons() {
        let integrated = Safety {
            integrated: Some(("refs/remotes/origin/main".into(), Check::MergeAddsNothing)),
            pushed: Some("refs/remotes/origin/x".into()),
        };
        assert_eq!(
            integrated.seen(),
            BranchSeen::Present {
                integrated: true,
                pushed: true
            }
        );
        assert_eq!(
            integrated.reason().as_deref(),
            Some("squash-merged into origin/main")
        );
        let pushed = Safety {
            integrated: None,
            pushed: Some("refs/remotes/origin/x".into()),
        };
        assert_eq!(
            pushed.seen(),
            BranchSeen::Present {
                integrated: false,
                pushed: true
            }
        );
        assert_eq!(pushed.reason().as_deref(), Some("pushed to origin/x"));
        let neither = Safety {
            integrated: None,
            pushed: None,
        };
        assert_eq!(
            neither.seen(),
            BranchSeen::Present {
                integrated: false,
                pushed: false
            }
        );
        assert_eq!(neither.reason(), None);
    }

    #[test]
    fn descriptions_and_short_refs() {
        let described: Vec<_> = Check::ALL.into_iter().map(Check::describe).collect();
        assert_eq!(
            described,
            [
                "same commit as",
                "merged into",
                "no changes beyond",
                "same content as",
                "squash-merged into",
                "rebased onto",
            ]
        );
        assert_eq!(short_ref("refs/remotes/origin/main"), "origin/main");
        assert_eq!(short_ref("refs/heads/main"), "main");
        assert_eq!(short_ref("HEAD"), "HEAD");
    }
}
