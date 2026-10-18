//! `tasq migrate <notebook> [--to <dir>] [--dry-run]`: an nb notebook
//! converted into a tasq store once (ADR-0026), by
//! `tasq_store::migrate`. The notebook is only read and no git runs.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use tasq_store::migrate::{self, Plan};
use tasq_store::store::os_random;

use crate::app::App;
use crate::error::{CliError, Result};
use crate::json;

/// nb's data directory variable, read to find a notebook given by name.
const NB_DIR: &str = "NB_DIR";

/// Runs `tasq migrate`: resolves the notebook and the target (`--to` from
/// the working directory, else the configured store directory), plans,
/// writes unless `dry_run`, then prints the mapping and the config lines.
pub fn run(app: &App, notebook: &str, to: Option<&Path>, dry_run: bool) -> Result<()> {
    let notebook = migrate::resolve_notebook(
        notebook,
        &app.opts.cwd,
        app.opts.home.as_deref(),
        app.opts.env.get(NB_DIR).map(Path::new),
    )?;
    let target = match to {
        Some(to) => lexical(&app.opts.cwd.join(to)),
        None => app.store_dir()?,
    };
    let settings = StoreSettings::of(&target)?;
    let plan = migrate::plan(
        &notebook,
        &target,
        &app.workflow(),
        app.clock()?.as_ref(),
        &mut os_random,
    )?;
    if !dry_run {
        plan.write()?;
    }
    if app.out.json_mode() {
        return app.out.json(&plan_json(&plan, &settings, dry_run));
    }
    app.out.print(&render(&plan, &settings, dry_run))
}

/// `path` with `.` dropped and each `..` taking off the component before
/// it, without asking the filesystem (the target may not exist yet), so
/// the printed `data_dir` is the directory meant.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The `data_dir` and `store.name` that select a store directory: its
/// parent and its last component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSettings {
    /// The parent of the store directory.
    pub data_dir: PathBuf,
    /// The store directory's name.
    pub name: String,
}

impl StoreSettings {
    /// The settings of `dir`; an error when `dir` does not end in a name
    /// (`/`, `..`).
    pub fn of(dir: &Path) -> Result<Self> {
        match (dir.parent(), dir.file_name()) {
            (Some(parent), Some(name)) => Ok(Self {
                data_dir: parent.to_owned(),
                name: name.to_string_lossy().into_owned(),
            }),
            _ => Err(CliError::user(format!(
                "{}: the target must end in a directory name",
                dir.display()
            ))),
        }
    }

    /// The config lines, as TOML.
    pub fn toml(&self) -> String {
        let quote = |text: &str| toml::Value::String(text.to_owned()).to_string();
        format!(
            "data_dir = {}\n\n[store]\nname = {}\n",
            quote(&self.data_dir.display().to_string()),
            quote(&self.name)
        )
    }
}

