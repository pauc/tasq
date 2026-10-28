//! Integration tests for layered configuration loading.
//!
//! Every test builds its own home and working directory under a `tempfile`
//! directory, so nothing depends on the machine running the tests.

#![allow(clippy::unwrap_used, reason = "test helpers panic on setup failures")]
#![allow(
    clippy::disallowed_methods,
    reason = "tests build fixtures on disk; the no-I/O rule is for the library"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tasq_core::config::{
    Config, ConfigError, DetailPosition, DueFormat, EnvStrategy, ForgeKind, KeySpec, LoadOptions,
    Origin, RepoConfig, SourceKind, Summarizer, WeekStart, expand_tilde, repo_configs,
};
use tasq_core::model::Status;
use tasq_core::theme::Preset;
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

    fn write_profile(&self, name: &str, text: &str) -> PathBuf {
        let dir = self.home.join(".config/tasq/profiles");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.toml"));
        fs::write(&path, text).unwrap();
        path
    }

    /// Writes the repository config `repos/<name>.toml` in the default
    /// config directory.
    fn write_repo(&self, name: &str, text: &str) -> PathBuf {
        let dir = self.home.join(".config/tasq/repos");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.toml"));
        fs::write(&path, text).unwrap();
        path
    }

    fn opts(&self) -> LoadOptions {
        LoadOptions {
            cwd: self.project.clone(),
            home: Some(self.home.clone()),
            env: BTreeMap::new(),
            config_dir: None,
            profile: None,
            repo: None,
            overrides: Vec::new(),
        }
    }

    /// [`Self::opts`] for the worktree commands of the repository whose
    /// main checkout is `main`.
    fn opts_for_repo(&self, main: &Path) -> LoadOptions {
        LoadOptions {
            repo: Some(main.to_path_buf()),
            ..self.opts()
        }
    }

    /// [`Self::opts`] with the profile `name` selected.
    fn opts_with_profile(&self, name: &str) -> LoadOptions {
        LoadOptions {
            profile: Some(name.to_owned()),
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
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Defaults));
    assert_eq!(loaded.file_for("store.name"), None);
    // An unset optional key next to set ones is not explained. (`[work]` is
    // an empty table by default, and an empty table explains its children.)
    assert_eq!(loaded.explain("report.summary.model"), None);
    assert_eq!(loaded.explain("no.such.key"), None);
}

#[test]
fn defaults_match_the_script() {
    let c = Config::default();
    assert_eq!(c.data_dir, None);
    assert_eq!(c.store.name, "default");
    assert_eq!(c.workflow.statuses, Status::DEFAULTS.to_vec());
    assert_eq!(c.workflow.default_status, Status::READY);
    assert_eq!(c.workflow.workflow(), tasq_core::model::Workflow::default());
    assert_eq!(c.work.default_project, None);
    assert!(!c.worktrees_enabled);
    assert_eq!(
        c.worktree.path,
        "{{project_parent}}/{{project_name}}-{{branch_safe}}"
    );
    assert_eq!(c.worktree.branch, "{{id}}-{{slug}}");
    assert_eq!(c.worktree.base, "origin/HEAD");
    assert_eq!(c.worktree.on_create, tasq_core::config::OnCreate::Auto);
    assert_eq!(c.worktree.on_done, tasq_core::config::OnDone::Release);
    assert_eq!(
        c.worktree.delete_branch,
        tasq_core::worktree::lifecycle::DeleteBranch::Safe
    );
    assert_eq!(c.worktree.ports, None);
    assert_eq!(c.launch.default, "claude");
    assert_eq!(c.launch.window, "auto");
    assert_eq!(c.launch.env, EnvStrategy::Direnv);
    assert_eq!(c.launch.herdr.placement, tasq_core::config::Placement::Auto);
    assert_eq!(c.launch.claude.prompt_file, None);
    assert_eq!(c.launch.log, None);
    assert_eq!(c.ui.pager, "less -RFX");
    assert!(!c.ui.no_osc8);
    assert_eq!(c.ui.glow_style, "dark");
    assert_eq!(c.ui.week_start, WeekStart::Monday);
    assert_eq!(c.ui.week_start.weekday(), chrono::Weekday::Mon);
    assert_eq!(c.ui.due_format, DueFormat::Relative);
    assert_eq!(c.ui.detail_position, DetailPosition::Right);
    assert!(c.ui.colors.is_empty());
    assert_eq!(c.ui.theme.preset, Preset::Dark);
    assert!(c.ui.theme.colors.is_empty());
    assert!(c.ui.keys.is_empty());
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
worktrees_enabled = false

[store]
name = \"default\"

[workflow]
statuses = [\"in-progress\", \"ready\", \"waiting\", \"blocked\", \"later\"]
default_status = \"ready\"

[work]

[launch]
default = \"claude\"
window = \"auto\"
env = \"direnv\"

[launch.claude]

[launch.herdr]
placement = \"auto\"

[ui]
pager = \"less -RFX\"
no_osc8 = false
glow_style = \"dark\"
week_start = \"monday\"
due_format = \"relative\"
detail_position = \"right\"

[ui.colors]

[ui.keys]

[ui.theme]
preset = \"dark\"

[ui.theme.colors]

[forge]

[report.summary]
summarizer = \"llm\"
command = \"claude -p\"

[hooks]
post-create = []
post-done = []
pre-launch = []

[plugins]
enabled = []
timeout = 2

[plugin]

[worktree]
path = \"{{project_parent}}/{{project_name}}-{{branch_safe}}\"
branch = \"{{id}}-{{slug}}\"
base = \"origin/HEAD\"
on_create = \"auto\"
on_done = \"release\"
delete_branch = \"safe\"
symlinks = []
copies = []
templates = []

[worktree.variables]

[worktree.hooks]
pre-create = []
post-create = []
pre-remove = []
post-remove = []
";
    assert_eq!(text, expected);
}

#[test]
fn herdr_placement_values_and_errors() {
    use tasq_core::config::Placement;
    for (value, placement) in [
        ("auto", Placement::Auto),
        ("workspace", Placement::Workspace),
        ("tab", Placement::Tab),
    ] {
        let sb = Sandbox::new();
        sb.write_global(&format!("[launch.herdr]\nplacement = \"{value}\"\n"));
        let c = Config::load(&sb.opts()).unwrap().config;
        assert_eq!(c.launch.herdr.placement, placement, "{value}");
    }
    let sb = Sandbox::new();
    sb.write_global("[launch.herdr]\nplacement = \"window\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.ends_with(
            "config.toml:2:13: unknown variant `window`, expected one of `auto`, `workspace`, `tab`"
        ),
        "{err}"
    );
}

#[test]
fn theme_preset_and_role_colors_from_every_layer() {
    // File: the preset and a role table under it.
    let sb = Sandbox::new();
    let global = sb.write_global(
        "[ui.theme]
preset = \"light\"

[ui.theme.colors]
focus = \"208\"
",
    );
    let project = sb.write_profile(
        "p",
        "[ui.theme.colors]
link = \"blue\"

[ui.colors]
ready = \"green\"
",
    );
    let mut opts = sb.opts();
    opts.profile = Some("p".to_owned());
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.ui.theme.preset, Preset::Light);
    assert_eq!(
        c.ui.theme.colors,
        BTreeMap::from([
            ("focus".to_owned(), "208".to_owned()),
            ("link".to_owned(), "blue".to_owned()),
        ])
    );
    assert_eq!(c.ui.colors["ready"], "green");
    assert_eq!(loaded.file_for("ui.theme.preset"), Some(global.as_path()));
    assert_eq!(
        loaded.file_for("ui.theme.colors.focus"),
        Some(global.as_path())
    );
    assert_eq!(
        loaded.file_for("ui.theme.colors.link"),
        Some(project.as_path())
    );
    // Env and --set win, --set over env; the role table takes any name.
    opts.env = env(&[("TASQ_THEME", "solarized")]);
    opts.overrides = vec![
        ("ui.theme.preset".to_owned(), "mono".to_owned()),
        ("ui.theme.colors.chip-bg".to_owned(), "none".to_owned()),
    ];
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.ui.theme.preset, Preset::Mono);
    assert_eq!(c.ui.theme.colors["chip-bg"], "none");
    assert_eq!(c.ui.theme.colors["focus"], "208");
    assert_eq!(loaded.explain("ui.theme.preset"), Some(&Origin::Overrides));
    assert_eq!(
        loaded.explain("ui.theme.colors.chip-bg"),
        Some(&Origin::Overrides)
    );
    // Every preset name parses; anything else is an error with a position.
    for preset in Preset::ALL {
        let sb = Sandbox::new();
        sb.write_global(&format!(
            "[ui.theme]
preset = \"{}\"
",
            preset.name()
        ));
        let c = Config::load(&sb.opts()).unwrap().config;
        assert_eq!(c.ui.theme.preset, preset);
    }
    let sb = Sandbox::new();
    sb.write_global(
        "[ui.theme]
preset = \"nord\"
",
    );
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.ends_with(
            "config.toml:2:10: unknown variant `nord`, expected one of `dark`, `light`, `solarized`, `gruvbox`, `mono`"
        ),
        "{err}"
    );
    let sb = Sandbox::new();
    sb.write_global(
        "[ui.theme]
colours = {}
",
    );
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("unknown field `colours`"), "{err}");
}

#[test]
fn detail_position_values_and_errors() {
    for (name, position) in [
        ("right", DetailPosition::Right),
        ("bottom", DetailPosition::Bottom),
    ] {
        let sb = Sandbox::new();
        sb.write_global(&format!("[ui]\ndetail_position = \"{name}\"\n"));
        let c = Config::load(&sb.opts()).unwrap().config;
        assert_eq!(c.ui.detail_position, position);
    }
    let sb = Sandbox::new();
    sb.write_global("[ui]\ndetail_position = \"left\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.ends_with("config.toml:2:19: unknown variant `left`, expected `right` or `bottom`"),
        "{err}"
    );
}

