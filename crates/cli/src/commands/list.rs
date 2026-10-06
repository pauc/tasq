//! `tasq list` (and the bare `tasq [WORD]`): the status-grouped view of
//! the original script.
//!
//! Rendering reproduces `print_group` from the script: a bold, coloured
//! group header, then one row per task
//! `  [id] #prio Title (due) chips`, then a blank line. Priority `A` is
//! bold red, `B` and `C` dim; due dates read `overdue 3d` (bold red),
//! `due today` / `due tomorrow` (yellow) or `due in 4d` (dim) under the
//! default `ui.due_format`; topic tags are chips (white on dark blue). When the filter names a single status the
//! header is the status name itself, uncoloured, as the script printed it.

use chrono::NaiveDate;
use tasq_core::clock::format_date;
use tasq_core::config::DueFormat;
#[cfg(test)]
use tasq_core::config::UiConfig;
use tasq_core::dates::{Due, due_label};
use tasq_core::model::{Priority, Status, Tag, Task, Workflow};
use tasq_core::query::{self, Filter, Group};
use tasq_core::store::Store;

use crate::app::App;
use crate::cli::ListArgs;
use crate::error::{CliError, Result};
use crate::json;
use crate::output::Style;
pub use tasq_core::theme::{DONE_LABEL, Role, Theme, group_label};

/// Runs `list`.
pub fn run(app: &App, args: &ListArgs) -> Result<()> {
    let workflow = app.workflow();
    let criteria = Criteria::from_args(args, &workflow)?;
    let store = app.open_store()?;
    let tasks = store.list(&criteria.scope.store_filter())?;
    let matched = query::filter(&tasks, &criteria.filter());
    let groups = query::group_by_status(matched.iter().copied().filter(|t| !t.done), &workflow);
    let done = query::sort(matched.iter().copied().filter(|t| t.done));
    if app.out.json_mode() {
        let tasks: Vec<&Task> = groups
            .iter()
            .flat_map(|g| g.tasks.iter().copied())
            .chain(done.iter().copied())
            .collect();
        return app
            .out
            .json(&json::document([("tasks", json::to_value(&tasks))]));
    }
    if tasks.is_empty() {
        return app.out.print(&format!("{}.\n", criteria.scope.nothing()));
    }
    if groups.is_empty() && done.is_empty() {
        return app.out.print(&format!("{}\n", criteria.empty_message()));
    }
    let theme = Theme::from_config(&app.config().ui);
    let look = Look {
        theme: &theme,
        style: app.out.style(),
        today: app.clock()?.today(),
        due_format: app.config().ui.due_format,
    };
    let text = render(&groups, &done, &look, criteria.status.is_some());
    app.out.page(&text)
}

/// Which tasks `list` looks at: open ones (the default), every task
/// (`--all`) or done ones (`--done`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Open tasks only, as the script listed.
    #[default]
    Open,
    /// Open and done tasks.
    All,
    /// Done tasks only.
    Done,
}

impl Scope {
    /// `--all` wins over `--done` (clap rejects both together).
    pub fn from_flags(all: bool, done: bool) -> Self {
        match (all, done) {
            (true, _) => Self::All,
            (false, true) => Self::Done,
            (false, false) => Self::Open,
        }
    }

    /// The filter for reading the store.
    pub fn store_filter(self) -> Filter {
        match self {
            Self::Open => Filter::default(),
            Self::All => Filter::default().any_done(),
            Self::Done => Filter::default().done(true),
        }
    }

