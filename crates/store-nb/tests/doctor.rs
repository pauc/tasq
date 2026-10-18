//! T-205 (library side): `doctor::checks` on a healthy and on a broken
//! notebook.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]

mod support;

use tasq_core::model::Workflow;
use tasq_store_nb::doctor::{any_failed, checks, tool_check};
use tasq_store_nb::{Check, CheckStatus, NbStore, NbStoreOptions, Store};

use support::NbEnv;

fn by_name<'a>(checks: &'a [Check], name: &str) -> &'a Check {
    checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no check {name} in {checks:?}"))
}

fn names(checks: &[Check]) -> Vec<&str> {
    checks.iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn healthy_native_notebook() {
    let nb = NbEnv::fixture();
    nb.restore_missing_file();
    nb.git_init();
    let store = nb.open_syncing(3600);
    let report = checks(&store);
    assert_eq!(
        names(&report),
        ["notebook", "index", "ids", "bookkeeper", "git-identity"]
    );
    assert!(!any_failed(&report), "{report:#?}");
    assert_eq!(store.doctor(), report, "the trait reports the same checks");
    let notebook = by_name(&report, "notebook");
    assert_eq!(notebook.status, CheckStatus::Ok);
    assert_eq!(
        notebook.detail,
        format!("notebook at {}, a git repository", nb.notebook().display())
    );
    let index = by_name(&report, "index");
    assert_eq!(index.status, CheckStatus::Ok);
    assert_eq!(
        index.detail,
        format!("{}: index matches the files", store.index_path().display())
    );
    let ids = by_name(&report, "ids");
    assert_eq!(ids.status, CheckStatus::Warn, "positional ids always warn");
    assert_eq!(
        ids.detail,
        "ids are positions in .index (6 tasks) and can change when another tool rewrites it (nb index reconcile)"
    );
    assert!(ids.fix.is_some());
    assert_eq!(
        by_name(&report, "bookkeeper").detail,
        "bookkeeper: native, syncing at most every 3600 s after a write"
    );
    let identity = by_name(&report, "git-identity");
    assert_eq!(identity.status, CheckStatus::Ok);
    assert_eq!(
        identity.detail,
        "git identity: tasq tests <tasq-tests@example.invalid>"
    );
    let json = serde_json::to_string(&report).unwrap();
    let back: Vec<Check> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn broken_notebook_without_git() {
    let nb = NbEnv::fixture();
    let options = NbStoreOptions::new(Workflow::default()).with_env(vec![(
        "PATH".to_owned(),
        nb.root.path().join("nowhere").display().to_string(),
    )]);
    let store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    let report = checks(&store);
    assert_eq!(
        names(&report),
        ["notebook", "index", "ids", "bookkeeper"],
        "no repository: no identity check"
    );
    assert!(any_failed(&report));
    assert_eq!(
        by_name(&report, "notebook").detail,
        format!(
            "notebook at {}, not a git repository (no commits or sync)",
            nb.notebook().display()
        )
    );
    let index = by_name(&report, "index");
    assert_eq!(index.status, CheckStatus::Fail);
    assert_eq!(
        index.detail,
        format!(
            "{}: index out of date: 1 listed file(s) missing, 0 file(s) not listed",
            store.index_path().display()
        )
    );
    assert_eq!(
        index.fix.as_deref(),
        Some(
            "edit .index by hand: blank the line of a file that is gone, append a file that is not listed"
        )
    );
    assert_eq!(
        by_name(&report, "bookkeeper").detail,
        "bookkeeper: native, no automatic sync"
    );

    // Index file gone entirely (opened before deletion).
    std::fs::remove_file(store.index_path()).unwrap();
    let report = checks(&store);
    let index = by_name(&report, "index");
    assert_eq!(index.status, CheckStatus::Fail);
    assert_eq!(
        index.detail,
        format!("{} is missing", store.index_path().display())
    );
    assert_eq!(
        index.fix.as_deref(),
        Some(
            "edit .index by hand: blank the line of a file that is gone, append a file that is not listed"
        )
    );
}

#[test]
fn missing_identity_is_a_warning() {
    let nb = NbEnv::fixture();
    nb.restore_missing_file();
    nb.git_init();
    let mut options = nb.options();
    let bare_home = nb.root.path().join("bare-home");
    std::fs::create_dir_all(&bare_home).unwrap();
    for (k, v) in &mut options.env {
        if k == "HOME" {
            *v = bare_home.display().to_string();
        }
    }
    let store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    let report = checks(&store);
    let identity = by_name(&report, "git-identity");
    assert_eq!(identity.status, CheckStatus::Warn);
    assert_eq!(
        identity.detail,
        "git user.name and user.email not set; commits will fail"
    );
    assert_eq!(
        identity.fix.as_deref(),
        Some(
            "set a git identity: git config --global user.name <name>; git config --global user.email <email>"
        )
    );

    assert!(!any_failed(&report));
}

#[test]
fn tool_checks_use_the_store_environment_shape() {
    let nb = NbEnv::fixture();
    let git = tool_check("git", &nb.env, "install git");
    assert_eq!(git.status, CheckStatus::Ok);
    let missing = tool_check("definitely-not-a-tool", &nb.env, "install it");
    assert_eq!(missing.status, CheckStatus::Warn);
    assert_eq!(missing.fix.as_deref(), Some("install it"));
}