#[test]
fn due_format_values_and_errors() {
    for (name, format) in [
        ("relative", DueFormat::Relative),
        ("iso", DueFormat::Iso),
        ("both", DueFormat::Both),
    ] {
        let sb = Sandbox::new();
        sb.write_global(&format!("[ui]\ndue_format = \"{name}\"\n"));
        let c = Config::load(&sb.opts()).unwrap().config;
        assert_eq!(c.ui.due_format, format);
    }
    let sb = Sandbox::new();
    sb.write_global("[ui]\ndue_format = \"ago\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.ends_with(
            "config.toml:2:14: unknown variant `ago`, expected one of `relative`, `iso`, `both`"
        ),
        "{err}"
    );
}

#[test]
fn hooks_are_read_from_files_and_set_overrides() {
    use tasq_core::config::HooksConfig;
    let sb = Sandbox::new();
    sb.write_global(
        "[hooks]\npost-create = [\"tasq-notify\", \"~/bin/log-it --quiet\"]\npre-launch = [\"check-vpn\"]\n",
    );
    let mut opts = sb.opts();
    let c = Config::load(&opts).unwrap().config;
    assert_eq!(
        c.hooks.post_create,
        vec!["tasq-notify".to_owned(), "~/bin/log-it --quiet".to_owned()]
    );
    assert_eq!(c.hooks.post_done, Vec::<String>::new());
    assert_eq!(c.hooks.pre_launch, vec!["check-vpn".to_owned()]);
    assert!(!c.hooks.is_empty());
    assert_eq!(
        c.hooks.entries(),
        [
            ("post-create", c.hooks.post_create.as_slice()),
            ("post-done", c.hooks.post_done.as_slice()),
            ("pre-launch", c.hooks.pre_launch.as_slice()),
        ]
    );
    assert!(HooksConfig::default().is_empty());
    for hooks in [
        HooksConfig {
            post_create: vec!["x".to_owned()],
            ..HooksConfig::default()
        },
        HooksConfig {
            post_done: vec!["x".to_owned()],
            ..HooksConfig::default()
        },
        HooksConfig {
            pre_launch: vec!["x".to_owned()],
            ..HooksConfig::default()
        },
    ] {
        assert!(!hooks.is_empty(), "{hooks:?}");
    }

    // Arrays can be set from the command line, comma-separated.
    opts.overrides = vec![("hooks.post-done".to_owned(), "a, b".to_owned())];
    let c = Config::load(&opts).unwrap().config;
    assert_eq!(c.hooks.post_done, vec!["a".to_owned(), "b".to_owned()]);

    // An unknown hook name is an error naming the file.
    let file = sb.write_global("[hooks]\npost-sync = [\"x\"]\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!(
            "{}:2:1: unknown field `post-sync`",
            file.display()
        )),
        "{err}"
    );
}

#[test]
fn plugins_are_enabled_by_name_and_their_tables_load_unchecked() {
    let sb = Sandbox::new();
    sb.write_global(
        "[plugins]\nenabled = [\"git\"]\n\n[plugin.git]\ninterval = 60\nratio = 0.5\nremote = \"origin\"\n[plugin.git.deep]\nanything = [1, \"two\"]\n",
    );
    let profile = sb.write_profile(
        "p",
        "[plugin.git]\ninterval = 30\n\n[plugin.echo]\nlog = \"/tmp/x\"\n",
    );
    let mut opts = sb.opts_with_profile("p");
    let c = Config::load(&opts).unwrap().config;
    assert_eq!(c.plugins.enabled, vec!["git".to_owned()]);
    assert_eq!(c.plugins.timeout, 2, "the default");
    let git = &c.plugin["git"];
    assert_eq!(
        git["interval"].as_integer(),
        Some(30),
        "the profile wins key by key"
    );
    assert_eq!(git["remote"].as_str(), Some("origin"));
    assert_eq!(
        git["deep"]["anything"],
        toml::Value::Array(vec![1.into(), "two".into()])
    );
    assert_eq!(c.plugin["echo"]["log"].as_str(), Some("/tmp/x"));

    // Env and --set reach both tables. A plugin value takes the type the
    // key has in the layers below, and is a string when no layer set it.
    opts.env = env(&[(
        "TASQ_SET",
        "plugin.git.remote=1.0\nplugin.new.on=true\nplugin.git.deep.anything=a, b",
    )]);
    opts.overrides = vec![
        ("plugins.enabled".to_owned(), "git, echo".to_owned()),
        ("plugins.timeout".to_owned(), "7".to_owned()),
        ("plugin.git.interval".to_owned(), "5".to_owned()),
        ("plugin.git.name".to_owned(), "a b".to_owned()),
        ("plugin.git.ratio".to_owned(), "2".to_owned()),
    ];
    let c = Config::load(&opts).unwrap().config;
    assert_eq!(c.plugins.enabled, vec!["git".to_owned(), "echo".to_owned()]);
    assert_eq!(c.plugins.timeout, 7);
    assert_eq!(c.plugin["git"]["interval"].as_integer(), Some(5));
    assert_eq!(c.plugin["git"]["name"].as_str(), Some("a b"));
    assert_eq!(c.plugin["git"]["ratio"].as_float(), Some(2.0));
    assert_eq!(c.plugin["git"]["remote"].as_str(), Some("1.0"));
    assert_eq!(c.plugin["new"]["on"].as_str(), Some("true"));
    assert_eq!(
        c.plugin["git"]["deep"]["anything"],
        toml::Value::Array(vec!["a".into(), "b".into()])
    );
    opts.env = BTreeMap::new();
    opts.overrides = vec![("plugin.git.interval".to_owned(), "soon".to_owned())];
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "--set: plugin.git.interval=\"soon\": expected an integer"
    );
    opts.overrides = vec![("plugin.git.ratio".to_owned(), "half".to_owned())];
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "--set: plugin.git.ratio=\"half\": expected a number"
    );

    // A whole plugin table cannot be set from one value.
    opts.overrides = vec![("plugin.git".to_owned(), "x".to_owned())];
    assert!(matches!(
        Config::load(&opts).unwrap_err(),
        ConfigError::UnknownKey { .. }
    ));

    // Another profile carries its own plugin settings.
    fs::remove_file(profile).unwrap();
    sb.write_profile(
        "work",
        "[plugins]\nenabled = [\"echo\"]\n[plugin.echo]\nlog = \"w\"\n",
    );
    let mut opts = sb.opts();
    opts.profile = Some("work".to_owned());
    let c = Config::load(&opts).unwrap().config;
    assert_eq!(c.plugins.enabled, vec!["echo".to_owned()]);
    assert_eq!(c.plugin["echo"]["log"].as_str(), Some("w"));
    assert_eq!(c.plugin["git"]["interval"].as_integer(), Some(60));
}

#[test]
fn an_unknown_key_under_plugins_names_file_line_and_column() {
    let sb = Sandbox::new();
    let file = sb.write_global("[plugins]\nenabled = []\nenable = [\"git\"]\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!("{}:3:1: unknown field `enable`", file.display())),
        "{err}"
    );
}

#[test]
fn plugin_names_are_command_names() {
    for (name, reason) in [
        ("", "a plugin name must not be empty"),
        ("a/b", "a plugin name must not contain '/'"),
        ("-v", "a plugin name must not start with '-'"),
        ("a b", "a plugin name must not contain whitespace"),
        ("a\tb", "a plugin name must not contain whitespace"),
        (".", "a plugin name must not be '.' or '..'"),
        ("..", "a plugin name must not be '.' or '..'"),
        ("ok", "a plugin name must not be listed twice"),
    ] {
        let sb = Sandbox::new();
        let file = sb.write_global(&format!("[plugins]\nenabled = [\"ok\", {name:?}]\n"));
        assert_eq!(
            Config::load(&sb.opts()).unwrap_err().to_string(),
            format!(
                "plugins.enabled: {name:?} (set by {}): {reason}",
                file.display()
            )
        );
    }
}

#[test]
fn tasq_set_env_is_a_line_separated_set() {
    use tasq_core::config::{ENV_SET, parse_env_set};
    assert_eq!(ENV_SET, "TASQ_SET");
    assert_eq!(
        parse_env_set("store.name=work\n\n ui.no_osc8 =true\n").unwrap(),
        vec![
            ("store.name".to_owned(), "work".to_owned()),
            ("ui.no_osc8".to_owned(), "true".to_owned()),
        ]
    );
    assert_eq!(
        parse_env_set("a=b=c").unwrap(),
        vec![("a".to_owned(), "b=c".to_owned())]
    );
    assert_eq!(parse_env_set("").unwrap(), Vec::new());
    assert_eq!(
        parse_env_set("nonsense").unwrap_err().to_string(),
        "env: TASQ_SET=\"nonsense\": expected KEY=VALUE"
    );
    assert_eq!(
        parse_env_set("=x").unwrap_err().to_string(),
        "env: TASQ_SET=\"=x\": expected KEY=VALUE"
    );

    let sb = Sandbox::new();
    sb.write_global("[store]\nname = \"file\"\n");
    let mut opts = sb.opts();
    // TASQ_SET beats TASQ_STORE_NAME for the same key and is part of the env layer.
    opts.env = env(&[
        ("TASQ_STORE_NAME", "env"),
        ("TASQ_SET", "store.name=set\nui.no_osc8=1"),
    ]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "set");
    assert!(loaded.config.ui.no_osc8);
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Env));
    assert_eq!(loaded.explain("ui.no_osc8"), Some(&Origin::Env));
    // `--set` still wins.
    opts.overrides = vec![("store.name".to_owned(), "flag".to_owned())];
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "flag");
    // Errors name env, not --set.
    opts.env = env(&[("TASQ_SET", "store.nope=1")]);
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "env: unknown config key \"store.nope\""
    );
    opts.env = env(&[("TASQ_SET", "ui.no_osc8=maybe")]);
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "env: ui.no_osc8=\"maybe\": expected true or false"
    );
    // Empty is unset.
    opts.env = env(&[("TASQ_SET", "")]);
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "flag");
}

