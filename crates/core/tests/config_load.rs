//! Integration tests for layered configuration loading.
//!
//! Every test builds its own home and working directory under a `tempfile`
//! directory, so nothing depends on the machine running the tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tasq_core::config::{
    Bookkeeper, Config, ConfigError, EnvStrategy, ForgeKind, LoadOptions, Origin, SourceKind,
    Summarizer, WorktreeManager, expand_tilde,
};
use tasq_core::model::Status;
use tempfile::TempDir;

/// A throwaway `$HOME` with a `project/sub` tree inside it.
struct Sandbox {
    _dir: TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let project = home.join("code").join("project");
        fs::create_dir_all(project.join("sub")).unwrap();
        fs::create_dir_all(home.join(".config").join("tasq")).unwrap();
        Self {
            _dir: dir,
            home,
            project,
        }
    }

    fn global_path(&self) -> PathBuf {
        self.home.join(".config/tasq/config.toml")
    }

    fn write_global(&self, text: &str) -> PathBuf {
        let path = self.global_path();
        fs::write(&path, text).unwrap();
        path
    }

    fn write_project(&self, text: &str) -> PathBuf {
        let path = self.project.join(".tasq.toml");
        fs::write(&path, text).unwrap();
        path
    }

    fn opts(&self) -> LoadOptions {
        LoadOptions {
            cwd: self.project.clone(),
            home: Some(self.home.clone()),
            env: BTreeMap::new(),
            explicit_file: None,
            profile: None,
            overrides: Vec::new(),
        }
    }

    fn opts_in(&self, cwd: &Path) -> LoadOptions {
        LoadOptions {
            cwd: cwd.to_path_buf(),
            ..self.opts()
        }
    }
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn status(s: &str) -> Status {
    Status::new(s).unwrap()
}

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/config")
            .join(name),
    )
    .unwrap()
}

// --- defaults ---------------------------------------------------------------

#[test]
fn empty_environment_yields_defaults() {
    let loaded = Config::load(&LoadOptions::new("/nonexistent/cwd")).unwrap();
    assert_eq!(loaded.config, Config::default());
    assert_eq!(loaded.layers.len(), 1);
    assert_eq!(loaded.layers[0].origin, Origin::Defaults);
    assert_eq!(loaded.profile, None);
    assert_eq!(loaded.profiles, Vec::<String>::new());
    assert_eq!(loaded.explain("store.notebook"), Some(&Origin::Defaults));
    assert_eq!(loaded.file_for("store.notebook"), None);
    assert_eq!(loaded.explain("work.default_project"), None);
    assert_eq!(loaded.explain("no.such.key"), None);
}

#[test]
fn defaults_match_the_script() {
    let c = Config::default();
    assert_eq!(c.store.notebook, "home");
    assert_eq!(c.store.bookkeeper, Bookkeeper::Auto);
    assert_eq!(c.workflow.statuses, Status::DEFAULTS.to_vec());
    assert_eq!(c.workflow.default_status, Status::READY);
    assert_eq!(c.workflow.workflow(), tasq_core::model::Workflow::default());
    assert_eq!(c.work.default_project, None);
    assert_eq!(c.work.worktree_manager, WorktreeManager::Git);
    assert_eq!(c.work.worktree_command, None);
    assert_eq!(c.launch.default, "claude");
    assert_eq!(c.launch.env, EnvStrategy::Direnv);
    assert_eq!(c.launch.claude.prompt_file, None);
    assert_eq!(c.ui.pager, "less -RFX");
    assert!(!c.ui.no_osc8);
    assert_eq!(c.ui.glow_style, "dark");
    assert!(c.ui.colors.is_empty());
    assert!(c.forge.is_empty());
    assert_eq!(c.source.len(), 0);
    assert_eq!(c.report.summary.summarizer, Summarizer::Llm);
    assert_eq!(c.report.summary.command, "claude -p");
    assert_eq!(c.report.summary.model, None);
}

#[test]
fn default_config_round_trips_through_toml_and_matches_the_module_docs() {
    let text = toml::to_string(&Config::default()).unwrap();
    let back: Config = toml::from_str(&text).unwrap();
    assert_eq!(back, Config::default());
    // The reference document in the module docs is this serialisation.
    let expected = "\
[store]
kind = \"nb\"
notebook = \"home\"
bookkeeper = \"auto\"

[workflow]
statuses = [\"in-progress\", \"ready\", \"waiting\", \"blocked\", \"later\"]
default_status = \"ready\"

[work]
worktree_manager = \"git\"

[launch]
default = \"claude\"
env = \"direnv\"

[launch.claude]

[ui]
pager = \"less -RFX\"
no_osc8 = false
glow_style = \"dark\"

[ui.colors]

[forge]

[report.summary]
summarizer = \"llm\"
command = \"claude -p\"
";
    assert_eq!(text, expected);
}

#[test]
fn empty_file_is_the_defaults() {
    let sb = Sandbox::new();
    let path = sb.write_global("");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config, Config::default());
    assert_eq!(loaded.layers[1].origin, Origin::File(path));
    assert_eq!(loaded.layers[1].keys, Vec::<String>::new());
}

// --- files -----------------------------------------------------------------

#[test]
fn global_file_alone() {
    let sb = Sandbox::new();
    let path = sb.write_global("[store]\nnotebook = \"work\"\n[ui]\nno_osc8 = true\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.notebook, "work");
    assert!(loaded.config.ui.no_osc8);
    assert_eq!(
        loaded.config.ui.pager, "less -RFX",
        "untouched keys keep defaults"
    );
    assert_eq!(
        loaded.explain("store.notebook"),
        Some(&Origin::File(path.clone()))
    );
    assert_eq!(loaded.file_for("store.notebook"), Some(path.as_path()));
    assert_eq!(loaded.explain("store.bookkeeper"), Some(&Origin::Defaults));
    assert_eq!(loaded.layers[1].keys, vec!["store.notebook", "ui.no_osc8"]);
}

#[test]
fn xdg_config_home_replaces_dot_config() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nnotebook = \"from-dot-config\"\n");
    let xdg = sb.home.join("xdg");
    fs::create_dir_all(xdg.join("tasq")).unwrap();
    fs::write(
        xdg.join("tasq/config.toml"),
        "[store]\nnotebook = \"from-xdg\"\n",
    )
    .unwrap();
    let mut opts = sb.opts();
    opts.env = env(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "from-xdg");

    // An empty XDG_CONFIG_HOME is ignored, as the spec says.
    opts.env = env(&[("XDG_CONFIG_HOME", "")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "from-dot-config");
}

