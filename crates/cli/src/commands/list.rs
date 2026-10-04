//! `tasq list` (and the bare `tasq [WORD]`): the status-grouped view of
//! the original script.
//!
//! Rendering reproduces `print_group` from the script: a bold, coloured
//! group header, then one row per task
//! `  [id] #prio Title (due YYYY-MM-DD) chips`, then a blank line. Priority
//! `A` is bold red, `B` and `C` dim; due dates are dim; topic tags are
//! chips (white on dark blue). When the filter names a single status the
//! header is the status name itself, uncoloured, as the script printed it.

use std::collections::BTreeMap;

use tasq_core::clock::format_date;
use tasq_core::config::UiConfig;
use tasq_core::model::{Priority, Status, Tag, Task, Workflow};
use tasq_core::query::{self, Filter, Group};
use tasq_core::store::Store;

use crate::app::App;
use crate::cli::ListArgs;
use crate::error::{CliError, Result};
use crate::json;
use crate::output::{Color, Style};

/// Runs `list`.
pub fn run(app: &App, args: &ListArgs) -> Result<()> {
    let workflow = app.workflow();
    let criteria = Criteria::from_args(args, &workflow)?;
    let store = app.open_store()?;
    let open = store.list(&Filter::default())?;
    let groups = query::list(&open, &criteria.filter(), &workflow);
    if app.out.json_mode() {
        let tasks: Vec<&Task> = groups
            .iter()
            .flat_map(|g| g.tasks.iter().copied())
            .collect();
        return app
            .out
            .json(&json::document([("tasks", json::to_value(&tasks))]));
    }
    if open.is_empty() {
        return app.out.print("No open todos.\n");
    }
    if groups.is_empty() {
        return app.out.print(&format!("{}\n", criteria.empty_message()));
    }
    let theme = Theme::from_config(&app.config().ui);
    let text = render(&groups, &theme, app.out.style(), criteria.status.is_some());
    app.out.page(&text)
}

/// The list filter as the user expressed it, kept apart from [`Filter`]
/// so that the empty-result message can name what was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Criteria {
    /// `--status` or a status WORD.
    pub status: Option<Status>,
    /// `--tag`s plus a tag WORD.
    pub tags: Vec<Tag>,
    /// `--prio` or a priority WORD.
    pub priority: Option<Priority>,
    /// `--text`.
    pub text: Option<String>,
}

impl Criteria {
    /// Interprets WORD like the script (status, then `A`/`B`/`C`, then a
    /// tag) and adds the explicit flags.
    pub fn from_args(args: &ListArgs, workflow: &Workflow) -> Result<Self> {
        let mut criteria = Self::default();
        if let Some(word) = &args.word {
            if let Some(status) = workflow.parse_status(word) {
                criteria.status = Some(status);
            } else if let Ok(priority) = word.parse::<Priority>() {
                criteria.priority = Some(priority);
            } else {
                criteria.tags.push(word.parse::<Tag>()?);
            }
        }
        if let Some(status) = &args.status {
            criteria.status = Some(workflow.parse_status(status).ok_or_else(|| {
                CliError::user(format!(
                    "unknown status '{status}' (statuses: {})",
                    status_list(workflow)
                ))
            })?);
        }
        for tag in &args.tag {
            let tag = tag.parse::<Tag>()?;
            if !criteria.tags.contains(&tag) {
                criteria.tags.push(tag);
            }
        }
        if let Some(prio) = &args.prio {
            criteria.priority = Some(prio.parse::<Priority>().map_err(|_| {
                CliError::user(format!("unknown priority '{prio}' (priorities: A B C)"))
            })?);
        }
        criteria.text.clone_from(&args.text);
        Ok(criteria)
    }

    /// The core filter: open tasks matching every criterion.
    pub fn filter(&self) -> Filter {
        let mut filter = Filter::default();
        if let Some(status) = &self.status {
            filter = filter.status(status.clone());
        }
        for tag in &self.tags {
            filter = filter.tag(tag.clone());
        }
        if let Some(priority) = self.priority {
            filter = filter.priority(priority);
        }
        if let Some(text) = &self.text {
            filter = filter.text(text);
        }
        filter
    }

    /// What to print when open tasks exist but none matches.
    pub fn empty_message(&self) -> String {
        let mut parts = Vec::new();
        if let Some(status) = &self.status {
            parts.push(format!("with status {status}"));
        }
        if !self.tags.is_empty() {
            let tags: Vec<String> = self.tags.iter().map(Tag::to_hash).collect();
            parts.push(format!("tagged {}", tags.join(" ")));
        }
        if let Some(priority) = self.priority {
            parts.push(format!("with priority {}", priority.to_hash()));
        }
        if let Some(text) = &self.text {
            parts.push(format!("matching {text:?}"));
        }
        if parts.is_empty() {
            "No open todos.".to_owned()
        } else {
            format!("No open todos {}.", parts.join(" "))
        }
    }
}