#[test]
fn empty_file_is_the_defaults() {
    let sb = Sandbox::new();
    let path = sb.write_global("");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(
        loaded.config,
        Config {
            data_dir: Some(sb.home.join(".local/share/tasq/stores")),
            ..Config::default()
        }
    );
    assert_eq!(loaded.layers[1].origin, Origin::File(path));
    assert_eq!(loaded.layers[1].keys, Vec::<String>::new());
}

// --- files -----------------------------------------------------------------

#[test]
fn ui_keys_take_a_string_or_a_list_and_merge_per_action() {
    let sb = Sandbox::new();
    sb.write_global("[ui.keys]\nquit = \"x\"\nsync = [\"a\", \"b\"]\nfilter = []\n");
    let project = sb.write_profile("p", "[ui.keys]\nquit = [\"y\"]\n");
    let mut opts = sb.opts_with_profile("p");
    opts.overrides = vec![(
        "ui.keys.launch-window".into(),
        "alt+enter, ctrl+enter".into(),
    )];
    let loaded = Config::load(&opts).unwrap();
    let keys = &loaded.config.ui.keys;
    assert_eq!(keys["quit"], KeySpec::Many(vec!["y".to_owned()]));
    assert_eq!(
        keys["sync"],
        KeySpec::Many(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(keys["filter"], KeySpec::Many(Vec::new()));
    assert_eq!(
        keys["launch-window"],
        KeySpec::Many(vec!["alt+enter".to_owned(), "ctrl+enter".to_owned()])
    );
    assert_eq!(keys.len(), 4);
    assert_eq!(loaded.file_for("ui.keys.quit"), Some(project.as_path()));
    assert_eq!(KeySpec::One("x".to_owned()).keys(), ["x".to_owned()]);
    assert_eq!(keys["filter"].keys(), Vec::<String>::new());
    assert_eq!(
        Config::load(&sb.opts()).unwrap().config.ui.keys["quit"],
        KeySpec::One("x".to_owned()),
        "a string stays a string when nothing above it is a list"
    );
}

#[test]
fn global_file_alone() {
    let sb = Sandbox::new();
    let path = sb.write_global("[store]\nname = \"work\"\n[ui]\nno_osc8 = true\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.name, "work");
    assert!(loaded.config.ui.no_osc8);
    assert_eq!(
        loaded.config.ui.pager, "less -RFX",
        "untouched keys keep defaults"
    );
    assert_eq!(
        loaded.explain("store.name"),
        Some(&Origin::File(path.clone()))
    );
    assert_eq!(loaded.file_for("store.name"), Some(path.as_path()));
    assert_eq!(loaded.explain("ui.glow_style"), Some(&Origin::Defaults));
    assert_eq!(loaded.layers[1].keys, vec!["store.name", "ui.no_osc8"]);
}

#[test]
fn xdg_config_home_replaces_dot_config() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nname = \"from-dot-config\"\n");
    let xdg = sb.home.join("xdg");
    fs::create_dir_all(xdg.join("tasq")).unwrap();
    fs::write(
        xdg.join("tasq/config.toml"),
        "[store]\nname = \"from-xdg\"\n",
    )
    .unwrap();
    let mut opts = sb.opts();
    opts.env = env(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "from-xdg");

    // An empty XDG_CONFIG_HOME is ignored, as the spec says.
    opts.env = env(&[("XDG_CONFIG_HOME", "")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "from-dot-config");
}

#[test]
fn without_home_there_is_no_global_file_but_xdg_still_works() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nname = \"global\"\n");
    let mut opts = sb.opts();
    opts.home = None;
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "default");

    let xdg = sb.home.join("xdg");
    fs::create_dir_all(xdg.join("tasq")).unwrap();
    fs::write(xdg.join("tasq/config.toml"), "[store]\nname = \"xdg\"\n").unwrap();
    opts.env = env(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "xdg");
}

/// `.tasq.toml` is never read (ADR-0031): not in the cwd, not above it,
/// not in the repository's main checkout.
#[test]
fn a_tasq_toml_has_no_effect() {
    let sb = Sandbox::new();
    for dir in [&sb.project, &sb.project.join("sub"), &sb.home] {
        fs::write(
            dir.join(".tasq.toml"),
            "[store]\nname = \"dotfile\"\n[worktree]\nbase = \"dotfile\"\n",
        )
        .unwrap();
    }
    let mut opts = sb.opts_for_repo(&sb.project);
    opts.cwd = sb.project.join("sub");
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "default");
    assert_eq!(loaded.config.worktree.base, "origin/HEAD");
    assert_eq!(loaded.layers.len(), 1);
}

#[test]
fn config_dir_replaces_the_default_directory_and_must_exist() {
    let sb = Sandbox::new();
    sb.write_global("[store]\nname = \"global\"\n[ui]\nno_osc8 = true\n");
    let named = sb.home.join("conf");
    fs::create_dir(&named).unwrap();
    let file = named.join("config.toml");
    fs::write(&file, "[store]\nname = \"named\"\n").unwrap();
    fs::create_dir(named.join("profiles")).unwrap();
    fs::write(
        named.join("profiles/p.toml"),
        "[ui]\nglow_style = \"light\"\n",
    )
    .unwrap();

    let mut opts = sb.opts_with_profile("p");
    opts.config_dir = Some(named.clone());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "named");
    assert_eq!(loaded.file_for("store.name"), Some(file.as_path()));
    assert!(
        !loaded.config.ui.no_osc8,
        "the default directory is not read"
    );
    assert_eq!(
        loaded.config.ui.glow_style, "light",
        "the profile comes from the named directory"
    );

    // TASQ_CONFIG_DIR does the same, with `~` expanded.
    opts.config_dir = None;
    opts.env = env(&[("TASQ_CONFIG_DIR", "~/conf")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.file_for("store.name"), Some(file.as_path()));

    // The flag wins over the variable, and the variable over XDG_CONFIG_HOME.
    opts.profile = None;
    let other = sb.home.join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("config.toml"), "[store]\nname = \"flag\"\n").unwrap();
    opts.config_dir = Some(other.clone());
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "flag");
    opts.config_dir = None;
    opts.env = env(&[
        ("TASQ_CONFIG_DIR", "~/conf"),
        ("XDG_CONFIG_HOME", other.to_str().unwrap()),
    ]);
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "named");

    // A named directory without a config.toml is an empty layer.
    fs::remove_file(&file).unwrap();
    opts.env = env(&[("TASQ_CONFIG_DIR", "~/conf")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "default");
    assert_eq!(loaded.layers.len(), 1, "the defaults only");

    opts.env = env(&[("TASQ_CONFIG_DIR", "~/missing")]);
    let err = Config::load(&opts).unwrap_err();
    match &err {
        ConfigError::MissingDir { dir, named_by } => {
            assert_eq!(dir, &sb.home.join("missing"));
            assert_eq!(*named_by, "TASQ_CONFIG_DIR");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        format!(
            "config directory {} does not exist or is not a directory (named by TASQ_CONFIG_DIR)",
            sb.home.join("missing").display()
        )
    );

    // A file is not a directory.
    opts.config_dir = Some(sb.global_path());
    let err = Config::load(&opts).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "config directory {} does not exist or is not a directory (named by --config-dir)",
            sb.global_path().display()
        )
    );

    // A relative directory starts at the injected cwd, not the process's.
    opts.cwd = sb.home.clone();
    opts.config_dir = Some(PathBuf::from("conf"));
    fs::write(&file, "[store]\nname = \"relative\"\n").unwrap();
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "relative");
    assert_eq!(loaded.file_for("store.name"), Some(file.as_path()));
    opts.cwd = sb.project.clone();
    fs::remove_file(&file).unwrap();

    // An empty TASQ_CONFIG_DIR is ignored.
    opts.config_dir = None;
    opts.env = env(&[("TASQ_CONFIG_DIR", "")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.store.name, "global");
}

#[test]
fn an_absent_default_config_dir_is_not_an_error() {
    let sb = Sandbox::new();
    fs::remove_dir_all(sb.home.join(".config/tasq")).unwrap();
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.store.name, "default");
}

#[test]
fn unreadable_file_names_the_file() {
    let sb = Sandbox::new();
    let path = sb.global_path();
    fs::create_dir(&path).unwrap();
    // A directory is not a file, so it is skipped as the global file.
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.layers.len(), 1, "only the defaults");

    // An unreadable regular file reports a read error.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::remove_dir(&path).unwrap();
        let unreadable = sb.write_global("[store]\nname = \"x\"\n");
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
    let path = sb.write_global("[store]\nname = \"x\"\nnotbook = \"typo\"\n");
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
                message.contains("`name`"),
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
    sb.write_global("[stor]\ncolour = \"x\"\n");
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
}

/// `[store]` has one key, `name`: a file that sets `kind`, `notebook` or
/// `sync_interval` (keys older configs had) fails like any other unknown
/// key, naming the file, line and column of the key and the keys that exist.
#[test]
fn old_store_keys_are_unknown_fields() {
    for (line, key) in [
        ("kind = \"x\"", "kind"),
        ("notebook = \"home\"", "notebook"),
        ("sync_interval = 3600", "sync_interval"),
    ] {
        let sb = Sandbox::new();
        let path = sb.write_global(&format!("[store]\nname = \"work\"\n{line}\n"));
        let err = Config::load(&sb.opts()).unwrap_err();
        match &err {
            ConfigError::Parse {
                file,
                line,
                column,
                message,
            } => {
                assert_eq!(file, &path, "{key}");
                assert_eq!((*line, *column), (3, 1), "{key}");
                assert!(
                    message.starts_with(&format!("unknown field `{key}`, expected `name`")),
                    "{key}: {message}"
                );
            }
            other => panic!("{key}: {other:?}"),
        }
        assert!(
            err.to_string().starts_with(&format!(
                "{}:3:1: unknown field `{key}`, expected `name`",
                path.display()
            )),
            "{key}: {err}"
        );
    }
}