#[test]
fn without_home_there_is_no_global_file_but_xdg_still_works() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nnotebook = \"global\"\n");
    let mut opts = sb.opts();
    opts.home = None;
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "home");

    let xdg = sb.home.join("xdg");
    fs::create_dir_all(xdg.join("tasq")).unwrap();
    fs::write(
        xdg.join("tasq/config.toml"),
        "[store]\nnotebook = \"xdg\"\n",
    )
    .unwrap();
    opts.env = env(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "xdg");
}

#[test]
fn project_file_overrides_global_field_wise() {
    let sb = Sandbox::new();
    let global = sb.write_global(
        "[store]\nnotebook = \"global\"\nbookkeeper = \"nb\"\n[ui]\nglow_style = \"light\"\n",
    );
    let project = sb.write_project("[store]\nnotebook = \"project\"\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.notebook, "project");
    assert_eq!(
        loaded.config.store.bookkeeper,
        Bookkeeper::Nb,
        "sibling key survives"
    );
    assert_eq!(loaded.config.ui.glow_style, "light");
    assert_eq!(
        loaded.explain("store.notebook"),
        Some(&Origin::File(project.clone()))
    );
    assert_eq!(
        loaded.explain("store.bookkeeper"),
        Some(&Origin::File(global.clone()))
    );
    assert_eq!(loaded.explain("store"), Some(&Origin::File(project)));
    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(origins.len(), 3);
    assert_eq!(origins[1], &Origin::File(global));
}

#[test]
fn project_file_is_found_from_a_nested_cwd() {
    let sb = Sandbox::new();
    let project = sb.write_project("[store]\nnotebook = \"nested\"\n");
    let loaded = Config::load(&sb.opts_in(&sb.project.join("sub"))).unwrap();
    assert_eq!(loaded.config.store.notebook, "nested");
    assert_eq!(loaded.file_for("store.notebook"), Some(project.as_path()));
}

#[test]
fn nearest_project_file_wins_and_only_one_is_read() {
    let sb = Sandbox::new();
    sb.write_project("[store]\nnotebook = \"outer\"\n[ui]\nno_osc8 = true\n");
    let inner = sb.project.join("sub/.tasq.toml");
    fs::write(&inner, "[store]\nnotebook = \"inner\"\n").unwrap();
    let loaded = Config::load(&sb.opts_in(&sb.project.join("sub"))).unwrap();
    assert_eq!(loaded.config.store.notebook, "inner");
    assert!(!loaded.config.ui.no_osc8, "the outer file is not merged");
    assert_eq!(loaded.layers.len(), 2);
}

#[test]
fn walk_up_checks_home_itself_but_stops_there() {
    let sb = Sandbox::new();
    fs::write(
        sb.home.join(".tasq.toml"),
        "[store]\nnotebook = \"in-home\"\n",
    )
    .unwrap();
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.notebook, "in-home");

    // A file above the home directory is never picked up.
    fs::remove_file(sb.home.join(".tasq.toml")).unwrap();
    let above = sb.home.parent().unwrap().join(".tasq.toml");
    fs::write(&above, "[store]\nnotebook = \"above-home\"\n").unwrap();
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.notebook, "home");

    // ... unless the cwd is outside the home directory altogether.
    let outside = sb.home.parent().unwrap().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let loaded = Config::load(&sb.opts_in(&outside)).unwrap();
    assert_eq!(loaded.config.store.notebook, "above-home");
    let direct = tasq_core::config::project_file(&outside, Some(&sb.home));
    assert_eq!(direct, Some(above));
    assert_eq!(
        tasq_core::config::project_file(&sb.project, Some(&sb.home)),
        None
    );
}

#[test]
fn a_directory_named_like_the_project_file_is_skipped() {
    let sb = Sandbox::new();
    fs::create_dir(sb.project.join("sub/.tasq.toml")).unwrap();
    sb.write_project("[store]\nnotebook = \"real\"\n");
    let loaded = Config::load(&sb.opts_in(&sb.project.join("sub"))).unwrap();
    assert_eq!(loaded.config.store.notebook, "real");
}