fn status_list(workflow: &Workflow) -> String {
    workflow
        .statuses
        .iter()
        .map(Status::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Colours of the group headers: the script's five plus `cyan` for any
/// other configured status and dim for `NO STATUS`, overridable per status
/// name under `[ui.colors]` (`no-status` for the last group).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    overrides: BTreeMap<String, Color>,
}

impl Theme {
    /// Reads `[ui.colors]`; entries that are not a colour are ignored.
    pub fn from_config(ui: &UiConfig) -> Self {
        Self {
            overrides: ui
                .colors
                .iter()
                .filter_map(|(name, spec)| Color::parse(spec).map(|c| (name.clone(), c)))
                .collect(),
        }
    }

    /// The header colour of a status group (`None` is the no-status group).
    pub fn color(&self, status: Option<&Status>) -> Color {
        let name = status.map_or("no-status", Status::as_str);
        if let Some(color) = self.overrides.get(name) {
            return *color;
        }
        match status {
            None => Color::Dim,
            Some(s) if *s == Status::IN_PROGRESS => Color::Blue,
            Some(s) if *s == Status::READY => Color::Green,
            Some(s) if *s == Status::WAITING => Color::Yellow,
            Some(s) if *s == Status::BLOCKED => Color::Red,
            Some(s) if *s == Status::LATER => Color::Magenta,
            Some(_) => Color::Cyan,
        }
    }
}

/// The `IN PROGRESS` / `NO STATUS` header of a group.
pub fn group_label(status: Option<&Status>) -> String {
    status.map_or_else(
        || "NO STATUS".to_owned(),
        |s| s.as_str().to_uppercase().replace('-', " "),
    )
}

/// Renders the groups. `single_status` reproduces the script's
/// one-status view: the header is the status name, bold but uncoloured.
pub fn render(groups: &[Group<'_>], theme: &Theme, style: Style, single_status: bool) -> String {
    let mut out = String::new();
    for group in groups {
        let header = match (&group.status, single_status) {
            (Some(status), true) => style.bold(status.as_str()),
            (status, _) => {
                style.bold_color(theme.color(status.as_ref()), &group_label(status.as_ref()))
            }
        };
        out.push_str(&header);
        out.push('\n');
        for task in &group.tasks {
            out.push_str(&row(task, style));
        }
        out.push('\n');
    }
    out
}

/// One task line: `  [id] #prio Title (due date) chips`.
pub fn row(task: &Task, style: Style) -> String {
    let id = style.dim(&format!("[{:>2}]", task.id.as_str()));
    let prio = match task.priority {
        Priority::A => style.bold_color(Color::Red, "#A"),
        Priority::B => style.dim("#B"),
        Priority::C => style.dim("#C"),
    };
    let due = task.due.map_or_else(String::new, |d| {
        format!(" {}", style.dim(&format!("(due {})", format_date(d))))
    });
    let chips = task.tags.iter().fold(String::new(), |mut acc, t| {
        acc.push(' ');
        acc.push_str(&style.chip(&t.to_hash()));
        acc
    });
    format!("  {id} {prio} {}{due}{chips}\n", task.title)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use tasq_core::model::TaskId;

    use super::*;

    fn args(word: Option<&str>) -> ListArgs {
        ListArgs {
            word: word.map(str::to_owned),
            ..ListArgs::default()
        }
    }

    #[test]
    fn word_is_status_then_priority_then_tag() {
        let wf = Workflow::default();
        assert_eq!(
            Criteria::from_args(&args(Some("ready")), &wf)
                .unwrap()
                .status,
            Some(Status::READY)
        );
        assert_eq!(
            Criteria::from_args(&args(Some("#A")), &wf)
                .unwrap()
                .priority,
            Some(Priority::A)
        );
        let c = Criteria::from_args(&args(Some("#gitlab")), &wf).unwrap();
        assert_eq!(c.tags, vec![Tag::new("gitlab").unwrap()]);
        assert!(Criteria::from_args(&args(Some("##x")), &wf).is_err());
    }

    #[test]
    fn flags_combine_with_the_word() {
        let wf = Workflow::default();
        let a = ListArgs {
            word: Some("gitlab".into()),
            status: Some("waiting".into()),
            tag: vec!["gitlab".into(), "#support".into()],
            prio: Some("c".into()),
            text: Some("build".into()),
        };
        assert_eq!(
            Criteria::from_args(&a, &wf).unwrap_err().to_string(),
            "unknown priority 'c' (priorities: A B C)"
        );
        let a = ListArgs {
            prio: Some("C".into()),
            ..a
        };
        let c = Criteria::from_args(&a, &wf).unwrap();
        assert_eq!(c.status, Some(Status::WAITING));
        assert_eq!(c.tags.len(), 2, "duplicate tag collapsed: {:?}", c.tags);
        assert_eq!(c.priority, Some(Priority::C));
        assert_eq!(c.text.as_deref(), Some("build"));
        assert_eq!(
            c.empty_message(),
            "No open todos with status waiting tagged #gitlab #support with priority #C matching \"build\"."
        );
        let bad = ListArgs {
            status: Some("nope".into()),
            ..ListArgs::default()
        };
        assert_eq!(
            Criteria::from_args(&bad, &wf).unwrap_err().to_string(),
            "unknown status 'nope' (statuses: in-progress ready waiting blocked later)"
        );
    }

    #[test]
    fn empty_messages_match_the_script() {
        let wf = Workflow::default();
        assert_eq!(
            Criteria::from_args(&args(Some("x")), &wf)
                .unwrap()
                .empty_message(),
            "No open todos tagged #x."
        );
        assert_eq!(Criteria::default().empty_message(), "No open todos.");
    }

    #[test]
    fn filter_round_trip() {
        let wf = Workflow::default();
        let c = Criteria::from_args(&args(Some("ready")), &wf).unwrap();
        let mut t = Task::new(TaskId::from(1), "x");
        assert!(!c.filter().matches(&t));
        t.set_status(Status::READY);
        assert!(c.filter().matches(&t));
    }

    #[test]
    fn theme_defaults_and_overrides() {
        let theme = Theme::from_config(&UiConfig::default());
        assert_eq!(theme.color(Some(&Status::IN_PROGRESS)), Color::Blue);
        assert_eq!(theme.color(Some(&Status::READY)), Color::Green);
        assert_eq!(theme.color(Some(&Status::WAITING)), Color::Yellow);
        assert_eq!(theme.color(Some(&Status::BLOCKED)), Color::Red);
        assert_eq!(theme.color(Some(&Status::LATER)), Color::Magenta);
        assert_eq!(
            theme.color(Some(&Status::new("review").unwrap())),
            Color::Cyan
        );
        assert_eq!(theme.color(None), Color::Dim);
        let mut ui = UiConfig::default();
        ui.colors.insert("ready".into(), "208".into());
        ui.colors.insert("no-status".into(), "white".into());
        ui.colors.insert("later".into(), "not-a-colour".into());
        let theme = Theme::from_config(&ui);
        assert_eq!(theme.color(Some(&Status::READY)), Color::Fixed(208));
        assert_eq!(theme.color(None), Color::White);
        assert_eq!(theme.color(Some(&Status::LATER)), Color::Magenta);
    }

    #[test]
    fn labels() {
        assert_eq!(group_label(Some(&Status::IN_PROGRESS)), "IN PROGRESS");
        assert_eq!(group_label(None), "NO STATUS");
    }

    #[test]
    fn row_format_matches_the_script() {
        let mut t = Task::new(TaskId::from(3), "Fix it");
        t.set_priority(Priority::A);
        t.due = NaiveDate::from_ymd_opt(2026, 10, 10);
        t.add_tag(Tag::new("gitlab").unwrap());
        t.add_tag(Tag::new("review-request").unwrap());
        assert_eq!(
            row(&t, Style::OFF),
            "  [ 3] #A Fix it (due 2026-10-10)  #gitlab   #review-request \n"
        );
        assert_eq!(
            row(&t, Style::ON),
            "  \x1b[2m[ 3]\x1b[0m \x1b[1;31m#A\x1b[0m Fix it \x1b[2m(due 2026-10-10)\x1b[0m \x1b[48;5;24m\x1b[38;5;231m #gitlab \x1b[0m \x1b[48;5;24m\x1b[38;5;231m #review-request \x1b[0m\n"
        );
        let plain = Task::new(TaskId::from(12), "Plain");
        assert_eq!(row(&plain, Style::OFF), "  [12] #B Plain\n");
    }

    #[test]
    fn render_groups() {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        let b = Task::new(TaskId::from(2), "B");
        let tasks = vec![a, b];
        let groups = query::list(&tasks, &Filter::default(), &Workflow::default());
        let theme = Theme::from_config(&UiConfig::default());
        assert_eq!(
            render(&groups, &theme, Style::OFF, false),
            "READY\n  [ 1] #B A\n\nNO STATUS\n  [ 2] #B B\n\n"
        );
        let one = query::list(
            &tasks,
            &Filter::default().status(Status::READY),
            &Workflow::default(),
        );
        assert_eq!(
            render(&one, &theme, Style::ON, true),
            "\x1b[1mready\x1b[0m\n  \x1b[2m[ 1]\x1b[0m \x1b[2m#B\x1b[0m A\n\n"
        );
        assert_eq!(
            render(&one, &theme, Style::ON, false),
            "\x1b[1;32mREADY\x1b[0m\n  \x1b[2m[ 1]\x1b[0m \x1b[2m#B\x1b[0m A\n\n"
        );
    }
}