/// A `TASQ_*` variable that maps to no key (`TASQ_NOTEBOOK`, which older
/// versions read) is ignored like any other unknown variable.
#[test]
fn an_unmapped_tasq_variable_is_ignored() {
    let sb = Sandbox::new();
    let mut opts = sb.opts();
    opts.env = env(&[("TASQ_NOTEBOOK", "home"), ("TASQ_SYNC_INTERVAL", "60")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config, Config::load(&sb.opts()).unwrap().config);
    assert_eq!(loaded.config.store.name, "default");
}

// --- data_dir and store.name ------------------------------------------------

/// Where the default store directory is: `$XDG_DATA_HOME/tasq/stores`,
/// else `~/.local/share/tasq/stores`, else unknown.
#[test]
fn data_dir_defaults_to_the_xdg_data_directory() {
    let sb = Sandbox::new();
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(
        loaded.config.data_dir,
        Some(sb.home.join(".local/share/tasq/stores"))
    );
    assert_eq!(loaded.config.store.name, "default");
    assert_eq!(loaded.explain("data_dir"), Some(&Origin::Defaults));
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Defaults));
    assert!(loaded.layers[0].keys.contains(&"data_dir".to_owned()));

    let mut opts = sb.opts();
    opts.env = env(&[("XDG_DATA_HOME", "/srv/data")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(
        loaded.config.data_dir,
        Some(PathBuf::from("/srv/data/tasq/stores"))
    );
    assert_eq!(loaded.explain("data_dir"), Some(&Origin::Defaults));

    // An empty XDG_DATA_HOME is unset, as the spec says.
    opts.env = env(&[("XDG_DATA_HOME", "")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(
        loaded.config.data_dir,
        Some(sb.home.join(".local/share/tasq/stores"))
    );

    // Without a home and without XDG_DATA_HOME nothing can be derived.
    opts.home = None;
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.data_dir, None);
    assert_eq!(loaded.explain("data_dir"), None);
    assert!(!loaded.layers[0].keys.contains(&"data_dir".to_owned()));
    opts.env = env(&[("XDG_DATA_HOME", "/srv/data")]);
    assert_eq!(
        Config::load(&opts).unwrap().config.data_dir,
        Some(PathBuf::from("/srv/data/tasq/stores"))
    );
}

#[test]
fn data_dir_and_store_name_come_from_every_layer() {
    let sb = Sandbox::new();
    let global = sb.write_global("data_dir = \"~/tasks\"\n\n[store]\nname = \"global\"\n");
    sb.write_profile(
        "work",
        "data_dir = \"/srv/work\"\n[store]\nname = \"work\"\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.config.data_dir, Some(sb.home.join("tasks")));
    assert_eq!(loaded.config.store.name, "global");
    assert_eq!(
        loaded.explain("data_dir"),
        Some(&Origin::File(global.clone()))
    );
    assert_eq!(loaded.file_for("store.name"), Some(global.as_path()));

    let mut opts = sb.opts();
    opts.profile = Some("work".to_owned());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.data_dir, Some(PathBuf::from("/srv/work")));
    assert_eq!(loaded.config.store.name, "work");

    opts.env = env(&[("TASQ_DATA_DIR", "~/env"), ("TASQ_STORE_NAME", "env")]);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.data_dir, Some(sb.home.join("env")));
    assert_eq!(loaded.config.store.name, "env");
    assert_eq!(loaded.explain("data_dir"), Some(&Origin::Env));
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Env));

    opts.overrides = vec![
        ("data_dir".to_owned(), "/tmp/flag".to_owned()),
        ("store.name".to_owned(), "flag".to_owned()),
    ];
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.data_dir, Some(PathBuf::from("/tmp/flag")));
    assert_eq!(loaded.config.store.name, "flag");
    assert_eq!(loaded.explain("data_dir"), Some(&Origin::Overrides));
}

/// A store name is one directory component: a mistyped one makes a
/// harmless empty directory under the stores root, so nothing may escape it.
#[test]
fn store_name_must_be_a_single_directory_name() {
    for (name, reason) in [
        ("", "a store name must not be empty"),
        ("a/b", "a store name must not contain '/'"),
        ("..", "a store name must not be '.' or '..'"),
        (".", "a store name must not be '.' or '..'"),
    ] {
        let sb = Sandbox::new();
        let file = sb.write_global(&format!("[store]\nname = {name:?}\n"));
        let err = Config::load(&sb.opts()).unwrap_err();
        match &err {
            ConfigError::BadStoreName {
                name: n,
                reason: r,
                origin,
            } => {
                assert_eq!(n, name);
                assert_eq!(*r, reason);
                assert_eq!(origin, &Origin::File(file.clone()));
            }
            other => panic!("{name:?}: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            format!(
                "store.name = {name:?} (set by {}): {reason}",
                file.display()
            )
        );
    }
    let mut opts = LoadOptions::new("/nonexistent");
    opts.env = env(&[("TASQ_STORE_NAME", "x/y")]);
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "store.name = \"x/y\" (set by env): a store name must not contain '/'"
    );
    opts.env = env(&[("TASQ_STORE_NAME", "work.2026")]);
    assert_eq!(
        Config::load(&opts).unwrap().config.store.name,
        "work.2026",
        "dots inside a name are fine"
    );
}

#[test]
fn syntax_error_names_file_and_line() {
    let sb = Sandbox::new();
    let path = sb.write_global("[store]\nname = \n");
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
    sb.write_global("[launch]\nenv = \"manual\"\n");
    let err = Config::load(&sb.opts()).unwrap_err().to_string();
    assert!(err.contains("unknown variant `manual`"), "{err}");
    assert!(err.contains("expected `inherit` or `direnv`"), "{err}");

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

    // A profile shrinking the list breaks the global default.
    sb.write_global("[workflow]\ndefault_status = \"later\"\n");
    sb.write_profile("p", "[workflow]\nstatuses = [\"todo\", \"doing\"]\n");
    let err = Config::load(&sb.opts_with_profile("p")).unwrap_err();
    assert!(
        matches!(&err, ConfigError::DefaultStatusNotInWorkflow { origin, .. } if origin == &Origin::File(global.clone())),
        "{err}"
    );

    // Shrinking the list without naming a default breaks the built-in one.
    fs::remove_file(&global).unwrap();
    let err = Config::load(&sb.opts_with_profile("p")).unwrap_err();
    assert!(
        matches!(&err, ConfigError::DefaultStatusNotInWorkflow { origin: Origin::Defaults, status, .. } if status == "ready"),
        "{err}"
    );

    sb.write_profile(
        "p",
        "[workflow]\nstatuses = [\"todo\", \"doing\"]\ndefault_status = \"todo\"\n",
    );
    let loaded = Config::load(&sb.opts_with_profile("p")).unwrap();
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
    sb.write_global("[store]\nname = \"global\"\n");
    sb.write_profile("p", "[store]\nname = \"profile\"\n[ui]\npager = \"more\"\n");
    let mut opts = sb.opts_with_profile("p");
    opts.env = env(&[
        ("TASQ_STORE_NAME", "env"),
        ("TASQ_DATA_DIR", "~/data"),
        ("TASQ_DEFAULT_PROJECT", "~/code/x"),
        ("TASQ_WORKTREES_ENABLED", "true"),
        (
            "TASQ_WORKTREE_PATH",
            "{{project}}/.worktrees/{{branch_safe}}",
        ),
        ("TASQ_WORKTREE_BRANCH", "{{slug}}"),
        ("TASQ_WORKTREE_BASE", "origin/main"),
        ("TASQ_WORKTREE_ON_CREATE", "never"),
        ("TASQ_WORKTREE_ON_DONE", "keep"),
        ("TASQ_WORKTREE_DELETE_BRANCH", "never"),
        ("TASQ_LAUNCHER", "tmux"),
        ("TASQ_LAUNCH_WINDOW", "herdr"),
        ("TASQ_LAUNCH_ENV", "inherit"),
        ("TASQ_LAUNCH_LOG", "~/tasq/launch.log"),
        ("TASQ_HERDR_PLACEMENT", "workspace"),
        ("TASQ_PAGER", "bat -p"),
        ("TASQ_NO_OSC8", "1"),
        ("TASQ_GLOW_STYLE", "light"),
        ("TASQ_WEEK_START", "saturday"),
        ("TASQ_DUE_FORMAT", "both"),
        ("TASQ_DETAIL_POSITION", "bottom"),
        ("TASQ_THEME", "gruvbox"),
        ("TASQ_SUMMARIZER", "raw"),
        ("TASQ_SUMMARY_MODEL", "opus"),
        ("TASQ_SUMMARY_COMMAND", "llm -m gpt"),
        ("TASQ_SUMMARY_PROMPT_FILE", "~/prompts/standup.md"),
        ("UNRELATED", "ignored"),
    ]);
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.name, "env");
    assert_eq!(c.data_dir, Some(sb.home.join("data")));
    assert_eq!(c.work.default_project, Some(sb.home.join("code/x")));
    assert!(c.worktrees_enabled);
    assert_eq!(c.worktree.path, "{{project}}/.worktrees/{{branch_safe}}");
    assert_eq!(c.worktree.branch, "{{slug}}");
    assert_eq!(c.worktree.base, "origin/main");
    assert_eq!(c.worktree.on_create, tasq_core::config::OnCreate::Never);
    assert_eq!(c.worktree.on_done, tasq_core::config::OnDone::Keep);
    assert_eq!(
        c.worktree.delete_branch,
        tasq_core::worktree::lifecycle::DeleteBranch::Never
    );
    assert_eq!(c.launch.default, "tmux");
    assert_eq!(c.launch.window, "herdr");
    assert_eq!(c.launch.env, EnvStrategy::Inherit);
    assert_eq!(c.launch.log, Some(sb.home.join("tasq/launch.log")));
    assert_eq!(
        c.launch.herdr.placement,
        tasq_core::config::Placement::Workspace
    );
    assert_eq!(c.ui.pager, "bat -p");
    assert!(c.ui.no_osc8);
    assert_eq!(c.ui.glow_style, "light");
    assert_eq!(c.ui.week_start, WeekStart::Saturday);
    assert_eq!(c.ui.week_start.weekday(), chrono::Weekday::Sat);
    assert_eq!(c.ui.due_format, DueFormat::Both);
    assert_eq!(c.ui.detail_position, DetailPosition::Bottom);
    assert_eq!(c.ui.theme.preset, Preset::Gruvbox);
    assert_eq!(c.report.summary.summarizer, Summarizer::Raw);
    assert_eq!(c.report.summary.model, Some("opus".to_owned()));
    assert_eq!(c.report.summary.command, "llm -m gpt");
    assert_eq!(
        c.report.summary.prompt_file,
        Some(sb.home.join("prompts/standup.md"))
    );
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Env));
    assert_eq!(loaded.explain("ui.pager"), Some(&Origin::Env));
    let env_layer = loaded.layers.last().unwrap();
    assert_eq!(env_layer.origin, Origin::Env);
    assert_eq!(env_layer.keys.len(), 26);
    assert_eq!(Origin::Env.to_string(), "env");
}