#[test]
fn explicit_file_replaces_the_global_file_and_must_exist() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nnotebook = \"global\"\n[ui]\nno_osc8 = true\n");
    let explicit = sb.home.join("explicit.toml");
    fs::write(&explicit, "[store]\nnotebook = \"explicit\"\n").unwrap();
    sb.write_project("[ui]\nglow_style = \"light\"\n");

    let mut opts = sb.opts();
    opts.explicit_file = Some(explicit.clone());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "explicit");
    assert!(!loaded.config.ui.no_osc8, "global file is not read");
    assert_eq!(
        loaded.config.ui.glow_style, "light",
        "project file still applies"
    );

    // TASQ_CONFIG does the same, with `~` expanded; the flag wins over it.
    opts.explicit_file = None;
    opts.env = env(&[("TASQ_CONFIG", "~/explicit.toml")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.file_for("store.notebook"), Some(explicit.as_path()));

    opts.env = env(&[("TASQ_CONFIG", "~/missing.toml")]);
    let err = Config::load(&opts).unwrap_err();
    match &err {
        ConfigError::MissingFile { file, named_by } => {
            assert_eq!(file, &sb.home.join("missing.toml"));
            assert_eq!(*named_by, "TASQ_CONFIG");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        format!(
            "config file {} does not exist (named by TASQ_CONFIG)",
            sb.home.join("missing.toml").display()
        )
    );

    opts.explicit_file = Some(PathBuf::from("/nonexistent.toml"));
    let err = Config::load(&opts).unwrap_err();
    assert!(
        matches!(
            err,
            ConfigError::MissingFile {
                named_by: "--config",
                ..
            }
        ),
        "{err}"
    );

    // An empty TASQ_CONFIG is ignored.
    opts.explicit_file = None;
    opts.env = env(&[("TASQ_CONFIG", "")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "global");
}

#[test]
fn unreadable_file_names_the_file() {
    let sb = Sandbox::new();
    let path = sb.global_path();
    fs::create_dir(&path).unwrap();
    let mut opts = sb.opts();
    opts.explicit_file = None;
    // A directory is not a file, so it is skipped as the global file ...
    assert!(Config::load(&opts).is_ok());
    // ... but naming it explicitly fails as missing.
    opts.explicit_file = Some(path.clone());
    let err = Config::load(&opts).unwrap_err();
    assert!(matches!(err, ConfigError::MissingFile { .. }), "{err}");

    // An unreadable regular file reports a read error.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable = sb.write_project("[store]\nnotebook = \"x\"\n");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let result = Config::load(&sb.opts());
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
        // Root reads anything, so only assert when the read actually failed.
        if let Err(err) = result {
            assert!(
                matches!(&err, ConfigError::Read { file, .. } if file == &unreadable),
                "{err}"
            );
            assert!(err.to_string().starts_with(&format!(
                "cannot read config file {}: ",
                unreadable.display()
            )));
        }
    }
}

// --- errors in files -------------------------------------------------------

#[test]
fn unknown_key_names_file_line_and_column() {
    let sb = Sandbox::new();
    let path = sb.write_global("[store]\nnotebook = \"x\"\nnotbook = \"typo\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    match &err {
        ConfigError::Parse {
            file,
            line,
            column,
            message,
        } => {
            assert_eq!(file, &path);
            assert_eq!((*line, *column), (3, 1));
            assert!(message.contains("unknown field `notbook`"), "{message}");
            assert!(
                message.contains("`notebook`"),
                "lists the valid keys: {message}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        err.to_string()
            .starts_with(&format!("{}:3:1: unknown field `notbook`", path.display())),
        "{err}"
    );
}

#[test]
fn unknown_section_and_nested_unknown_key_are_errors() {
    let sb = Sandbox::new();
    sb.write_global("[stor]\nnotebook = \"x\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(err.to_string().contains("unknown field `stor`"), "{err}");

    sb.write_global("[launch.claude]\nprompt = \"x\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(err.to_string().contains("unknown field `prompt`"), "{err}");

    sb.write_global("[[source]]\nname = \"a\"\nkind = \"llm-bridge\"\ncommand = \"c\"\nfoo = 1\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(err.to_string().contains("unknown field `foo`"), "{err}");

    sb.write_global("[forge.gitlab]\nhots = \"x\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(err.to_string().contains("unknown field `hots`"), "{err}");

    sb.write_global("[profile.x]\nnotebook = \"x\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        err.to_string().contains("unknown field `notebook`"),
        "{err}"
    );
}

#[test]
fn syntax_error_names_file_and_line() {
    let sb = Sandbox::new();
    let path = sb.write_project("[store]\nnotebook = \n");
    let err = Config::load(&sb.opts()).unwrap_err();
    match &err {
        ConfigError::Parse { file, line, .. } => {
            assert_eq!(file, &path);
            assert_eq!(*line, 2);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn invalid_status_spelling_is_an_error_with_position() {
    let sb = Sandbox::new();
    let path = sb.write_global("[workflow]\nstatuses = [\"ready\", \"In Progress\"]\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    match &err {
        ConfigError::Parse {
            file,
            line,
            column,
            message,
        } => {
            assert_eq!(file, &path);
            assert_eq!(*line, 2);
            assert_eq!(
                *column, 12,
                "points at the value (toml spans the whole array)"
            );
            assert_eq!(
                message,
                "invalid status \"In Progress\": expected lowercase words joined by '-'"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn invalid_enum_value_lists_the_expected_ones() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nbookkeeper = \"manual\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("unknown variant `manual`"), "{err}");
    assert!(err.contains("`auto`, `nb`, `native`"), "{err}");

    sb.write_global("[[source]]\nname = \"a\"\nkind = \"slack\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("unknown variant `slack`"), "{err}");
    assert!(err.contains("`llm-bridge`"), "{err}");
}

#[test]
fn wrong_type_is_an_error() {
    let sb = Sandbox::new();
    sb.write_global("[ui]\nno_osc8 = \"yes\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("expected a boolean"), "{err}");
}

#[test]
fn default_status_must_be_in_the_workflow() {
    let sb = Sandbox::new();
    let global = sb.write_global("[workflow]\ndefault_status = \"todo\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    match &err {
        ConfigError::DefaultStatusNotInWorkflow {
            status,
            statuses,
            origin,
        } => {
            assert_eq!(status, "todo");
            assert_eq!(statuses.len(), 5);
            assert_eq!(origin, &Origin::File(global.clone()));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        format!(
            "workflow.default_status = \"todo\" (set by {}) is not in workflow.statuses \
             [\"in-progress\", \"ready\", \"waiting\", \"blocked\", \"later\"]",
            global.display()
        )
    );

    // The project file shrinking the list breaks the global default.
    sb.write_global("[workflow]\ndefault_status = \"later\"\n");
    sb.write_project("[workflow]\nstatuses = [\"todo\", \"doing\"]\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::DefaultStatusNotInWorkflow { origin, .. } if origin == &Origin::File(global.clone())),
        "{err}"
    );

    // Shrinking the list without naming a default breaks the built-in one.
    fs::remove_file(&global).unwrap();
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::DefaultStatusNotInWorkflow { origin: Origin::Defaults, status, .. } if status == "ready"),
        "{err}"
    );

    sb.write_project("[workflow]\nstatuses = [\"todo\", \"doing\"]\ndefault_status = \"todo\"\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.workflow.default_status, status("todo"));
    assert_eq!(
        loaded.config.workflow.workflow().statuses,
        vec![status("todo"), status("doing")]
    );
}

// --- environment and overrides ---------------------------------------------

#[test]
fn env_overrides_files() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nnotebook = \"global\"\n");
    sb.write_project("[store]\nnotebook = \"project\"\n[ui]\npager = \"more\"\n");
    let mut opts = sb.opts();
    opts.env = env(&[
        ("TASQ_NOTEBOOK", "env"),
        ("TASQ_BOOKKEEPER", "native"),
        ("TASQ_DEFAULT_PROJECT", "~/code/x"),
        ("TASQ_WORKTREE_MANAGER", "git"),
        ("TASQ_WORKTREE_COMMAND", "mkwt {branch}"),
        ("TASQ_LAUNCHER", "tmux"),
        ("TASQ_LAUNCH_ENV", "inherit"),
        ("TASQ_PAGER", "bat -p"),
        ("TASQ_NO_OSC8", "1"),
        ("TASQ_GLOW_STYLE", "light"),
        ("TASQ_SUMMARIZER", "raw"),
        ("TASQ_SUMMARY_MODEL", "opus"),
        ("UNRELATED", "ignored"),
    ]);
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.notebook, "env");
    assert_eq!(c.store.bookkeeper, Bookkeeper::Native);
    assert_eq!(c.work.default_project, Some(sb.home.join("code/x")));
    assert_eq!(c.work.worktree_manager, WorktreeManager::Git);
    assert_eq!(c.work.worktree_command.as_deref(), Some("mkwt {branch}"));
    assert_eq!(c.launch.default, "tmux");
    assert_eq!(c.launch.env, EnvStrategy::Inherit);
    assert_eq!(c.ui.pager, "bat -p");
    assert!(c.ui.no_osc8);
    assert_eq!(c.ui.glow_style, "light");
    assert_eq!(c.report.summary.summarizer, Summarizer::Raw);
    assert_eq!(c.report.summary.model, Some("opus".to_owned()));
    assert_eq!(loaded.explain("store.notebook"), Some(&Origin::Env));
    assert_eq!(loaded.explain("ui.pager"), Some(&Origin::Env));
    let env_layer = loaded.layers.last().unwrap();
    assert_eq!(env_layer.origin, Origin::Env);
    assert_eq!(env_layer.keys.len(), 12);
    assert_eq!(Origin::Env.to_string(), "env");
}

#[test]
fn every_documented_env_key_is_a_real_key() {
    let template = toml::to_string(&Config::default()).unwrap();
    for (var, key) in tasq_core::config::ENV_KEYS {
        assert!(var.starts_with("TASQ_"), "{var}");
        let mut opts = LoadOptions::new("/nonexistent");
        // Any value a string key accepts; enums get their first variant.
        let value = match *key {
            "store.bookkeeper" => "auto",
            "work.worktree_manager" => "git",
            "launch.env" => "direnv",
            "ui.no_osc8" => "true",
            "report.summary.summarizer" => "raw",
            _ => "value",
        };
        opts.env = env(&[(var, value)]);
        let loaded = Config::load(&opts).unwrap_or_else(|e| panic!("{var}: {e}"));
        assert_eq!(loaded.explain(key), Some(&Origin::Env), "{var} sets {key}");
        let section = key.split('.').next().unwrap();
        assert!(
            template.contains(&format!("[{section}")),
            "{key} is a real section"
        );
    }
}

#[test]
fn empty_env_values_are_ignored() {
    let mut opts = LoadOptions::new("/nonexistent");
    opts.env = env(&[
        ("TASQ_NOTEBOOK", ""),
        ("TASQ_PROFILE", ""),
        ("TASQ_NO_OSC8", ""),
    ]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config, Config::default());
    assert_eq!(loaded.layers.len(), 1, "no env layer is recorded");
}

#[test]
fn boolean_env_values_are_parsed_leniently() {
    for (raw, expected) in [
        ("1", true),
        ("true", true),
        ("TRUE", true),
        ("yes", true),
        ("on", true),
        (" on ", true),
        ("0", false),
        ("false", false),
        ("no", false),
        ("off", false),
    ] {
        let mut opts = LoadOptions::new("/nonexistent");
        opts.env = env(&[("TASQ_NO_OSC8", raw)]);
        let loaded = Config::load(&opts).unwrap_or_else(|e| panic!("{raw:?}: {e}"));
        assert_eq!(loaded.config.ui.no_osc8, expected, "{raw:?}");
    }
    let mut opts = LoadOptions::new("/nonexistent");
    opts.env = env(&[("TASQ_NO_OSC8", "maybe")]);
    let err = Config::load(&opts).unwrap_err();
    assert!(
        matches!(&err, ConfigError::InvalidValue { origin: Origin::Env, key, value, expected }
            if key == "ui.no_osc8" && value == "maybe" && expected == "true or false"),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "env: ui.no_osc8=\"maybe\": expected true or false"
    );
}

#[test]
fn invalid_env_enum_value_is_reported_without_a_file() {
    let mut opts = LoadOptions::new("/nonexistent");
    opts.env = env(&[("TASQ_BOOKKEEPER", "manual")]);
    let err = Config::load(&opts).unwrap_err();
    match &err {
        ConfigError::Merge { message } => {
            assert!(message.contains("unknown variant `manual`"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        err.to_string()
            .starts_with("invalid configuration after merging layers: ")
    );
}

#[test]
fn overrides_win_over_everything_and_are_typed_by_key() {
    let sb = Sandbox::new();
    sb.write_project("[ui]\nno_osc8 = true\n");
    let mut opts = sb.opts();
    opts.env = env(&[("TASQ_NOTEBOOK", "env")]);
    opts.overrides = vec![
        ("store.notebook".to_owned(), "flag".to_owned()),
        ("ui.no_osc8".to_owned(), "false".to_owned()),
        (
            "workflow.statuses".to_owned(),
            "todo, doing,,done".to_owned(),
        ),
        ("workflow.default_status".to_owned(), "todo".to_owned()),
        ("launch.claude.prompt_file".to_owned(), "~/p.md".to_owned()),
        ("forge.gl.host".to_owned(), "gitlab.example.com".to_owned()),
        ("forge.gl.kind".to_owned(), "gitlab".to_owned()),
        ("ui.colors.title".to_owned(), "red".to_owned()),
    ];
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.notebook, "flag");
    assert!(!c.ui.no_osc8);
    assert_eq!(
        c.workflow.statuses,
        vec![status("todo"), status("doing"), status("done")]
    );
    assert_eq!(c.launch.claude.prompt_file, Some(sb.home.join("p.md")));
    assert_eq!(c.forge["gl"].host.as_deref(), Some("gitlab.example.com"));
    assert_eq!(c.forge["gl"].kind, Some(ForgeKind::Gitlab));
    assert_eq!(c.ui.colors["title"], "red");
    assert_eq!(loaded.explain("store.notebook"), Some(&Origin::Overrides));
    assert_eq!(loaded.explain("ui.no_osc8"), Some(&Origin::Overrides));
    assert_eq!(loaded.explain("forge.gl.host"), Some(&Origin::Overrides));
    assert_eq!(loaded.explain("forge.gl"), Some(&Origin::Overrides));
    assert_eq!(Origin::Overrides.to_string(), "--set");
    let last = loaded.layers.last().unwrap();
    assert_eq!(last.keys[0], "forge.gl.host");
    assert_eq!(last.keys[1], "forge.gl.kind");
}

#[test]
fn unknown_override_key_is_an_error() {
    let mut opts = LoadOptions::new("/nonexistent");
    for key in [
        "store.notbook",
        "nope",
        "store.notebook.deeper",
        "ui.colors",
        "forge.x.nope",
    ] {
        opts.overrides = vec![(key.to_owned(), "x".to_owned())];
        let err = Config::load(&opts).unwrap_err();
        match &err {
            ConfigError::UnknownKey { origin, key: k } => {
                assert_eq!(origin, &Origin::Overrides);
                assert_eq!(k, key);
            }
            ConfigError::InvalidValue {
                key: k, expected, ..
            } if key == "ui.colors" => {
                assert_eq!(k, key);
                assert_eq!(expected, "a single value, not a table");
            }
            other => panic!("{key}: {other:?}"),
        }
    }
    opts.overrides = vec![("nope".to_owned(), "x".to_owned())];
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "--set: unknown config key \"nope\""
    );
}

#[test]
fn override_can_set_a_key_a_file_made_scalar_or_table() {
    // A later layer may turn a table into a scalar and back; set_leaf must cope.
    let mut opts = LoadOptions::new("/nonexistent");
    opts.overrides = vec![("store".to_owned(), "x".to_owned())];
    let err = Config::load(&opts).unwrap_err();
    assert!(matches!(err, ConfigError::InvalidValue { .. }), "{err}");
}

// --- profiles --------------------------------------------------------------

#[test]
fn profile_overlay_from_flag_and_env() {
    let sb = Sandbox::new();
    let global = sb.write_global(
        "[store]\nnotebook = \"home\"\n\n[profile.work]\n[profile.work.store]\nnotebook = \"work\"\n\
         [profile.work.ui]\nglow_style = \"light\"\n\n[profile.play]\n[profile.play.store]\nnotebook = \"play\"\n",
    );
    let project = sb.write_project(
        "[ui]\nno_osc8 = true\n[profile.work.launch]\ndefault = \"shell\"\n[profile.other]\n",
    );
    let mut opts = sb.opts();
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(
        loaded.config.store.notebook, "home",
        "no profile without selection"
    );
    assert_eq!(loaded.profile, None);
    assert_eq!(loaded.profiles, vec!["other", "play", "work"]);
    assert_eq!(loaded.layers.len(), 3);

    opts.profile = Some("work".to_owned());
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.notebook, "work");
    assert_eq!(c.ui.glow_style, "light");
    assert!(c.ui.no_osc8, "project keys outside the profile still apply");
    assert_eq!(
        c.launch.default, "shell",
        "profile blocks from every file merge"
    );
    assert_eq!(loaded.profile.as_deref(), Some("work"));
    let work_in_global = Origin::Profile {
        name: "work".to_owned(),
        file: global.clone(),
    };
    let work_in_project = Origin::Profile {
        name: "work".to_owned(),
        file: project.clone(),
    };
    assert_eq!(loaded.explain("store.notebook"), Some(&work_in_global));
    assert_eq!(loaded.explain("launch.default"), Some(&work_in_project));
    assert_eq!(loaded.file_for("launch.default"), Some(project.as_path()));
    assert_eq!(
        work_in_global.to_string(),
        format!("[profile.work] in {}", global.display())
    );
    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(
        origins,
        vec![
            &Origin::Defaults,
            &Origin::File(global.clone()),
            &Origin::File(project.clone()),
            &work_in_global,
            &work_in_project,
        ]
    );

    // Env selects a profile too; the flag wins over the env.
    opts.profile = None;
    opts.env = env(&[("TASQ_PROFILE", "play")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "play");
    assert_eq!(loaded.config.launch.default, "claude");
    opts.profile = Some("work".to_owned());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "work");

    // Env overrides still beat the profile.
    opts.env = env(&[("TASQ_NOTEBOOK", "env")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.notebook, "env");
}

#[test]
fn unknown_profile_lists_the_available_ones() {
    let sb = Sandbox::new();
    sb.write_global("[profile.a]\n[profile.b]\n");
    let mut opts = sb.opts();
    opts.profile = Some("c".to_owned());
    let err = Config::load(&opts).unwrap_err();
    match &err {
        ConfigError::UnknownProfile { name, available } => {
            assert_eq!(name, "c");
            assert_eq!(available, &["a", "b"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        "profile \"c\" is not defined; available profiles: \"a\", \"b\""
    );
    opts.env = env(&[("TASQ_PROFILE", "zzz")]);
    opts.profile = None;
    let err = Config::load(&opts).unwrap_err();
    assert!(
        matches!(err, ConfigError::UnknownProfile { ref name, .. } if name == "zzz"),
        "{err}"
    );
}

#[test]
fn profile_bodies_are_validated_and_cannot_nest() {
    let sb = Sandbox::new();
    let path = sb.write_global("[profile.work.store]\nnotebok = \"x\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!("{}:2:1: unknown field `notebok`", path.display())),
        "{err}"
    );

    sb.write_global("[profile.work.profile.inner]\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("unknown field `profile`"), "{err}");

    sb.write_global("profile = 3\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("expected a map"), "{err}");
}

// --- paths -----------------------------------------------------------------

#[test]
fn tilde_is_expanded_in_path_values() {
    let sb = Sandbox::new();
    sb.write_global(
        "[work]\ndefault_project = \"~/code/x\"\n[launch.claude]\nprompt_file = \"~/p.md\"\n\
         [[source]]\nname = \"inbox\"\nkind = \"llm-bridge\"\ncommand = \"c\"\nprompt_file = \"~/inbox.md\"\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    let c = &loaded.config;
    assert_eq!(c.work.default_project, Some(sb.home.join("code/x")));
    assert_eq!(c.launch.claude.prompt_file, Some(sb.home.join("p.md")));
    assert_eq!(c.source[0].prompt_file, Some(sb.home.join("inbox.md")));

    // Without a home the paths are left as written.
    let mut opts = sb.opts();
    opts.home = None;
    opts.explicit_file = Some(sb.global_path());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(
        loaded.config.work.default_project,
        Some(PathBuf::from("~/code/x"))
    );
    assert_eq!(
        expand_tilde(Path::new("~/a"), Some(Path::new("/h"))),
        PathBuf::from("/h/a")
    );
}

// --- forges and sources ----------------------------------------------------

#[test]
fn forge_kind_and_host_are_filled_from_the_block_name() {
    let sb = Sandbox::new();
    sb.write_global(
        "[forge.gitlab]\ntoken_cmd = \"glab auth token\"\n[forge.github]\n\
         [forge.work]\nkind = \"gitlab\"\nhost = \"git.example.com\"\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    let f = &loaded.config.forge;
    assert_eq!(f["gitlab"].kind, Some(ForgeKind::Gitlab));
    assert_eq!(f["gitlab"].host.as_deref(), Some("gitlab.com"));
    assert_eq!(f["gitlab"].token_cmd.as_deref(), Some("glab auth token"));
    assert_eq!(f["github"].kind, Some(ForgeKind::Github));
    assert_eq!(f["github"].host.as_deref(), Some("github.com"));
    assert_eq!(f["github"].token_cmd, None);
    assert_eq!(f["work"].kind, Some(ForgeKind::Gitlab));
    assert_eq!(f["work"].host.as_deref(), Some("git.example.com"));
    assert_eq!(ForkindNames::all(), ["gitlab", "github"]);
}

/// Tiny helper keeping the `ForgeKind` string API under test.
struct ForkindNames;
impl ForkindNames {
    fn all() -> [&'static str; 2] {
        [ForgeKind::Gitlab.as_str(), ForgeKind::Github.as_str()]
    }
}

#[test]
fn forge_without_kind_and_unknown_name_is_an_error() {
    let sb = Sandbox::new();
    let path = sb.write_project("[forge.work]\nhost = \"git.example.com\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::ForgeKindRequired { name, origin } if name == "work" && origin == &Origin::File(path.clone())),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "forge.work.kind is required (set by {}): expected \"gitlab\" or \"github\"",
            path.display()
        )
    );
    assert_eq!(ForgeKind::from_name("bitbucket"), None);
    assert_eq!(ForgeKind::from_name("github"), Some(ForgeKind::Github));
    assert_eq!(ForgeKind::Gitlab.default_host(), "gitlab.com");
    assert_eq!(ForgeKind::Github.default_host(), "github.com");
}

#[test]
fn sources_need_the_keys_their_kind_requires() {
    let sb = Sandbox::new();
    let path = sb.write_project("[[source]]\nname = \"inbox\"\nkind = \"llm-bridge\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "source \"inbox\" of kind \"llm-bridge\" requires `command` (set by {})",
            path.display()
        )
    );

    sb.write_project("[[source]]\nname = \"mrs\"\nkind = \"github-review-requests\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::MissingSourceField { field: "forge", kind, .. } if kind == "github-review-requests"),
        "{err}"
    );

    sb.write_project("[[source]]\nname = \"mrs\"\nkind = \"gitlab-work-items\"\nforge = \"gl\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "source \"mrs\" references forge \"gl\" (set by {}): no [forge.gl] block is defined",
            path.display()
        )
    );

    sb.write_project(
        "[forge.gh]\nkind = \"github\"\n[[source]]\nname = \"mrs\"\nkind = \"gitlab-review-requests\"\nforge = \"gh\"\n",
    );
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::BadForgeReference { reason, .. }
        if reason == "source kind \"gitlab-review-requests\" needs a gitlab forge, but its kind is github"),
        "{err}"
    );

    sb.write_project(
        "[forge.gh]\nkind = \"github\"\n[[source]]\nname = \"issues\"\nkind = \"github-work-items\"\nforge = \"gh\"\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    let s = &loaded.config.source[0];
    assert_eq!(s.kind, SourceKind::GithubWorkItems);
    assert!(s.enabled, "enabled defaults to true");
    assert_eq!(s.tags, Vec::<String>::new());
    assert_eq!(s.status, None);
    assert_eq!(s.command, None);
    assert_eq!(loaded.explain("source"), Some(&Origin::File(path.clone())));
    assert_eq!(loaded.explain("source.0.name"), Some(&Origin::File(path)));
}

#[test]
fn source_kind_metadata() {
    use SourceKind::*;
    assert_eq!(GitlabReviewRequests.forge_kind(), Some(ForgeKind::Gitlab));
    assert_eq!(GitlabWorkItems.forge_kind(), Some(ForgeKind::Gitlab));
    assert_eq!(GithubReviewRequests.forge_kind(), Some(ForgeKind::Github));
    assert_eq!(GithubWorkItems.forge_kind(), Some(ForgeKind::Github));
    assert_eq!(LlmBridge.forge_kind(), None);
    for kind in [
        GitlabReviewRequests,
        GitlabWorkItems,
        GithubReviewRequests,
        GithubWorkItems,
        LlmBridge,
    ] {
        let text = format!("name = \"x\"\nkind = \"{}\"\n", kind.as_str());
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        let round: SourceKind = parsed["kind"].clone().try_into().unwrap();
        assert_eq!(round, kind);
    }
}

#[test]
fn source_list_is_replaced_not_appended_by_a_later_layer() {
    let sb = Sandbox::new();
    sb.write_global("[[source]]\nname = \"a\"\nkind = \"llm-bridge\"\ncommand = \"a\"\n");
    sb.write_project(
        "[[source]]\nname = \"b\"\nkind = \"llm-bridge\"\ncommand = \"b\"\nenabled = false\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.source.len(), 1);
    assert_eq!(loaded.config.source[0].name, "b");
    assert!(!loaded.config.source[0].enabled);
}

// --- the plan's example ----------------------------------------------------

#[test]
fn plan_section_4_6_example_loads_as_is() {
    let sb = Sandbox::new();
    let path = sb.write_project(&fixture("plan-4.6.toml"));
    let loaded = Config::load(&sb.opts()).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.notebook, "home");
    assert_eq!(c.store.bookkeeper, Bookkeeper::Auto);
    assert_eq!(c.workflow.statuses.len(), 5);
    assert_eq!(
        c.work.default_project,
        Some(sb.home.join("code/SF/silverfin_worspace/silverfin"))
    );
    assert_eq!(c.work.worktree_manager, WorktreeManager::Command);
    assert_eq!(
        c.work.worktree_command.as_deref(),
        Some("gwm create {new} {branch} --no-tmux -s")
    );
    assert_eq!(c.launch.default, "claude");
    assert_eq!(c.launch.env, EnvStrategy::Direnv);
    assert_eq!(c.forge.len(), 1);
    let gl = &c.forge["gitlab"];
    assert_eq!(gl.kind, Some(ForgeKind::Gitlab));
    assert_eq!(gl.host.as_deref(), Some("gitlab.silverfin.com"));
    assert_eq!(gl.token_cmd.as_deref(), Some("glab auth token"));
    assert_eq!(c.source.len(), 3);
    assert_eq!(c.source[0].kind, SourceKind::GitlabReviewRequests);
    assert_eq!(c.source[0].forge.as_deref(), Some("gitlab"));
    assert_eq!(c.source[0].tags, ["gitlab", "review-request"]);
    assert_eq!(c.source[0].status, Some(Status::READY));
    assert_eq!(c.source[1].kind, SourceKind::GitlabWorkItems);
    assert_eq!(c.source[1].status, Some(Status::LATER));
    assert_eq!(c.source[2].kind, SourceKind::LlmBridge);
    assert_eq!(
        c.source[2].command.as_deref(),
        Some("claude -p --output-format json")
    );
    assert_eq!(
        c.source[2].prompt_file,
        Some(sb.home.join(".config/tasq/prompts/inbox.md"))
    );
    assert!(c.source.iter().all(|s| s.enabled));
    assert_eq!(c.report.summary.summarizer, Summarizer::Llm);
    assert_eq!(c.report.summary.command, "claude -p --model sonnet");
    assert_eq!(c.report.summary.model, None);
    assert_eq!(loaded.file_for("store.notebook"), Some(path.as_path()));
    assert_eq!(
        loaded.explain("forge.gitlab.host"),
        Some(&Origin::File(path.clone()))
    );
    assert_eq!(loaded.explain("ui.pager"), Some(&Origin::Defaults));
    let keys = &loaded.layers[1].keys;
    assert!(
        keys.contains(&"forge.gitlab.token_cmd".to_owned()),
        "{keys:?}"
    );
    assert!(keys.contains(&"source".to_owned()), "{keys:?}");
    assert!(
        keys.contains(&"report.summary.command".to_owned()),
        "{keys:?}"
    );

    // The effective config serialises back to valid TOML that loads again.
    let text = toml::to_string(c).unwrap();
    let again: Config = toml::from_str(&text).unwrap();
    assert_eq!(&again, c);
}

// --- misc API --------------------------------------------------------------

#[test]
fn load_options_new_and_display_of_origins() {
    let opts = LoadOptions::new("/x");
    assert_eq!(opts.cwd, PathBuf::from("/x"));
    assert_eq!(
        opts,
        LoadOptions {
            cwd: PathBuf::from("/x"),
            ..LoadOptions::default()
        }
    );
    assert_eq!(Origin::Defaults.to_string(), "defaults");
    assert_eq!(
        Origin::File(PathBuf::from("/a/b.toml")).to_string(),
        "/a/b.toml"
    );
    assert_eq!(Origin::Defaults.path(), None);
    assert_eq!(
        Origin::File(PathBuf::from("/a")).path(),
        Some(Path::new("/a"))
    );
    assert_eq!(
        Origin::Profile {
            name: "p".into(),
            file: PathBuf::from("/a")
        }
        .path(),
        Some(Path::new("/a"))
    );
    assert_eq!(
        tasq_core::config::global_file(&LoadOptions::new("/x")),
        None
    );
    let mut opts = LoadOptions::new("/x");
    opts.home = Some(PathBuf::from("/home/me"));
    assert_eq!(
        tasq_core::config::global_file(&opts),
        Some(PathBuf::from("/home/me/.config/tasq/config.toml"))
    );
}

#[test]
fn explain_matches_prefixes_but_not_lookalike_keys() {
    let sb = Sandbox::new();
    let path = sb.write_project("[store]\nnotebook = \"x\"\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    let file = Origin::File(path);
    assert_eq!(loaded.explain("store.notebook"), Some(&file));
    assert_eq!(loaded.explain("store"), Some(&file));
    assert_eq!(loaded.explain("store.notebook.x"), Some(&file));
    assert_eq!(loaded.explain("store.notebooks"), None);
    assert_eq!(loaded.explain("stor"), None);
    assert_eq!(loaded.explain("store.kind"), Some(&Origin::Defaults));
}

// --- acceptance: plan requirements pinned ----------------------------------

#[test]
fn env_vars_named_by_the_plan_are_all_supported() {
    use tasq_core::config::{ENV_CONFIG, ENV_KEYS, ENV_PROFILE};
    let keyed: Vec<&str> = ENV_KEYS.iter().map(|(var, _)| *var).collect();
    for var in [
        "TASQ_NOTEBOOK",
        "TASQ_DEFAULT_PROJECT",
        "TASQ_PAGER",
        "TASQ_NO_OSC8",
        "TASQ_GLOW_STYLE",
        "TASQ_LAUNCHER",
    ] {
        assert!(keyed.contains(&var), "{var} missing from ENV_KEYS");
    }
    assert_eq!(ENV_PROFILE, "TASQ_PROFILE");
    assert_eq!(ENV_CONFIG, "TASQ_CONFIG");
    assert!(!keyed.contains(&ENV_PROFILE) && !keyed.contains(&ENV_CONFIG));
    let mut vars = keyed.clone();
    vars.sort_unstable();
    vars.dedup();
    assert_eq!(vars.len(), keyed.len(), "no duplicate variables");
}

#[test]
fn all_six_layers_stack_field_wise_in_documented_order() {
    let sb = Sandbox::new();
    // Every layer sets `store.notebook`; each also owns one private key.
    let global = sb.write_global(
        "[store]\nnotebook = \"global\"\nbookkeeper = \"nb\"\n\
         [profile.p.store]\nnotebook = \"profile\"\n[profile.p.ui]\nglow_style = \"light\"\n",
    );
    let project = sb.write_project("[store]\nnotebook = \"project\"\n[ui]\npager = \"more\"\n");
    let mut opts = sb.opts();
    opts.profile = Some("p".to_owned());
    opts.env = env(&[("TASQ_NOTEBOOK", "env"), ("TASQ_LAUNCHER", "shell")]);
    opts.overrides = vec![
        ("store.notebook".to_owned(), "override".to_owned()),
        ("launch.env".to_owned(), "inherit".to_owned()),
    ];
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.notebook, "override");
    assert_eq!(c.store.bookkeeper, Bookkeeper::Nb);
    assert_eq!(c.ui.pager, "more");
    assert_eq!(c.ui.glow_style, "light");
    assert_eq!(c.launch.default, "shell");
    assert_eq!(c.launch.env, EnvStrategy::Inherit);
    assert_eq!(c.store.kind, tasq_core::config::StoreKind::Nb);

    let profile = Origin::Profile {
        name: "p".to_owned(),
        file: global.clone(),
    };
    assert_eq!(loaded.explain("store.notebook"), Some(&Origin::Overrides));
    assert_eq!(
        loaded.explain("store.bookkeeper"),
        Some(&Origin::File(global.clone()))
    );
    assert_eq!(
        loaded.explain("ui.pager"),
        Some(&Origin::File(project.clone()))
    );
    assert_eq!(loaded.explain("ui.glow_style"), Some(&profile));
    assert_eq!(loaded.explain("launch.default"), Some(&Origin::Env));
    assert_eq!(loaded.explain("launch.env"), Some(&Origin::Overrides));
    assert_eq!(loaded.explain("store.kind"), Some(&Origin::Defaults));
    assert_eq!(loaded.file_for("store.notebook"), None);
    assert_eq!(loaded.file_for("ui.glow_style"), Some(global.as_path()));

    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(
        origins,
        vec![
            &Origin::Defaults,
            &Origin::File(global),
            &Origin::File(project),
            &profile,
            &Origin::Env,
            &Origin::Overrides,
        ]
    );
    // Dropping the overrides exposes the env value, and so on down the stack.
    opts.overrides.clear();
    assert_eq!(Config::load(&opts).unwrap().config.store.notebook, "env");
    opts.env.remove("TASQ_NOTEBOOK");
    assert_eq!(
        Config::load(&opts).unwrap().config.store.notebook,
        "profile"
    );
    opts.profile = None;
    assert_eq!(
        Config::load(&opts).unwrap().config.store.notebook,
        "project"
    );
    fs::remove_file(sb.project.join(".tasq.toml")).unwrap();
    assert_eq!(Config::load(&opts).unwrap().config.store.notebook, "global");
}

#[test]
fn command_worktree_manager_requires_a_command() {
    let sb = Sandbox::new();
    let file = sb.write_project("[work]\nworktree_manager = \"command\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "work.worktree_manager = \"command\" (set by {}) requires work.worktree_command, e.g. \"gwm create {{new}} {{branch}} --no-tmux -s\"",
            file.display()
        )
    );
    sb.write_project("[work]\nworktree_manager = \"command\"\nworktree_command = \"  \"\n");
    assert!(matches!(
        Config::load(&sb.opts()).unwrap_err(),
        ConfigError::WorktreeCommandRequired { .. }
    ));
    sb.write_project(
        "[work]\nworktree_manager = \"command\"\nworktree_command = \"gwm create {new} {branch} --no-tmux -s\"\n",
    );
    let c = Config::load(&sb.opts()).unwrap().config;
    assert_eq!(c.work.worktree_manager, WorktreeManager::Command);
    // The git manager never needs the command.
    sb.write_project("[work]\nworktree_manager = \"git\"\n");
    assert!(Config::load(&sb.opts()).is_ok());
}

#[test]
fn an_empty_table_is_recorded_as_a_leaf_key() {
    let sb = Sandbox::new();
    sb.write_project("[ui.colors]\n\n[forge]\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.layers[1].keys, vec!["forge", "ui.colors"]);
    assert!(
        loaded
            .explain("ui.colors")
            .is_some_and(|o| o.path().is_some())
    );
}