    /// `No open todos` / `No todos` / `No done todos`.
    pub fn nothing(self) -> &'static str {
        match self {
            Self::Open => "No open todos",
            Self::All => "No todos",
            Self::Done => "No done todos",
        }
    }
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
    /// `--all` / `--done`.
    pub scope: Scope,
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
        criteria.scope = Scope::from_flags(args.all, args.done);
        Ok(criteria)
    }

    /// The core filter: the scope's tasks matching every criterion.
    pub fn filter(&self) -> Filter {
        let mut filter = self.scope.store_filter();
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
            format!("{}.", self.scope.nothing())
        } else {
            format!("{} {}.", self.scope.nothing(), parts.join(" "))
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

/// What rows are drawn with: the colours, whether escape codes are
/// written, and the day and format due dates are shown against.
#[derive(Debug, Clone, Copy)]
pub struct Look<'a> {
    /// The colour of every role.
    pub theme: &'a Theme,
    /// Escape codes on or off.
    pub style: Style,
    /// The day relative due dates count from.
    pub today: NaiveDate,
    /// `ui.due_format`.
    pub due_format: DueFormat,
}

/// Renders the groups, then `done` as a `DONE` group when non-empty.
/// `single_status` reproduces the script's one-status view: the header is
/// the status name, bold but uncoloured.
pub fn render(
    groups: &[Group<'_>],
    done: &[&Task],
    look: &Look<'_>,
    single_status: bool,
) -> String {
    let (theme, style) = (look.theme, look.style);
    let mut out = String::new();
    for group in groups {
        let header = match (&group.status, single_status) {
            (Some(status), true) => style.bold(status.as_str()),
            (status, _) => style.bold_color(
                theme.status_color(status.as_ref()),
                &group_label(status.as_ref()),
            ),
        };
        out.push_str(&header);
        out.push('\n');
        for task in &group.tasks {
            out.push_str(&row(task, look));
        }
        out.push('\n');
    }
    if !done.is_empty() {
        out.push_str(&style.bold_color(theme.done_color(), DONE_LABEL));
        out.push('\n');
        for task in done {
            out.push_str(&row(task, look));
        }
        out.push('\n');
    }
    out
}

/// One task line: `  [id] #prio Title (due) chips`, the id, `#B` and `#C`
/// in the theme's `dim`, `#A` in `prio-a`, the chips in `chip-fg` on
/// `chip-bg`. The due date is shown in `ui.due_format`: bold `overdue`
/// when past, `due-soon` today and tomorrow, `dim` further ahead; a done
/// task shows its date, dim.
pub fn row(task: &Task, look: &Look<'_>) -> String {
    let (theme, style) = (look.theme, look.style);
    let dim = theme.color(Role::Dim);
    let id = style.color(dim, &format!("[{:>2}]", task.id.as_str()));
    let prio = match task.priority {
        Priority::A => style.bold_color(theme.color(Role::PrioA), "#A"),
        Priority::B => style.color(dim, "#B"),
        Priority::C => style.color(dim, "#C"),
    };
    let due = task.due.map_or_else(String::new, |d| {
        let due = if task.done {
            style.color(dim, &format!("(due {})", format_date(d)))
        } else {
            let text = format!("({})", due_label(d, look.today, look.due_format));
            match Role::of_due(Due::of(d, look.today)) {
                Role::Overdue => style.bold_color(theme.color(Role::Overdue), &text),
                role => style.color(theme.color(role), &text),
            }
        };
        format!(" {due}")
    });
    let (bg, fg) = (theme.color(Role::ChipBg), theme.color(Role::ChipFg));
    let chips = task.tags.iter().fold(String::new(), |mut acc, t| {
        acc.push(' ');
        acc.push_str(&style.chip(bg, fg, &t.to_hash()));
        acc
    });
    format!("  {id} {prio} {}{due}{chips}\n", task.title)
}

#[cfg(test)]
mod tests {
    use tasq_core::model::TaskId;

    use super::*;

    /// 2026-10-06, ISO dates: the rows as they were before relative dates.
    fn look(theme: &Theme, style: Style) -> Look<'_> {
        Look {
            theme,
            style,
            today: NaiveDate::from_ymd_opt(2026, 10, 6).unwrap(),
            due_format: DueFormat::Iso,
        }
    }

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
            all: false,
            done: false,
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
    fn scope_flags_filters_and_messages() {
        assert_eq!(Scope::from_flags(false, false), Scope::Open);
        assert_eq!(Scope::from_flags(true, false), Scope::All);
        assert_eq!(Scope::from_flags(true, true), Scope::All);
        assert_eq!(Scope::from_flags(false, true), Scope::Done);
        let mut open = Task::new(TaskId::from(1), "open");
        open.set_status(Status::READY);
        let mut done = Task::new(TaskId::from(2), "done");
        done.done = true;
        assert!(Scope::Open.store_filter().matches(&open));
        assert!(!Scope::Open.store_filter().matches(&done));
        assert!(Scope::All.store_filter().matches(&open));
        assert!(Scope::All.store_filter().matches(&done));
        assert!(!Scope::Done.store_filter().matches(&open));
        assert!(Scope::Done.store_filter().matches(&done));

        let wf = Workflow::default();
        let a = ListArgs {
            all: true,
            tag: vec!["x".into()],
            ..ListArgs::default()
        };
        let c = Criteria::from_args(&a, &wf).unwrap();
        assert_eq!(c.scope, Scope::All);
        assert_eq!(c.empty_message(), "No todos tagged #x.");
        let mut tagged_done = done.clone();
        tagged_done.add_tag(Tag::new("x").unwrap());
        assert!(c.filter().matches(&tagged_done));
        assert!(!c.filter().matches(&done));
        let a = ListArgs {
            done: true,
            ..ListArgs::default()
        };
        let c = Criteria::from_args(&a, &wf).unwrap();
        assert_eq!(c.scope, Scope::Done);
        assert_eq!(c.empty_message(), "No done todos.");
        assert!(c.filter().matches(&done));
        assert!(!c.filter().matches(&open));
        assert_eq!(Scope::Open.nothing(), "No open todos");
    }

    #[test]
    fn render_done_group_comes_last() {
        let mut a = Task::new(TaskId::from(1), "A");
        a.set_status(Status::READY);
        let mut d = Task::new(TaskId::from(2), "D");
        d.done = true;
        let tasks = vec![a, d];
        let groups = query::list(&tasks, &Filter::default(), &Workflow::default());
        let done: Vec<&Task> = tasks.iter().filter(|t| t.done).collect();
        let theme = Theme::from_config(&UiConfig::default());
        assert_eq!(
            render(&groups, &done, &look(&theme, Style::OFF), false),
            "READY\n  [ 1] #B A\n\nDONE\n  [ 2] #B D\n\n"
        );
        assert_eq!(
            render(&[], &done, &look(&theme, Style::ON), false),
            "\x1b[1;2mDONE\x1b[0m\n  \x1b[2m[ 2]\x1b[0m \x1b[2m#B\x1b[0m D\n\n"
        );
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
    fn row_format_matches_the_script() {
        let mut t = Task::new(TaskId::from(3), "Fix it");
        t.set_priority(Priority::A);
        t.due = NaiveDate::from_ymd_opt(2026, 10, 10);
        t.add_tag(Tag::new("gitlab").unwrap());
        t.add_tag(Tag::new("review-request").unwrap());
        let theme = Theme::default();
        assert_eq!(
            row(&t, &look(&theme, Style::OFF)),
            "  [ 3] #A Fix it (due 2026-10-10)  #gitlab   #review-request \n"
        );
        assert_eq!(
            row(&t, &look(&theme, Style::ON)),
            "  \x1b[2m[ 3]\x1b[0m \x1b[1;31m#A\x1b[0m Fix it \x1b[2m(due 2026-10-10)\x1b[0m \x1b[48;5;24m\x1b[38;5;231m #gitlab \x1b[0m \x1b[48;5;24m\x1b[38;5;231m #review-request \x1b[0m\n"
        );
        let plain = Task::new(TaskId::from(12), "Plain");
        assert_eq!(row(&plain, &look(&theme, Style::OFF)), "  [12] #B Plain\n");
        // A preset restyles the id, the marker, the date and the chips.
        let mut ui = UiConfig::default();
        ui.theme.preset = tasq_core::theme::Preset::Light;
        let light = Theme::from_config(&ui);
        assert_eq!(
            row(&t, &look(&light, Style::ON)),
            "  \x1b[38;5;245m[ 3]\x1b[0m \x1b[1;38;5;124m#A\x1b[0m Fix it \x1b[38;5;245m(due 2026-10-10)\x1b[0m \x1b[48;5;153m\x1b[38;5;17m #gitlab \x1b[0m \x1b[48;5;153m\x1b[38;5;17m #review-request \x1b[0m\n"
        );
        ui.theme.preset = tasq_core::theme::Preset::Mono;
        let mono = Theme::from_config(&ui);
        assert_eq!(
            row(&plain, &look(&mono, Style::ON)),
            "  \x1b[2m[12]\x1b[0m \x1b[2m#B\x1b[0m Plain\n"
        );
        assert_eq!(
            row(&t, &look(&mono, Style::ON)),
            "  \x1b[2m[ 3]\x1b[0m \x1b[1m#A\x1b[0m Fix it \x1b[2m(due 2026-10-10)\x1b[0m \x1b[7m #gitlab \x1b[0m \x1b[7m #review-request \x1b[0m\n"
        );
    }

    #[test]
    fn row_due_dates_relative_and_coloured() {
        let theme = Theme::default();
        let mut on = look(&theme, Style::ON);
        on.due_format = DueFormat::Relative;
        let off = Look {
            style: Style::OFF,
            ..on
        };
        let due = |day: u32| {
            let mut t = Task::new(TaskId::from(3), "T");
            t.due = NaiveDate::from_ymd_opt(2026, 10, day);
            t
        };
        assert_eq!(row(&due(3), &off), "  [ 3] #B T (overdue 3d)\n");
        assert_eq!(row(&due(6), &off), "  [ 3] #B T (due today)\n");
        assert_eq!(row(&due(7), &off), "  [ 3] #B T (due tomorrow)\n");
        assert_eq!(row(&due(10), &off), "  [ 3] #B T (due in 4d)\n");
        let id = "  \x1b[2m[ 3]\x1b[0m \x1b[2m#B\x1b[0m T";
        assert_eq!(
            row(&due(3), &on),
            format!("{id} \x1b[1;31m(overdue 3d)\x1b[0m\n")
        );
        assert_eq!(
            row(&due(6), &on),
            format!("{id} \x1b[33m(due today)\x1b[0m\n")
        );
        assert_eq!(
            row(&due(7), &on),
            format!("{id} \x1b[33m(due tomorrow)\x1b[0m\n")
        );
        assert_eq!(
            row(&due(10), &on),
            format!("{id} \x1b[2m(due in 4d)\x1b[0m\n")
        );
        // `both`, and a done task: its date only, dim, never overdue.
        let both = Look {
            due_format: DueFormat::Both,
            ..off
        };
        assert_eq!(
            row(&due(3), &both),
            "  [ 3] #B T (overdue 3d, 2026-10-03)\n"
        );
        let mut closed = due(3);
        closed.done = true;
        assert_eq!(row(&closed, &both), "  [ 3] #B T (due 2026-10-03)\n");
        assert_eq!(
            row(&closed, &on),
            format!("{id} \x1b[2m(due 2026-10-03)\x1b[0m\n")
        );
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
            render(&groups, &[], &look(&theme, Style::OFF), false),
            "READY\n  [ 1] #B A\n\nNO STATUS\n  [ 2] #B B\n\n"
        );
        let one = query::list(
            &tasks,
            &Filter::default().status(Status::READY),
            &Workflow::default(),
        );
        assert_eq!(
            render(&one, &[], &look(&theme, Style::ON), true),
            "\x1b[1mready\x1b[0m\n  \x1b[2m[ 1]\x1b[0m \x1b[2m#B\x1b[0m A\n\n"
        );
        assert_eq!(
            render(&one, &[], &look(&theme, Style::ON), false),
            "\x1b[1;32mREADY\x1b[0m\n  \x1b[2m[ 1]\x1b[0m \x1b[2m#B\x1b[0m A\n\n"
        );
    }
}