#[test]
fn worktrees_enabled_is_a_top_level_switch() {
    let sb = Sandbox::new();
    let file = sb.write_global("worktrees_enabled = true\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert!(loaded.config.worktrees_enabled);
    assert_eq!(
        loaded.explain("worktrees_enabled"),
        Some(&Origin::File(file))
    );
}

#[test]
fn worktree_enabled_names_the_top_level_switch() {
    for value in ["true", "false", "\"yes\""] {
        let sb = Sandbox::new();
        let file = sb.write_global(&format!("[worktree]\nbase = \"main\"\nenabled = {value}\n"));
        let err = Config::load(&sb.opts()).unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "{}:3:11: worktree.enabled moved to the top level: \
                 set worktrees_enabled above any table (docs/config.md, \"Worktrees\")",
                file.display()
            )
        );
    }
}

#[test]
fn worktree_enabled_from_set_or_env_names_the_top_level_switch() {
    let sb = Sandbox::new();
    let mut opts = sb.opts();
    opts.overrides = vec![("worktree.enabled".to_owned(), "true".to_owned())];
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "--set: worktree.enabled moved to the top level: \
         set worktrees_enabled (docs/config.md, \"Worktrees\")"
    );
    let mut opts = sb.opts();
    opts.env = env(&[("TASQ_SET", "worktree.enabled=1")]);
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        "env: worktree.enabled moved to the top level: \
         set worktrees_enabled (docs/config.md, \"Worktrees\")"
    );
}

#[test]
fn the_old_worktree_enabled_env_var_is_ignored() {
    let sb = Sandbox::new();
    let mut opts = sb.opts();
    opts.env = env(&[
        ("TASQ_WORKTREE_ENABLED", "true"),
        ("TASQ_WORKTREES", "true"),
    ]);
    let loaded = Config::load(&opts).unwrap();
    assert!(!loaded.config.worktrees_enabled);
    assert_eq!(loaded.layers.last().unwrap().origin, Origin::Defaults);
}

#[test]
fn every_documented_env_key_is_a_real_key() {
    let template = toml::to_string(&Config::default()).unwrap();
    for (var, key) in tasq_core::config::ENV_KEYS {
        assert!(var.starts_with("TASQ_"), "{var}");
        let mut opts = LoadOptions::new("/nonexistent");
        // Any value a string key accepts; enums get their first variant.
        let value = match *key {
            "launch.herdr.placement" => "auto",
            "worktree.on_create" | "worktree.delete_branch" => "never",
            "worktree.on_done" => "keep",
            "launch.env" => "direnv",
            "ui.no_osc8" | "worktrees_enabled" => "true",
            "report.summary.summarizer" => "raw",
            "ui.week_start" => "sunday",
            "ui.due_format" => "iso",
            "ui.detail_position" => "bottom",
            "ui.theme.preset" => "mono",
            _ => "value",
        };
        opts.env = env(&[(var, value)]);
        let loaded = Config::load(&opts).unwrap_or_else(|e| panic!("{var}: {e}"));
        assert_eq!(loaded.explain(key), Some(&Origin::Env), "{var} sets {key}");
        if let Some((section, _)) = key.split_once('.') {
            assert!(
                template.contains(&format!("[{section}")),
                "{key} is a real section"
            );
        }
    }
}

#[test]
fn empty_env_values_are_ignored() {
    let mut opts = LoadOptions::new("/nonexistent");
    opts.env = env(&[
        ("TASQ_STORE_NAME", ""),
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
    opts.env = env(&[("TASQ_LAUNCH_ENV", "manual")]);
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
    sb.write_global("[ui]\nno_osc8 = true\n");
    let mut opts = sb.opts();
    opts.env = env(&[("TASQ_STORE_NAME", "env")]);
    opts.overrides = vec![
        ("store.name".to_owned(), "flag".to_owned()),
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
    assert_eq!(c.store.name, "flag");
    assert!(!c.ui.no_osc8);
    assert_eq!(
        c.workflow.statuses,
        vec![status("todo"), status("doing"), status("done")]
    );
    assert_eq!(c.launch.claude.prompt_file, Some(sb.home.join("p.md")));
    assert_eq!(c.forge["gl"].host.as_deref(), Some("gitlab.example.com"));
    assert_eq!(c.forge["gl"].kind, Some(ForgeKind::Gitlab));
    assert_eq!(c.ui.colors["title"], "red");
    assert_eq!(loaded.explain("store.name"), Some(&Origin::Overrides));
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
        "store.nmae",
        "nope",
        "store.name.deeper",
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
fn a_selected_profile_file_applies_over_the_global_file() {
    let sb = Sandbox::new();
    let global = sb.write_global("[store]\nname = \"home\"\n[ui]\nno_osc8 = true\n");
    let work = sb.write_profile(
        "work",
        "[store]\nname = \"work\"\n[ui]\nglow_style = \"light\"\n",
    );
    sb.write_profile("play", "[store]\nname = \"play\"\n");
    let mut opts = sb.opts();
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(
        loaded.config.store.name, "home",
        "no profile without selection"
    );
    assert_eq!(loaded.profile, None);
    assert_eq!(loaded.profiles, vec!["play", "work"]);
    assert_eq!(loaded.layers.len(), 2);

    opts.profile = Some("work".to_owned());
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.name, "work");
    assert_eq!(c.ui.glow_style, "light");
    assert!(c.ui.no_osc8, "global keys outside the profile still apply");
    assert_eq!(loaded.profile.as_deref(), Some("work"));
    let work_origin = Origin::Profile {
        name: "work".to_owned(),
        file: work.clone(),
    };
    assert_eq!(loaded.explain("store.name"), Some(&work_origin));
    assert_eq!(loaded.file_for("ui.glow_style"), Some(work.as_path()));
    assert_eq!(
        work_origin.to_string(),
        format!("profile work ({})", work.display())
    );
    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(
        origins,
        vec![&Origin::Defaults, &Origin::File(global), &work_origin]
    );

    // Env selects a profile too; the flag wins over the env.
    opts.profile = None;
    opts.env = env(&[("TASQ_PROFILE", "play")]);
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "play");
    opts.profile = Some("work".to_owned());
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "work");

    // Env overrides still beat the profile.
    opts.env = env(&[("TASQ_STORE_NAME", "env")]);
    assert_eq!(Config::load(&opts).unwrap().config.store.name, "env");
}

/// Only `*.toml` files directly in `profiles/` are profiles; a profile
/// file in the directory named by `--config-dir` is found there.
#[test]
fn profile_names_come_from_the_toml_files_of_the_profiles_directory() {
    let sb = Sandbox::new();
    sb.write_profile("b", "");
    sb.write_profile("a", "");
    let dir = sb.home.join(".config/tasq/profiles");
    fs::write(dir.join("notes.md"), "").unwrap();
    fs::create_dir_all(dir.join("nested.toml")).unwrap();
    assert_eq!(Config::load(&sb.opts()).unwrap().profiles, vec!["a", "b"]);

    let other = sb.home.join("other");
    fs::create_dir_all(other.join("profiles")).unwrap();
    let file = other.join("profiles/x.toml");
    fs::write(&file, "[store]\nname = \"x\"\n").unwrap();
    let mut opts = sb.opts();
    opts.config_dir = Some(other);
    opts.profile = Some("x".to_owned());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.profiles, vec!["x"]);
    assert_eq!(loaded.file_for("store.name"), Some(file.as_path()));
}

#[test]
fn unknown_profile_lists_the_available_ones() {
    let sb = Sandbox::new();
    sb.write_profile("a", "");
    sb.write_profile("b", "");
    let mut opts = sb.opts();
    opts.profile = Some("c".to_owned());
    let err = Config::load(&opts).unwrap_err();
    match &err {
        ConfigError::UnknownProfile {
            name, available, ..
        } => {
            assert_eq!(name, "c");
            assert_eq!(available, &["a", "b"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        format!(
            "profile \"c\" is not defined: no file {}; available profiles: \"a\", \"b\"",
            sb.home.join(".config/tasq/profiles/c.toml").display()
        )
    );
    // A name is a file stem in `profiles/`, never a path.
    fs::write(sb.home.join(".config/tasq/escape.toml"), "").unwrap();
    opts.profile = Some("../escape".to_owned());
    assert!(matches!(
        Config::load(&opts).unwrap_err(),
        ConfigError::UnknownProfile { .. }
    ));
    opts.env = env(&[("TASQ_PROFILE", "zzz")]);
    opts.profile = None;
    let err = Config::load(&opts).unwrap_err();
    assert!(
        matches!(err, ConfigError::UnknownProfile { ref name, .. } if name == "zzz"),
        "{err}"
    );
}

#[test]
fn profile_files_are_validated_like_any_config_file() {
    let sb = Sandbox::new();
    let path = sb.write_profile("work", "[store]\nnotebok = \"x\"\n");
    let mut opts = sb.opts();
    opts.profile = Some("work".to_owned());
    let err = Config::load(&opts).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!("{}:2:1: unknown field `notebok`", path.display())),
        "{err}"
    );
}

/// A `[profile.<name>]` table pasted into its own profile file: the file
/// is the profile, so the fix is to drop the header and the prefix, not to
/// move it to the file it is already in.
#[test]
fn an_inline_profile_in_a_profile_file_says_to_drop_the_header() {
    let sb = Sandbox::new();
    let file = sb.write_profile(
        "work",
        "[profile.work]\nworktrees_enabled = true\n\n[profile.work.store]\nname = \"w\"\n",
    );
    let mut opts = sb.opts();
    opts.profile = Some("work".to_owned());
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        format!(
            "{}:1:10: a profile file is the profile itself: remove the [profile.work] header \
             and the profile.work. prefix of its tables",
            file.display()
        )
    );
    // Another profile's table belongs in that profile's file.
    fs::write(&file, "profile.q.ui.pager = \"x\"\n").unwrap();
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        format!(
            "{}:1:9: profiles are files now: move [profile.q] to {}",
            file.display(),
            sb.home.join(".config/tasq/profiles/q.toml").display()
        )
    );
}

