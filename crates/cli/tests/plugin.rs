//! The Claude Code plugin under `plugins/claude` (plan T-701): its manifest
//! and skills are checked here so that the launch prompt, which tells the
//! agent to run `/tasq:wrapup`, can never name a skill the plugin does not
//! ship. `claude plugin validate --strict plugins/claude` is the external
//! check; this one runs without Claude Code installed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn plugin_dir() -> PathBuf {
    repo_root().join("plugins/claude")
}

/// The frontmatter of a SKILL.md as `(key, value)` pairs.
fn frontmatter(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("---"),
        "SKILL.md must start with frontmatter"
    );
    lines
        .take_while(|line| *line != "---")
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

fn skills() -> BTreeSet<String> {
    std::fs::read_dir(plugin_dir().join("skills"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn manifest_names_the_tasq_namespace() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(plugin_dir().join(".claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["name"], "tasq",
        "skills are invoked as /tasq:<name>"
    );
    assert_eq!(manifest["license"], "GPL-3.0-or-later");
    assert!(
        manifest["description"]
            .as_str()
            .unwrap()
            .contains("/tasq:wrapup")
    );
    assert!(
        manifest["description"]
            .as_str()
            .unwrap()
            .contains("/tasq:sync")
    );
}

#[test]
fn marketplace_points_at_the_plugin() {
    let marketplace: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(marketplace["name"], "tasq");
    let plugins = marketplace["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["name"], "tasq");
    let source = plugins[0]["source"].as_str().unwrap();
    assert_eq!(source, "./plugins/claude");
    assert!(
        repo_root()
            .join(source)
            .join(".claude-plugin/plugin.json")
            .is_file()
    );
}

#[test]
fn every_skill_has_a_matching_name_and_a_description() {
    let skills = skills();
    assert_eq!(
        skills,
        BTreeSet::from(["sync".to_owned(), "wrapup".to_owned()])
    );
    for skill in &skills {
        let file = plugin_dir().join("skills").join(skill).join("SKILL.md");
        let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file:?}: {e}"));
        let fm = frontmatter(&text);
        let get = |key: &str| {
            fm.iter().find(|(k, _)| k == key).map_or_else(
                || panic!("{skill}: frontmatter lacks {key}"),
                |(_, v)| v.as_str(),
            )
        };
        assert_eq!(get("name"), skill);
        assert!(
            get("description").len() > 40,
            "{skill}: description too short"
        );
        // Every write the skill asks for goes through the CLI.
        assert!(text.contains("tasq "), "{skill}: never calls tasq");
        assert!(
            !text.contains(".todo.md"),
            "{skill}: skills must not edit notebook files directly"
        );
    }
}

#[test]
fn the_launch_prompt_only_names_skills_the_plugin_ships() {
    let template =
        std::fs::read_to_string(repo_root().join("crates/launch/templates/claude.md")).unwrap();
    let skills = skills();
    let mentioned: BTreeSet<String> = template
        .match_indices("/tasq:")
        .map(|(i, _)| {
            template[i + "/tasq:".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect()
        })
        .collect();
    assert_eq!(mentioned, BTreeSet::from(["wrapup".to_owned()]));
    assert!(
        mentioned.is_subset(&skills),
        "{mentioned:?} not all in {skills:?}"
    );
}

#[test]
fn the_statusline_snippet_is_executable_and_reads_the_task_id() {
    use std::os::unix::fs::PermissionsExt;
    let script = plugin_dir().join("statusline/tasq-statusline.sh");
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0, "statusline script must be executable");
    let text = std::fs::read_to_string(&script).unwrap();
    assert!(text.starts_with("#!/bin/sh\n"));
    assert!(text.contains("TASQ_TASK_ID"));
    assert!(text.contains("tasq view --raw"));
}
