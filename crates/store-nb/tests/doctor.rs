//! T-205 (library side): `doctor::checks` on a healthy and on a broken
//! notebook.

mod support;

use tasq_core::config::Bookkeeper as Choice;
use tasq_core::model::Workflow;
use tasq_store_nb::doctor::{any_failed, checks, tool_check};
use tasq_store_nb::{Check, CheckStatus, NbStore, NbStoreOptions};

use support::{NbEnv, file_name, id, nb_or_skip};

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
    let store = nb.open_with(Choice::Native);
    let report = checks(&store);
    assert_eq!(
        names(&report),
        [
            "notebook",
            "index",
            "ids",
            "nb",
            "bookkeeper",
            "git-identity"
        ]
    );
    assert!(!any_failed(&report), "{report:#?}");
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
        "ids are positions in .index (6 tasks) and can change after 'nb index reconcile' or deletions"
    );
    assert!(ids.fix.is_some());
    assert_eq!(by_name(&report, "bookkeeper").detail, "bookkeeper: native");
    let identity = by_name(&report, "git-identity");
    assert_eq!(identity.status, CheckStatus::Ok);
    assert_eq!(
        identity.detail,
        "git identity: tasq tests <tasq-tests@example.invalid>"
    );
    // The nb check depends on whether nb is installed; both verdicts are valid here.
    let nb_check = by_name(&report, "nb");
    if store.nb().is_some() {
        assert_eq!(nb_check.status, CheckStatus::Ok);
        assert!(nb_check.detail.starts_with("nb "), "{nb_check:?}");
    } else {
        assert_eq!(nb_check.status, CheckStatus::Warn);
        assert_eq!(
            nb_check.detail,
            "nb is not on PATH; the native bookkeeper keeps .index and commits"
        );
    }
    let json = serde_json::to_string(&report).unwrap();
    let back: Vec<Check> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn broken_notebook_without_git_or_nb() {
    let nb = NbEnv::fixture();
    let options = NbStoreOptions::new(Workflow::default()).with_env(vec![(
        "PATH".to_owned(),
        nb.root.path().join("nowhere").display().to_string(),
    )]);
    let store = NbStore::open_dir(nb.notebook(), &options).unwrap();
    let report = checks(&store);
    assert_eq!(
        names(&report),
        ["notebook", "index", "ids", "nb", "bookkeeper"],
        "no repository and native: no identity check"
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
        Some("run 'nb index reconcile' in the notebook")
    );
    let nb_check = by_name(&report, "nb");
    assert_eq!(nb_check.status, CheckStatus::Warn);
    assert_eq!(
        nb_check.fix.as_deref(),
        Some("install nb (https://github.com/xwmx/nb) to share the notebook with nb")
    );
    assert_eq!(by_name(&report, "bookkeeper").detail, "bookkeeper: native");

    // Index file gone entirely (opened before deletion).
    std::fs::remove_file(store.index_path()).unwrap();
    let report = checks(&store);
    let index = by_name(&report, "index");
    assert_eq!(index.status, CheckStatus::Fail);
    assert_eq!(
        index.detail,
        format!("{} is missing", store.index_path().display())
    );
}

#[test]
fn missing_identity_is_a_warning_for_native_and_a_failure_for_nb() {
    let nb = NbEnv::fixture();
    nb.restore_missing_file();
    nb.git_init();
    let mut options = nb.options().with_bookkeeper(Choice::Native);
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

    if !nb_or_skip("missing_identity_is_a_warning_for_native_and_a_failure_for_nb (nb part)") {
        return;
    }
    let store = NbStore::open_dir(nb.notebook(), &options.with_bookkeeper(Choice::Nb)).unwrap();
    let report = checks(&store);
    let identity = by_name(&report, "git-identity");
    assert_eq!(identity.status, CheckStatus::Fail);
    assert_eq!(
        identity.detail,
        "git user.name and user.email not set; nb silently skips commits without an identity"
    );
    assert!(any_failed(&report));
}

#[test]
fn nb_checks_against_the_real_nb() {
    if !nb_or_skip("nb_checks_against_the_real_nb") {
        return;
    }
    let nb = NbEnv::fixture();
    nb.git_init();
    let store = nb.open_with(Choice::Nb);
    let report = checks(&store);
    let nb_check = by_name(&report, "nb");
    assert_eq!(nb_check.status, CheckStatus::Ok);
    let version = store.nb().unwrap().version().unwrap();
    assert!(
        version.starts_with(|c: char| c.is_ascii_digit()),
        "{version}"
    );
    assert_eq!(
        nb_check.detail,
        format!(
            "nb {version} at {}",
            store.nb().unwrap().program().display()
        )
    );
    assert_eq!(by_name(&report, "bookkeeper").detail, "bookkeeper: nb");
    // The fixture gap is found through `nb index verify`.
    let index = by_name(&report, "index");
    assert_eq!(index.status, CheckStatus::Fail);
    assert_eq!(
        index.detail,
        format!(
            "{}: nb index verify: index corrupted",
            store.index_path().display()
        )
    );
    assert_eq!(by_name(&report, "git-identity").status, CheckStatus::Ok);
    nb.restore_missing_file();
    assert!(!any_failed(&checks(&store)));
    let _ = file_name(id::MISSING);
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
