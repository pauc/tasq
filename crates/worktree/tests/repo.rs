//! Finding the repository a directory is inside from the files git leaves
//! on disk: a checkout's `.git` directory, a linked worktree's `.git` file
//! and its `commondir`, the shared `config`.

use std::fs;
use std::path::{Path, PathBuf};

use tasq_worktree::Repo;
use tempfile::TempDir;

const ORIGIN: &str =
    "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@gitlab.com:group/shop.git\n";

/// A temp dir, canonical so paths compare with what `Repo::of` resolves.
fn tree() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    (dir, root)
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A main checkout at `<root>/shop` with `config`, and a linked worktree
/// `b` whose git directory is `.git/worktrees/b`.
fn shop(root: &Path, config: &str) -> PathBuf {
    let main = root.join("shop");
    write(&main.join(".git/config"), config);
    write(&main.join(".git/worktrees/b/commondir"), "../..\n");
    fs::create_dir_all(main.join("src/bin")).unwrap();
    main
}

#[test]
fn a_checkout_is_its_own_main() {
    let (_dir, root) = tree();
    let main = shop(&root, ORIGIN);
    let expected = Some(Repo {
        main: main.clone(),
        name: Some("shop".to_owned()),
    });
    assert_eq!(Repo::of(&main), expected);
    assert_eq!(
        Repo::of(&main.join("src/bin")),
        expected,
        "from a subdirectory"
    );
}

#[test]
fn a_linked_worktree_names_its_main_checkout() {
    let (_dir, root) = tree();
    let main = shop(&root, ORIGIN);
    let expected = Some(Repo {
        main: main.clone(),
        name: Some("shop".to_owned()),
    });

    let absolute = root.join("shop-b");
    let gitdir = main.join(".git/worktrees/b");
    write(
        &absolute.join(".git"),
        &format!("gitdir: {}\n", gitdir.display()),
    );
    assert_eq!(Repo::of(&absolute), expected);

    let relative = root.join("shop-c");
    write(&relative.join(".git"), "gitdir: ../shop/.git/worktrees/b\n");
    fs::create_dir_all(relative.join("lib")).unwrap();
    assert_eq!(Repo::of(&relative.join("lib")), expected);
}

#[test]
fn without_an_origin_there_is_no_name() {
    let (_dir, root) = tree();
    let main = shop(&root, "[core]\n\tbare = false\n");
    assert_eq!(
        Repo::of(&main),
        Some(Repo {
            main: main.clone(),
            name: None,
        })
    );

    let (_dir, root) = tree();
    let main = root.join("bare");
    fs::create_dir_all(main.join(".git")).unwrap();
    assert_eq!(
        Repo::of(&main),
        Some(Repo {
            main: main.clone(),
            name: None,
        }),
        "no config at all"
    );
}

#[test]
fn a_shared_dir_not_named_git_leaves_the_work_tree_as_main() {
    let (_dir, root) = tree();
    write(&root.join("store/shop.git/config"), ORIGIN);
    let work = root.join("work");
    write(&work.join(".git"), "gitdir: ../store/shop.git\n");
    assert_eq!(
        Repo::of(&work),
        Some(Repo {
            main: work.clone(),
            name: Some("shop".to_owned()),
        })
    );
}

#[test]
fn a_git_file_without_gitdir_stands_for_itself() {
    let (_dir, root) = tree();
    let work = root.join("odd");
    write(&work.join(".git"), "not a pointer\n");
    assert_eq!(
        Repo::of(&work),
        Some(Repo {
            main: work.clone(),
            name: None,
        })
    );
}

#[test]
fn outside_a_repository_there_is_none() {
    let (_dir, root) = tree();
    let plain = root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(Repo::of(&plain), None);
}