/// A `[profile.<name>]` table in `config.toml` fails at its name with the
/// file to move it to (ADR-0031), not as an unknown key.
#[test]
fn an_inline_profile_names_the_file_to_move_it_to() {
    let sb = Sandbox::new();
    let target = sb.home.join(".config/tasq/profiles/work.toml");
    let global = sb.write_global("[store]\nname = \"x\"\n\n[profile.work.store]\nname = \"w\"\n");
    assert_eq!(
        Config::load(&sb.opts()).unwrap_err().to_string(),
        format!(
            "{}:4:10: profiles are files now: move [profile.work] to {}",
            global.display(),
            target.display()
        )
    );
    fs::remove_file(&global).unwrap();

    let global = sb.write_global("[profile.a]\n[profile.work]\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::InlineProfile { file, line: 1, column: 10, name, .. }
            if file == &global && name == "a"),
        "{err:?}"
    );
    fs::remove_file(&global).unwrap();

    // With `--config-dir`, the target is in that directory.
    let other = sb.home.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("config.toml"), "profile.q.ui.pager = \"x\"\n").unwrap();
    let mut opts = sb.opts();
    opts.config_dir = Some(other.clone());
    assert_eq!(
        Config::load(&opts).unwrap_err().to_string(),
        format!(
            "{}:1:9: profiles are files now: move [profile.q] to {}",
            other.join("config.toml").display(),
            other.join("profiles/q.toml").display()
        )
    );

    // The first profile in the file, not the first by name.
    let global = sb.write_global("[profile.zed]\n\n[profile.alpha]\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::InlineProfile { line: 1, name, .. } if name == "zed"),
        "{err:?}"
    );

    // A `profile` key that is not a table of tables is an unknown key like any other.
    for text in ["profile = 3\n", "profile = { x = 1 }\n"] {
        fs::write(&global, text).unwrap();
        let err = Config::load(&sb.opts()).unwrap_err().to_string();
        assert!(
            err.contains("1:1: unknown field `profile`"),
            "{text}: {err}"
        );
    }
}

// --- repository configs ----------------------------------------------------

/// A repository config applies when the worktree commands ask for its main
/// checkout, whatever the cwd and the profile (ADR-0031), and never to
/// another repository or to a load that names none.
#[test]
fn a_repository_config_applies_to_its_main_checkout_only() {
    let sb = Sandbox::new();
    sb.write_global("[worktree]\nbase = \"origin/main\"\nsymlinks = [\"global\"]\n");
    sb.write_profile("p", "[worktree]\nsymlinks = [\"profile\"]\n");
    let repo = sb.write_repo(
        "project",
        "path = \"~/code/project\"\n\n[worktree]\nsymlinks = [\"repo\"]\n",
    );
    sb.write_repo(
        "other",
        "path = \"~/code/other\"\n[worktree]\ncopies = [\"x\"]\n",
    );

    let mut opts = sb.opts_for_repo(&sb.project);
    opts.cwd = sb.home.join("elsewhere");
    let loaded = Config::load(&opts).unwrap();
    let w = &loaded.config.worktree;
    assert_eq!(w.symlinks, vec!["repo"]);
    assert_eq!(w.base, "origin/main", "keys the file leaves alone stay");
    assert_eq!(
        w.copies,
        Vec::<String>::new(),
        "the other repository's file is not merged"
    );
    assert_eq!(loaded.repo.as_deref(), Some(repo.as_path()));
    let origin = Origin::Repo(repo.clone());
    assert_eq!(loaded.explain("worktree.symlinks"), Some(&origin));
    assert_eq!(loaded.file_for("worktree.symlinks"), Some(repo.as_path()));
    assert_eq!(origin.to_string(), repo.display().to_string());
    assert_eq!(
        loaded.layers.last().unwrap().keys,
        vec!["worktree.symlinks"]
    );

    // It wins over the selected profile.
    opts.profile = Some("p".to_owned());
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.worktree.symlinks, vec!["repo"]);
    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(origins.len(), 4);
    assert_eq!(origins[3], &origin);

    // Another repository, a subdirectory of this one, or no repository at all.
    for repo in [
        Some(sb.home.join("code/third")),
        Some(sb.project.join("sub")),
        None,
    ] {
        let mut opts = sb.opts();
        opts.repo = repo;
        let loaded = Config::load(&opts).unwrap();
        assert_eq!(loaded.config.worktree.symlinks, vec!["global"]);
        assert_eq!(loaded.repo, None);
    }
}

/// A repository config holds `path` and `[worktree]`, nothing else; the
/// switch `worktrees_enabled` is not a repository setting.
#[test]
fn a_repository_config_holds_path_and_worktree_only() {
    let sb = Sandbox::new();
    let opts = sb.opts_for_repo(&sb.project);
    let err = |text: &str| {
        let file = sb.write_repo("project", text);
        let err = Config::load(&opts).unwrap_err().to_string();
        fs::remove_file(&file).unwrap();
        err.replace(&file.display().to_string(), "<file>")
    };
    assert_eq!(
        err("path = \"~/code/project\"\n[store]\nname = \"x\"\n"),
        "<file>:2:2: unknown field `store`, expected `path` or `worktree`"
    );
    assert_eq!(
        err("path = \"~/code/project\"\nworktrees_enabled = true\n"),
        "<file>:2:21: worktrees_enabled is not a repository setting: \
         set it in config.toml or a profile (docs/config.md, \"Worktrees\")"
    );
    assert_eq!(
        err("path = \"~/code/project\"\n[worktree]\nbase = \"main\"\nenabled = true\n"),
        "<file>:4:11: worktrees_enabled is not a repository setting: \
         set it in config.toml or a profile (docs/config.md, \"Worktrees\")"
    );
    assert_eq!(
        err("[worktree]\nbase = \"main\"\n"),
        "<file>:1:1: missing field `path`"
    );
    assert_eq!(
        err("path = \"code/project\"\n"),
        "<file>:1:8: path must be absolute or start with ~/, the main checkout of a repository"
    );
    assert_eq!(
        err("path = \"~/code/project\"\n[worktree]\nbranch = 3\n"),
        "<file>:3:10: invalid type: integer `3`, expected a string"
    );
}

/// Two repository configs with one `path` fail on load, naming both, even
/// when another repository is asked about.
#[test]
fn two_repository_configs_with_one_path_name_both_files() {
    let sb = Sandbox::new();
    let a = sb.write_repo("a", "path = \"~/code/project\"\n");
    let b = sb.write_repo("b", &format!("path = \"{}\"\n", sb.project.display()));
    sb.write_repo("c", "path = \"~/code/other\"\n");
    let expected = format!(
        "repository configs {} and {} both have path {}",
        a.display(),
        b.display(),
        sb.project.display()
    );
    for main in [sb.project.clone(), sb.home.join("code/other")] {
        let err = Config::load(&sb.opts_for_repo(&main)).unwrap_err();
        assert_eq!(err.to_string(), expected);
    }
    assert_eq!(
        Config::load(&sb.opts()).unwrap().repo,
        None,
        "not read without a repository"
    );
}

/// Only `*.toml` files directly in `repos/` count, and a `--config-dir`
/// has its own `repos/`.
#[test]
fn repository_configs_are_the_toml_files_of_the_repos_directory() {
    let sb = Sandbox::new();
    let repos = sb.home.join(".config/tasq/repos");
    fs::create_dir_all(&repos).unwrap();
    fs::write(repos.join("notes.md"), "not toml").unwrap();
    fs::create_dir_all(repos.join("nested.toml")).unwrap();
    assert_eq!(
        Config::load(&sb.opts_for_repo(&sb.project)).unwrap().repo,
        None
    );

    let other = sb.home.join("other");
    fs::create_dir_all(other.join("repos")).unwrap();
    let file = other.join("repos/x.toml");
    fs::write(
        &file,
        "path = \"~/code/project\"\n[worktree]\nbase = \"x\"\n",
    )
    .unwrap();
    sb.write_repo(
        "default",
        "path = \"~/code/project\"\n[worktree]\nbase = \"default\"\n",
    );
    let mut opts = sb.opts_for_repo(&sb.project);
    opts.config_dir = Some(other);
    let loaded = Config::load(&opts).unwrap();
    assert_eq!(loaded.config.worktree.base, "x");
    assert_eq!(loaded.file_for("worktree.base"), Some(file.as_path()));
}