/// The text: one line per task (id right-aligned, file, title), a blank
/// line, what was (or would be) written where, and the config lines.
pub fn render(plan: &Plan, settings: &StoreSettings, dry_run: bool) -> String {
    let width = plan
        .entries()
        .iter()
        .map(|e| e.id.as_str().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for e in plan.entries() {
        let _ = writeln!(out, "{:>width$}  {}  {}", e.id.as_str(), e.file, e.title);
    }
    let count = plan.entries().len();
    let tasks = if count == 1 { "task" } else { "tasks" };
    let target = plan.target().display();
    if dry_run {
        let _ = writeln!(
            out,
            "\nWould write {count} {tasks} to {target} (dry run: nothing written)."
        );
    } else {
        let _ = writeln!(out, "\nWrote {count} {tasks} to {target}.");
    }
    out.push_str("To use this store, set in the tasq config:\n\n");
    out.push_str(&settings.toml());
    out
}

/// The `--json` document: `dry_run`, `target`, `tasks` (`id`, `file`,
/// `title`, `from`, in id order) and `config` (`data_dir`, `store_name`).
fn plan_json(plan: &Plan, settings: &StoreSettings, dry_run: bool) -> Value {
    let tasks: Vec<Value> = plan
        .entries()
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id.as_str(),
                "file": e.file,
                "title": e.title,
                "from": e.from.display().to_string(),
            })
        })
        .collect();
    json::document([
        ("dry_run", Value::from(dry_run)),
        ("target", Value::from(plan.target().display().to_string())),
        ("tasks", Value::from(tasks)),
        (
            "config",
            serde_json::json!({
                "data_dir": settings.data_dir.display().to_string(),
                "store_name": settings.name,
            }),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use tasq_core::clock::FixedClock;
    use tasq_core::model::Workflow;

    use super::*;

    /// The plan of a notebook whose `.index` lists `names` (one per line,
    /// `""` for a blank line), each todo file titled after its name, into
    /// `/srv/stores/work`; every ULID's random part is 1.
    fn plan_of(names: &[&str]) -> Plan {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = names.join("\n") + "\n";
        std::fs::write(dir.path().join(".index"), index).expect("index");
        for name in names.iter().filter(|n| !n.is_empty()) {
            std::fs::write(dir.path().join(name), format!("# [ ] Title of {name}\n"))
                .expect("todo");
        }
        migrate::plan(
            dir.path(),
            Path::new("/srv/stores/work"),
            &Workflow::default(),
            &FixedClock::at("2026-09-01 09:00"),
            &mut || 1,
        )
        .expect("plan")
    }

    fn settings() -> StoreSettings {
        StoreSettings::of(Path::new("/srv/stores/work")).expect("settings")
    }

    #[test]
    fn ids_are_right_aligned_and_a_dry_run_says_nothing_was_written() {
        let names = [
            "20260901090000.todo.md",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "20260902090000.todo.md",
        ];
        let plan = plan_of(&names);
        let [one, ten] = plan.entries() else {
            panic!("two entries: {:?}", plan.entries());
        };
        assert_eq!(
            render(&plan, &settings(), true),
            format!(
                " 1  {}  Title of 20260901090000.todo.md\n\
                 10  {}  Title of 20260902090000.todo.md\n\
                 \n\
                 Would write 2 tasks to /srv/stores/work (dry run: nothing written).\n\
                 To use this store, set in the tasq config:\n\
                 \n\
                 data_dir = \"/srv/stores\"\n\
                 \n\
                 [store]\n\
                 name = \"work\"\n",
                one.file, ten.file
            )
        );
    }

    #[test]
    fn one_task_is_singular_and_a_write_says_where() {
        let text = render(&plan_of(&["20260901090000.todo.md"]), &settings(), false);
        assert!(
            text.contains("\nWrote 1 task to /srv/stores/work.\nTo use"),
            "{text}"
        );
        let empty = render(&plan_of(&[]), &settings(), false);
        assert!(
            empty.starts_with("\nWrote 0 tasks to /srv/stores/work.\n"),
            "{empty}"
        );
    }

    #[test]
    fn the_config_lines_are_quoted_toml() {
        let settings = StoreSettings::of(Path::new("/odd \"dir\"/my\\store")).expect("settings");
        assert_eq!(
            settings.toml(),
            "data_dir = '/odd \"dir\"'\n\n[store]\nname = 'my\\store'\n"
        );
        let parsed: toml::Table = settings.toml().parse().expect("valid TOML");
        assert_eq!(parsed["data_dir"].as_str(), Some("/odd \"dir\""));
        assert_eq!(parsed["store"]["name"].as_str(), Some("my\\store"));
    }

    #[test]
    fn dots_in_the_target_are_resolved_by_name() {
        assert_eq!(
            lexical(Path::new("/home/u/./a/../stores/./work")),
            PathBuf::from("/home/u/stores/work")
        );
        assert_eq!(
            lexical(Path::new("/home/u/../../../x")),
            PathBuf::from("/x")
        );
    }

    #[test]
    fn a_target_without_a_last_name_is_refused() {
        assert_eq!(
            StoreSettings::of(Path::new("/")).map_err(|e| e.to_string()),
            Err("/: the target must end in a directory name".to_owned())
        );
        assert_eq!(
            StoreSettings::of(Path::new("/srv/..")).map_err(|e| e.to_string()),
            Err("/srv/..: the target must end in a directory name".to_owned())
        );
    }

    #[test]
    fn json_lists_the_tasks_and_the_settings() {
        let plan = plan_of(&["20260901090000.todo.md"]);
        let value = plan_json(&plan, &settings(), false);
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["target"], "/srv/stores/work");
        assert_eq!(value["tasks"][0]["id"], "1");
        assert_eq!(value["tasks"][0]["file"], plan.entries()[0].file);
        assert!(
            value["tasks"][0]["from"]
                .as_str()
                .is_some_and(|from| from.ends_with("/20260901090000.todo.md")),
            "{value}"
        );
        assert_eq!(
            value["tasks"][0]["title"],
            "Title of 20260901090000.todo.md"
        );
        assert_eq!(
            value["config"],
            serde_json::json!({"data_dir": "/srv/stores", "store_name": "work"})
        );
    }
}
