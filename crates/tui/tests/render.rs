//! Rendering snapshots (plan T-801/T-803): models drawn onto a
//! `TestBackend` at the two layouts, with every overlay and mode. Accept
//! changes with `INSTA_UPDATE=always cargo test -p tasq-tui`.

use chrono::NaiveDate;
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use tasq_core::clock::FixedClock;
use tasq_core::config::UiConfig;
use tasq_core::model::{Link, Priority, Session, Status, Tag, Task, TaskId, Workflow, Worktree};
use tasq_core::theme::Theme;
use tasq_tui::{Model, Msg, update, view};

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn tasks() -> Vec<Task> {
    let mut first = Task::new(TaskId::from(1), "Fix the flaky build");
    first.set_status(Status::IN_PROGRESS);
    first.set_priority(Priority::A);
    first.due = Some(date("2026-10-10"));
    first.add_tag(Tag::new("ci").unwrap());
    first.description = Some("The nightly job fails one time in three.\nSuspect the cache.".into());
    first.project = Some("/home/me/code/app".into());
    first.add_related(Link::new("https://example.com/issue/1"));
    first.add_merge_request(Link::labelled(
        "https://gitlab.example.com/g/p/-/merge_requests/7",
        "Pin the cache key",
    ));
    first.add_worktree(Worktree::on_branch(
        "/home/me/code/app-fix-cache",
        "fix-cache",
    ));
    let mut session = Session::new(FixedClock::at("2026-10-03 16:40").0, "abc123");
    session.description = Some("first look".into());
    first.add_session(session);
    let clock = FixedClock::at("2026-10-04 09:30");
    for note in [
        "reproduced locally",
        "cache key uses the wrong hash",
        "fix pushed",
    ] {
        first.log(note, &clock);
    }

    let mut second = Task::new(TaskId::from(12), "Review MR !88: paginate the export");
    second.set_status(Status::READY);
    second.add_tag(Tag::new("gitlab").unwrap());
    second.add_tag(Tag::new("review-request").unwrap());

    let mut third = Task::new(TaskId::from(3), "Write the release notes");
    third.set_status(Status::READY);
    third.set_priority(Priority::C);

    let mut later = Task::new(TaskId::from(4), "Try the new profiler");
    later.set_status(Status::LATER);

    let loose = Task::new(TaskId::from(5), "Call the bank");
    vec![first, second, third, later, loose]
}

fn fixture(color: bool) -> Model {
    let mut model = Model::new(Workflow::default(), Theme::default(), color);
    update(&mut model, Msg::Loaded(tasks()));
    model
}

fn render(model: &mut Model, width: u16, height: u16) -> Terminal<TestBackend> {
    update(model, Msg::Resize(width, height));
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    terminal
}

fn screen(model: &mut Model, width: u16, height: u16) -> String {
    render(model, width, height).backend().to_string()
}

#[test]
fn two_pane_layout() {
    let mut model = fixture(true);
    assert_snapshot!(screen(&mut model, 120, 28));
}

#[test]
fn one_pane_layout_list_and_detail() {
    let mut model = fixture(true);
    assert_snapshot!("one_pane_list", screen(&mut model, 80, 24));
    update(&mut model, Msg::ToggleDetail);
    assert_snapshot!("one_pane_detail", screen(&mut model, 80, 24));
}

#[test]
fn selection_and_filter() {
    let mut model = fixture(true);
    update(&mut model, Msg::Down);
    update(&mut model, Msg::Down);
    assert_snapshot!("third_selected", screen(&mut model, 120, 20));
    update(&mut model, Msg::BeginFilter);
    for c in "re".chars() {
        update(&mut model, Msg::Char(c));
    }
    assert_snapshot!("typing_a_filter", screen(&mut model, 120, 20));
    update(&mut model, Msg::Enter);
    update(&mut model, Msg::Escape); // clears the filter
    update(&mut model, Msg::BeginFilter);
    for c in "#later".chars() {
        update(&mut model, Msg::Char(c));
    }
    update(&mut model, Msg::Enter);
    assert_snapshot!("hash_filter_applied", screen(&mut model, 120, 20));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginFilter);
    for c in "zzz".chars() {
        update(&mut model, Msg::Char(c));
    }
    assert_snapshot!("nothing_matches", screen(&mut model, 120, 20));
}