/// `repo_configs` lists every repository config for `tasq doctor`: its
/// file and its `path` with `~` expanded, in file name order, failing like
/// a load on a duplicate.
#[test]
fn repo_configs_lists_each_file_with_its_path() {
    let sb = Sandbox::new();
    assert_eq!(repo_configs(&sb.opts()).unwrap(), Vec::new());
    let b = sb.write_repo("b", "path = \"~/code/project\"\n");
    let a = sb.write_repo("a", "path = \"/elsewhere\"\n[worktree]\nbase = \"x\"\n");
    assert_eq!(
        repo_configs(&sb.opts()).unwrap(),
        vec![
            RepoConfig {
                file: a,
                path: PathBuf::from("/elsewhere"),
            },
            RepoConfig {
                file: b.clone(),
                path: sb.project.clone(),
            },
        ]
    );
    let c = sb.write_repo("c", "path = \"~/code/project\"\n");
    assert_eq!(
        repo_configs(&sb.opts()).unwrap_err().to_string(),
        format!(
            "repository configs {} and {} both have path {}",
            b.display(),
            c.display(),
            sb.project.display()
        )
    );
    let mut opts = sb.opts();
    opts.home = None;
    assert_eq!(
        repo_configs(&opts).unwrap(),
        Vec::new(),
        "no config directory"
    );
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
    opts.config_dir = Some(sb.home.join(".config/tasq"));
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
    let path = sb.write_global("[forge.work]\nhost = \"git.example.com\"\n");
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
    let path = sb.write_global("[[source]]\nname = \"inbox\"\nkind = \"llm-bridge\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "source \"inbox\" of kind \"llm-bridge\" requires `command` (set by {})",
            path.display()
        )
    );

    sb.write_global("[[source]]\nname = \"mrs\"\nkind = \"github-review-requests\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::MissingSourceField { field: "forge", kind, .. } if kind == "github-review-requests"),
        "{err}"
    );

    sb.write_global("[[source]]\nname = \"mrs\"\nkind = \"gitlab-work-items\"\nforge = \"gl\"\n");
    let err = Config::load(&sb.opts()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "source \"mrs\" references forge \"gl\" (set by {}): no [forge.gl] block is defined",
            path.display()
        )
    );

    sb.write_global(
        "[forge.gh]\nkind = \"github\"\n[[source]]\nname = \"mrs\"\nkind = \"gitlab-review-requests\"\nforge = \"gh\"\n",
    );
    let err = Config::load(&sb.opts()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::BadForgeReference { reason, .. }
        if reason == "source kind \"gitlab-review-requests\" needs a gitlab forge, but its kind is github"),
        "{err}"
    );

    sb.write_global(
        "[forge.gh]\nkind = \"github\"\n[[source]]\nname = \"issues\"\nkind = \"github-work-items\"\nforge = \"gh\"\n",
    );
    let loaded = Config::load(&sb.opts()).unwrap();
    let s = &loaded.config.source[0];
    assert_eq!(s.kind, SourceKind::GithubWorkItems);
    assert!(s.enabled, "enabled defaults to true");
    assert!(s.auto, "auto defaults to true");
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
    sb.write_profile(
        "p",
        "[[source]]\nname = \"b\"\nkind = \"llm-bridge\"\ncommand = \"b\"\nenabled = false\nauto = false\n",
    );
    let loaded = Config::load(&sb.opts_with_profile("p")).unwrap();
    assert_eq!(loaded.config.source.len(), 1);
    assert_eq!(loaded.config.source[0].name, "b");
    assert!(!loaded.config.source[0].enabled);
    assert!(!loaded.config.source[0].auto);
}

// --- the shipped examples ----------------------------------------------------

/// `examples/config/*.toml` and `examples/sources/config.toml` are documented
/// as complete files with real keys only; loading them keeps them honest.
#[test]
fn every_example_config_loads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("config"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files.push(root.join("sources/config.toml"));
    assert_eq!(files.len(), 3, "{files:?}");
    for file in files {
        let dir = tempfile::tempdir().unwrap();
        fs::copy(&file, dir.path().join("config.toml")).unwrap();
        let mut opts = LoadOptions::new("/nonexistent");
        opts.home = Some(PathBuf::from("/home/u"));
        opts.config_dir = Some(dir.path().to_path_buf());
        if let Err(e) = Config::load(&opts) {
            panic!("{}: {e}", file.display());
        }
    }
}

// --- the plan's example ----------------------------------------------------

#[test]
fn plan_section_4_6_example_loads_as_is() {
    let sb = Sandbox::new();
    let path = sb.write_global(&fixture("plan-4.6.toml"));
    let loaded = Config::load(&sb.opts()).unwrap();
    let c = &loaded.config;
    assert_eq!(c.store.name, "home");
    assert_eq!(c.workflow.statuses.len(), 5);
    assert_eq!(
        c.work.default_project,
        Some(sb.home.join("code/SF/silverfin_worspace/silverfin"))
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
    assert!(c.source.iter().all(|s| s.enabled && s.auto));
    assert_eq!(c.report.summary.summarizer, Summarizer::Llm);
    assert_eq!(c.report.summary.command, "claude -p --model sonnet");
    assert_eq!(c.report.summary.model, None);
    assert_eq!(loaded.file_for("store.name"), Some(path.as_path()));
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
    assert_eq!(tasq_core::config::config_dir(&LoadOptions::new("/x")), None);
    let mut opts = LoadOptions::new("/x");
    opts.home = Some(PathBuf::from("/home/me"));
    assert_eq!(
        tasq_core::config::config_dir(&opts),
        Some(PathBuf::from("/home/me/.config/tasq"))
    );
    opts.env = env(&[("XDG_CONFIG_HOME", "/xdg")]);
    assert_eq!(
        tasq_core::config::config_dir(&opts),
        Some(PathBuf::from("/xdg/tasq"))
    );
    opts.env = env(&[("TASQ_CONFIG_DIR", "~/conf"), ("XDG_CONFIG_HOME", "/xdg")]);
    assert_eq!(
        tasq_core::config::config_dir(&opts),
        Some(PathBuf::from("/home/me/conf"))
    );
    opts.config_dir = Some(PathBuf::from("/flag"));
    assert_eq!(
        tasq_core::config::config_dir(&opts),
        Some(PathBuf::from("/flag"))
    );
    opts.config_dir = Some(PathBuf::from("rel"));
    assert_eq!(
        tasq_core::config::config_dir(&opts),
        Some(PathBuf::from("/x/rel"))
    );
}

/// Only a named directory, resolved as [`config_dir`] resolves it: what
/// tasq forwards to the processes it starts.
#[test]
fn named_config_dir_is_the_flag_or_the_variable_resolved() {
    use tasq_core::config::named_config_dir;
    let mut opts = LoadOptions::new("/x");
    opts.home = Some(PathBuf::from("/home/me"));
    opts.env = env(&[("XDG_CONFIG_HOME", "/xdg")]);
    assert_eq!(named_config_dir(&opts), None);
    opts.env = env(&[("TASQ_CONFIG_DIR", "")]);
    assert_eq!(named_config_dir(&opts), None, "empty is unset");
    opts.env = env(&[("TASQ_CONFIG_DIR", "~/conf")]);
    assert_eq!(
        named_config_dir(&opts),
        Some((PathBuf::from("/home/me/conf"), "TASQ_CONFIG_DIR"))
    );
    opts.env = env(&[("TASQ_CONFIG_DIR", "conf")]);
    assert_eq!(
        named_config_dir(&opts),
        Some((PathBuf::from("/x/conf"), "TASQ_CONFIG_DIR"))
    );
    opts.config_dir = Some(PathBuf::from("rel"));
    assert_eq!(
        named_config_dir(&opts),
        Some((PathBuf::from("/x/rel"), "--config-dir"))
    );
}

#[test]
fn explain_matches_prefixes_but_not_lookalike_keys() {
    let sb = Sandbox::new();
    let path = sb.write_global("[store]\nname = \"x\"\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    let file = Origin::File(path);
    assert_eq!(loaded.explain("store.name"), Some(&file));
    assert_eq!(loaded.explain("store"), Some(&file));
    assert_eq!(loaded.explain("store.name.x"), Some(&file));
    assert_eq!(loaded.explain("store.names"), None);
    assert_eq!(loaded.explain("stor"), None);
    assert_eq!(loaded.explain("ui.pager"), Some(&Origin::Defaults));
}

// --- acceptance: plan requirements pinned ----------------------------------

#[test]
fn env_vars_named_by_the_plan_are_all_supported() {
    use tasq_core::config::{ENV_CONFIG_DIR, ENV_KEYS, ENV_PROFILE};
    let keyed: Vec<&str> = ENV_KEYS.iter().map(|(var, _)| *var).collect();
    for var in [
        "TASQ_STORE_NAME",
        "TASQ_DEFAULT_PROJECT",
        "TASQ_PAGER",
        "TASQ_NO_OSC8",
        "TASQ_GLOW_STYLE",
        "TASQ_LAUNCHER",
    ] {
        assert!(keyed.contains(&var), "{var} missing from ENV_KEYS");
    }
    assert_eq!(ENV_PROFILE, "TASQ_PROFILE");
    assert_eq!(ENV_CONFIG_DIR, "TASQ_CONFIG_DIR");
    assert!(!keyed.contains(&ENV_PROFILE) && !keyed.contains(&ENV_CONFIG_DIR));
    let mut vars = keyed.clone();
    vars.sort_unstable();
    vars.dedup();
    assert_eq!(vars.len(), keyed.len(), "no duplicate variables");
}

#[test]
fn all_six_layers_stack_field_wise_in_documented_order() {
    let sb = Sandbox::new();
    // Every layer sets `worktree.base`; each also owns one private key.
    let global =
        sb.write_global("[worktree]\nbase = \"global\"\n[hooks]\npost-done = [\"echo done\"]\n");
    let profile_file = sb.write_profile(
        "p",
        "[worktree]\nbase = \"profile\"\n[ui]\nglow_style = \"light\"\n",
    );
    let repo = sb.write_repo(
        "project",
        "path = \"~/code/project\"\n[worktree]\nbase = \"repo\"\nsymlinks = [\"node_modules\"]\n",
    );
    let mut opts = sb.opts_for_repo(&sb.project);
    opts.profile = Some("p".to_owned());
    opts.env = env(&[("TASQ_WORKTREE_BASE", "env"), ("TASQ_LAUNCHER", "shell")]);
    opts.overrides = vec![
        ("worktree.base".to_owned(), "override".to_owned()),
        ("launch.env".to_owned(), "inherit".to_owned()),
    ];
    let loaded = Config::load(&opts).unwrap();
    let c = &loaded.config;
    assert_eq!(c.worktree.base, "override");
    assert_eq!(c.worktree.symlinks, vec!["node_modules"]);
    assert_eq!(c.ui.glow_style, "light");
    assert_eq!(c.launch.default, "shell");
    assert_eq!(c.launch.env, EnvStrategy::Inherit);

    let profile = Origin::Profile {
        name: "p".to_owned(),
        file: profile_file.clone(),
    };
    assert_eq!(loaded.explain("worktree.base"), Some(&Origin::Overrides));
    assert_eq!(
        loaded.explain("hooks.post-done"),
        Some(&Origin::File(global.clone()))
    );
    assert_eq!(
        loaded.explain("worktree.symlinks"),
        Some(&Origin::Repo(repo.clone()))
    );
    assert_eq!(loaded.explain("ui.glow_style"), Some(&profile));
    assert_eq!(loaded.explain("launch.default"), Some(&Origin::Env));
    assert_eq!(loaded.explain("launch.env"), Some(&Origin::Overrides));
    assert_eq!(loaded.explain("ui.no_osc8"), Some(&Origin::Defaults));
    assert_eq!(loaded.file_for("worktree.base"), None);
    assert_eq!(
        loaded.file_for("ui.glow_style"),
        Some(profile_file.as_path())
    );

    let origins: Vec<&Origin> = loaded.layers.iter().map(|l| &l.origin).collect();
    assert_eq!(
        origins,
        vec![
            &Origin::Defaults,
            &Origin::File(global),
            &profile,
            &Origin::Repo(repo.clone()),
            &Origin::Env,
            &Origin::Overrides,
        ]
    );
    // Dropping the overrides exposes the env value, and so on down the stack.
    let base = |opts: &LoadOptions| Config::load(opts).unwrap().config.worktree.base;
    opts.overrides.clear();
    assert_eq!(base(&opts), "env");
    opts.env.remove("TASQ_WORKTREE_BASE");
    assert_eq!(base(&opts), "repo");
    fs::remove_file(&repo).unwrap();
    assert_eq!(base(&opts), "profile");
    opts.profile = None;
    assert_eq!(base(&opts), "global");
}

#[test]
fn removed_worktree_manager_keys_name_the_replacement() {
    for key in [
        "worktree_manager = \"git\"",
        "worktree_command = \"mk {branch}\"",
    ] {
        let sb = Sandbox::new();
        let file = sb.write_global(&format!("[work]\ndefault_project = \"~\"\n{key}\n"));
        let err = Config::load(&sb.opts()).unwrap_err().to_string();
        let prefix = format!("{}:3:", file.display());
        assert!(err.starts_with(&prefix), "{err}");
        assert!(
            err.ends_with(
                ": work.worktree_manager and work.worktree_command were removed; worktrees are configured under [worktree] (docs/config.md, \"Worktrees\")"
            ),
            "{err}"
        );
    }
}

#[test]
fn copy_ignored_is_rejected_until_it_is_implemented() {
    for value in ["true", "false"] {
        let sb = Sandbox::new();
        let file = sb.write_global(&format!(
            "[worktree]\nbase = \"main\"\ncopy_ignored = {value}\n"
        ));
        let err = Config::load(&sb.opts()).unwrap_err().to_string();
        let prefix = format!("{}:3:", file.display());
        assert!(err.starts_with(&prefix), "{err}");
        assert!(
            err.ends_with(
                ": worktree.copy_ignored is not implemented yet; list the files under worktree.copies (docs/config.md, \"Worktrees\")"
            ),
            "{err}"
        );
    }
}

#[test]
fn worktree_table_from_a_file() {
    let sb = Sandbox::new();
    sb.write_global(
        "[worktree]\nsymlinks = [\"node_modules\"]\ncopies = [\".envrc\"]\n\
         templates = [{ source = \"t/.envrc\", target = \".envrc\" }]\n\
         [worktree.variables]\ndb = \"app\"\n[worktree.ports]\nbase = 4000\nspan = 1000\nblock = 10\n\
         [worktree.hooks]\npost-create = [\"direnv allow .\"]\npre-remove = [\"x\"]\n",
    );
    let w = Config::load(&sb.opts()).unwrap().config.worktree;
    assert_eq!(w.symlinks, vec!["node_modules"]);
    assert_eq!(w.copies, vec![".envrc"]);
    assert_eq!(
        w.templates,
        vec![tasq_core::config::TemplateFile {
            source: "t/.envrc".into(),
            target: ".envrc".into()
        }]
    );
    assert_eq!(w.variables.get("db").map(String::as_str), Some("app"));
    assert_eq!(
        w.ports,
        Some(tasq_core::config::PortsConfig {
            base: 4000,
            span: 1000,
            block: 10
        })
    );
    assert_eq!(w.hooks.post_create, vec!["direnv allow ."]);
    assert_eq!(w.hooks.pre_remove, vec!["x"]);
    assert_eq!(w.hooks.pre_create, Vec::<String>::new());
    assert_eq!(w.hooks.post_remove, Vec::<String>::new());
}

#[test]
fn worktree_ports_must_hold_a_block_below_65536() {
    let load = |base: u32, span: u32, block: u32| {
        let sb = Sandbox::new();
        sb.write_global(&format!(
            "[worktree.ports]\nbase = {base}\nspan = {span}\nblock = {block}\n"
        ));
        Config::load(&sb.opts())
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    assert_eq!(load(4000, 10, 10), Ok(()));
    assert_eq!(load(65526, 10, 10), Ok(()));
    assert_eq!(load(0, 65535, 1), Ok(()));
    let sb = Sandbox::new();
    let file = sb.write_global("[worktree.ports]\nbase = 4000\nspan = 10\nblock = 0\n");
    assert_eq!(
        Config::load(&sb.opts()).unwrap_err().to_string(),
        format!(
            "worktree.ports (set by {}): base = 4000, span = 10, block = 0: block must be at least 1, span at least block, and base + span at most 65536",
            file.display()
        )
    );
    assert!(load(4000, 9, 10).is_err());
    assert!(load(65527, 10, 10).is_err());
    assert!(load(4000, 10, 11).is_err());
}

#[test]
fn an_empty_table_is_recorded_as_a_leaf_key() {
    let sb = Sandbox::new();
    sb.write_global("[ui.colors]\n\n[forge]\n");
    let loaded = Config::load(&sb.opts()).unwrap();
    assert_eq!(loaded.layers[1].keys, vec!["forge", "ui.colors"]);
    assert!(
        loaded
            .explain("ui.colors")
            .is_some_and(|o| o.path().is_some())
    );
}

#[test]
fn worktree_templates_only_name_variables_of_their_scope() {
    let load = |text: &str| {
        let sb = Sandbox::new();
        let file = sb.write_global(text);
        (
            Config::load(&sb.opts())
                .map(|_| ())
                .map_err(|e| e.to_string()),
            file,
        )
    };
    // The defaults and every scope's own names pass.
    assert_eq!(load("[worktree]\n").0, Ok(()));
    assert_eq!(
        load(
            "[worktree]\nbranch = \"{{repo}}-{{id}}-{{slug}}-{{title}}-{{project}}-{{project_name}}-{{project_parent}}-{{x}}\"\n\
             path = \"{{branch}}{{branch_safe}}{{branch_db}}{{port}}\"\n\
             [worktree.variables]\nx = \"1\"\n[worktree.ports]\nbase = 4000\nspan = 10\nblock = 10\n\
             [worktree.hooks]\npost-create = [\"echo {{worktree}} {{worktree_name}} {{port}}\"]\n"
        )
        .0,
        Ok(())
    );
    let (err, file) = load("[worktree]\nbranch = \"{{id}}-{{branch}}\"\n");
    assert_eq!(
        err.unwrap_err(),
        format!(
            "worktree.branch (set by {}): unknown variable {{{{branch}}}}; known: {{{{id}}}}, {{{{project}}}}, {{{{project_name}}}}, {{{{project_parent}}}}, {{{{repo}}}}, {{{{slug}}}}, {{{{title}}}}",
            file.display()
        )
    );
    for (text, key, name) in [
        (
            "[worktree]\npath = \"{{worktree}}\"\n",
            "worktree.path",
            "worktree",
        ),
        ("[worktree]\npath = \"{{port}}\"\n", "worktree.path", "port"),
        (
            "[worktree.hooks]\npre-create = [\"ok\", \"{{nope}}\"]\n",
            "worktree.hooks.pre-create",
            "nope",
        ),
        (
            "[worktree.hooks]\npost-create = [\"{{nope}}\"]\n",
            "worktree.hooks.post-create",
            "nope",
        ),
        (
            "[worktree.hooks]\npre-remove = [\"{{nope}}\"]\n",
            "worktree.hooks.pre-remove",
            "nope",
        ),
        (
            "[worktree.hooks]\npost-remove = [\"{{nope}}\"]\n",
            "worktree.hooks.post-remove",
            "nope",
        ),
    ] {
        let err = load(text).0.unwrap_err();
        assert!(err.starts_with(&format!("{key} (set by ")), "{err}");
        assert!(
            err.contains(&format!("unknown variable {{{{{name}}}}};")),
            "{err}"
        );
    }
    // Checked even with worktrees disabled, so enabling never surfaces it.
    assert!(load("[worktree]\nbranch = \"{{bad}}\"\n").0.is_err());
}

#[test]
fn worktree_variables_are_names_that_shadow_no_builtin() {
    let sb = Sandbox::new();
    let file = sb.write_global("[worktree.variables]\nport = \"1\"\n");
    assert_eq!(
        Config::load(&sb.opts()).unwrap_err().to_string(),
        format!(
            "worktree.variables.port (set by {}): a built-in variable has this name",
            file.display()
        )
    );
    sb.write_global("[worktree.variables]\n\"db-name\" = \"1\"\n");
    assert_eq!(
        Config::load(&sb.opts()).unwrap_err().to_string(),
        format!(
            "worktree.variables.db-name (set by {}): a variable name is letters, digits and underscores",
            file.display()
        )
    );
    sb.write_global("[worktree.variables]\n\"\" = \"1\"\n");
    assert!(Config::load(&sb.opts()).is_err());
    sb.write_global("[worktree.variables]\ndb_name2 = \"1\"\n");
    assert!(Config::load(&sb.opts()).is_ok());
}