#[test]
fn overlays() {
    let mut model = fixture(true);
    update(&mut model, Msg::Help);
    assert_snapshot!("help", screen(&mut model, 120, 28));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginStatus);
    assert_snapshot!("status_picker", screen(&mut model, 120, 20));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginPriority);
    assert_snapshot!("priority_picker", screen(&mut model, 120, 20));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginNote);
    for c in "found the cause".chars() {
        update(&mut model, Msg::Char(c));
    }
    assert_snapshot!("typing_a_note", screen(&mut model, 120, 20));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginDone);
    assert_snapshot!("done_prompt", screen(&mut model, 120, 20));
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginCreate);
    for c in "Call the bank".chars() {
        update(&mut model, Msg::Char(c));
    }
    assert_snapshot!("typing_a_title", screen(&mut model, 120, 20));
}

#[test]
fn creating_from_an_empty_list() {
    let mut model =
        Model::new(Workflow::default(), Theme::default(), true).with_default_status(Status::LATER);
    update(&mut model, Msg::Loaded(Vec::new()));
    update(&mut model, Msg::BeginCreate);
    for c in "First one".chars() {
        update(&mut model, Msg::Char(c));
    }
    assert_snapshot!("typing_a_title_with_no_tasks", screen(&mut model, 80, 10));
}

#[test]
fn messages_and_empty_states() {
    let mut model = fixture(true);
    update(&mut model, Msg::Info("[1] -> ready".into()));
    assert_snapshot!("info_message", screen(&mut model, 100, 12));
    update(&mut model, Msg::Failed("no task with id 9".into()));
    assert_snapshot!("error_message", screen(&mut model, 100, 12));
    update(&mut model, Msg::Loaded(Vec::new()));
    update(&mut model, Msg::Down);
    assert_snapshot!("no_tasks", screen(&mut model, 100, 12));
}

#[test]
fn long_lists_scroll_to_the_selection() {
    let mut model = Model::new(Workflow::default(), Theme::default(), true);
    let many: Vec<Task> = (1..=30)
        .map(|i| {
            let mut t = Task::new(TaskId::from(i), format!("Task number {i}"));
            t.set_status(Status::READY);
            t
        })
        .collect();
    update(&mut model, Msg::Loaded(many));
    update(&mut model, Msg::Bottom);
    assert_snapshot!(screen(&mut model, 80, 12));
}

#[test]
fn colours_follow_the_theme_and_no_color() {
    // Default theme: the IN PROGRESS header is blue and bold.
    let mut model = fixture(true);
    let terminal = render(&mut model, 120, 28);
    let cell = &terminal.backend().buffer()[(1, 1)];
    assert_eq!(cell.symbol(), "I");
    assert_eq!(cell.fg, Color::Blue);
    assert!(cell.modifier.contains(Modifier::BOLD));
    // The selected row is reversed; the id is dim.
    let row = &terminal.backend().buffer()[(3, 2)];
    assert_eq!(row.symbol(), "[");
    assert!(row.modifier.contains(Modifier::REVERSED));
    assert!(row.modifier.contains(Modifier::DIM));

    // [ui.colors] override by status name.
    let mut ui = UiConfig::default();
    ui.colors.insert("in-progress".into(), "208".into());
    let mut themed = Model::new(Workflow::default(), Theme::from_config(&ui), true);
    update(&mut themed, Msg::Loaded(tasks()));
    let terminal = render(&mut themed, 120, 28);
    assert_eq!(terminal.backend().buffer()[(1, 1)].fg, Color::Indexed(208));

    // NO_COLOR: no foreground colours anywhere, only modifiers.
    let mut plain = fixture(false);
    let terminal = render(&mut plain, 120, 28);
    let header = &terminal.backend().buffer()[(1, 1)];
    assert_eq!(header.fg, Color::Reset);
    assert!(header.modifier.contains(Modifier::BOLD));
    for cell in terminal.backend().buffer().content() {
        assert_eq!(cell.fg, Color::Reset, "{cell:?}");
        assert_eq!(cell.bg, Color::Reset, "{cell:?}");
    }
    // Both renderings show the same text.
    assert_eq!(
        screen(&mut plain, 120, 28),
        screen(&mut model, 120, 28),
        "colour must not change the layout"
    );
}
