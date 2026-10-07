//! Rendering snapshots (plan T-801/T-803): models drawn onto a
//! `TestBackend` at the two layouts, with every overlay and mode. Accept
//! changes with `INSTA_UPDATE=always cargo test -p tasq-tui`.

use chrono::{NaiveDate, Weekday};
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use tasq_core::clock::FixedClock;
use tasq_core::config::UiConfig;
use tasq_core::model::{Link, Priority, Session, Status, Tag, Task, TaskId, Workflow, Worktree};
use tasq_core::theme::{Preset, Theme};
use tasq_tui::{Model, Msg, SourceChoice, update, view};

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

/// Today is 2026-10-06 unless a test says otherwise.
fn fixture(color: bool) -> Model {
    let mut model =
        Model::new(Workflow::default(), Theme::default(), color).with_today(date("2026-10-06"));
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
    assert_snapshot!("two_pane_list", screen(&mut model, 120, 28));
    update(&mut model, Msg::ShowDetail);
    assert_snapshot!("two_pane_detail", screen(&mut model, 120, 28));
    update(&mut model, Msg::HideDetail);
    assert_snapshot!("two_pane_list", screen(&mut model, 120, 28));
}

#[test]
fn due_dates_are_relative_and_coloured() {
    let mut model = fixture(true);
    let mut tasks = tasks();
    tasks[1].due = Some(date("2026-10-03"));
    tasks[2].due = Some(date("2026-10-07"));
    update(&mut model, Msg::Loaded(tasks));
    let terminal = render(&mut model, 120, 28);
    let buffer = terminal.backend().buffer();
    let (x, y) = find(&terminal, "(overdue 3d)", 0);
    assert_eq!(buffer[(x, y)].fg, Color::Red);
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    let (x, y) = find(&terminal, "(due tomorrow)", 0);
    assert_eq!(buffer[(x, y)].fg, Color::Yellow);
    assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
    let (x, y) = find(&terminal, "(due in 4d)", 0);
    assert!(buffer[(x, y)].modifier.contains(Modifier::DIM));
    // The detail pane keeps the date beside the relative form.
    update(&mut model, Msg::Down);
    update(&mut model, Msg::ShowDetail);
    let terminal = render(&mut model, 120, 28);
    let (x, y) = find(&terminal, "overdue 3d, 2026-10-03", 60);
    assert_eq!(terminal.backend().buffer()[(x, y)].fg, Color::Red);
}

#[test]
fn long_rows_wrap_under_the_title() {
    let mut model = fixture(true);
    let mut long = Task::new(
        TaskId::from(42),
        "Investigate why the nightly export job times out on the biggest tenants",
    );
    long.set_status(Status::READY);
    long.due = Some(date("2026-10-20"));
    long.add_tag(Tag::new("gitlab").unwrap());
    long.add_tag(Tag::new("support").unwrap());
    let mut tasks = tasks();
    tasks.push(long);
    update(&mut model, Msg::Loaded(tasks));
    update(&mut model, Msg::Bottom);
    assert_snapshot!("wrapped_rows", screen(&mut model, 60, 17));
    // A wrapped selected row at the bottom scrolls fully into view.
    assert_snapshot!("wrapped_rows_scrolled", screen(&mut model, 60, 9));
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
fn folded_groups() {
    // Colours off: the count, the blank lines and `▸` carry the grouping.
    let mut model = fixture(false);
    update(&mut model, Msg::Down);
    update(&mut model, Msg::ToggleGroup);
    assert_snapshot!("folded_group", screen(&mut model, 80, 14));
}

/// Two done tasks for the DONE group: one closed by tasq (with a past due
/// date, not overdue once done), one closed by `nb todo do` (no time).
fn done_tasks() -> Vec<Task> {
    let mut shipped = Task::new(TaskId::from(7), "Ship the export fix");
    shipped.due = Some(date("2026-10-01"));
    shipped.log("merged", &FixedClock::at("2026-10-05 17:10"));
    shipped.close(&FixedClock::at("2026-10-05 17:10"));
    let mut old = Task::new(TaskId::from(2), "Rotate the API keys");
    old.log("done by hand", &FixedClock::at("2026-09-30 11:00"));
    old.mark_done();
    vec![shipped, old]
}

#[test]
fn done_group() {
    // Colours off, so the `+done` chip, the counts and `▸` carry it.
    let mut model = fixture(false);
    let mut all = tasks();
    all.extend(done_tasks());
    update(&mut model, Msg::Loaded(all));
    assert_snapshot!("done_hidden", screen(&mut model, 80, 18));
    update(&mut model, Msg::ToggleDone);
    assert_snapshot!("done_folded", screen(&mut model, 80, 18));
    update(&mut model, Msg::Bottom);
    update(&mut model, Msg::ToggleGroup);
    assert_snapshot!("done_unfolded", screen(&mut model, 80, 22));
    update(&mut model, Msg::ShowDetail);
    assert_snapshot!("done_detail", screen(&mut model, 120, 22));
    update(&mut model, Msg::BeginStatus);
    assert_snapshot!("done_refuses_status", screen(&mut model, 120, 22));
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
    let choice = |name: &str, kind: &str, auto| SourceChoice {
        name: name.to_owned(),
        kind: kind.to_owned(),
        auto,
    };
    let mut model = model.with_sources(vec![
        choice("gitlab-review-requests", "gitlab-review-requests", true),
        choice("issues", "gitlab-work-items", true),
        choice("inbox", "llm-bridge", false),
    ]);
    update(&mut model, Msg::BeginSources);
    update(&mut model, Msg::Down);
    assert_snapshot!("sources_picker", screen(&mut model, 120, 20));
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
fn edit_view() {
    let mut model = fixture(true).with_today(date("2026-10-05"));
    update(&mut model, Msg::Edit);
    let mut terminal = render(&mut model, 120, 24);
    assert_snapshot!("edit_view", terminal.backend().to_string());
    let (x, y) = find(&terminal, "\u{2502} Fix the flaky build", 0);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (x + 2 + 19, y).into(),
        "the cursor after the title, inside its box"
    );
    update(&mut model, Msg::NextField);
    update(&mut model, Msg::Right);
    assert_snapshot!("edit_view_choice_row", screen(&mut model, 120, 24));
    assert_snapshot!("edit_view_choice_row_100", screen(&mut model, 100, 20));
    for _ in 0..5 {
        update(&mut model, Msg::NextField);
    }
    update(&mut model, Msg::End);
    for c in " Cache.".chars() {
        update(&mut model, Msg::Char(c));
    }
    let mut terminal = render(&mut model, 120, 24);
    assert_snapshot!("edit_view_description", terminal.backend().to_string());
    let (x, y) = find(&terminal, "one time in three. Cache.", 0);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (x + 25, y).into(),
        "the cursor in the description"
    );
    update(&mut model, Msg::PrevField);
    update(&mut model, Msg::PrevField);
    update(&mut model, Msg::PrevField);
    update(&mut model, Msg::Char('x'));
    update(&mut model, Msg::Save);
    assert_snapshot!("edit_view_bad_due", screen(&mut model, 120, 24));
    // The due box is 12 columns inside; a longer value scrolls under the cursor.
    for c in " or later".chars() {
        update(&mut model, Msg::Char(c));
    }
    let mut terminal = render(&mut model, 120, 24);
    assert_snapshot!("edit_view_due_scrolled", terminal.backend().to_string());
    let (x, y) = find(&terminal, "0x or later", 0);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (x + 11, y).into(),
        "the first nine characters scrolled away; the cursor is on the box's last column"
    );
    // Narrow: the long description line wraps, the cursor follows.
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::Edit);
    for _ in 0..6 {
        update(&mut model, Msg::NextField);
    }
    update(&mut model, Msg::End);
    let mut terminal = render(&mut model, 40, 16);
    assert_snapshot!("edit_view_narrow", terminal.backend().to_string());
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (2 + 4, 10).into(),
        "a 40-character line wraps at 36 columns; the cursor is on the second piece"
    );
    // The description scrolls to keep the cursor in its five lines.
    for c in "\n\n\n\nlast".chars() {
        update(&mut model, Msg::Char(c));
    }
    let mut terminal = render(&mut model, 40, 16);
    assert_snapshot!("edit_view_scrolled", terminal.backend().to_string());
    assert_eq!(terminal.get_cursor_position().unwrap(), (2 + 4, 13).into());
    // A long single-line value scrolls under the cursor.
    for _ in 0..6 {
        update(&mut model, Msg::PrevField);
    }
    for c in " on arm64 too".chars() {
        update(&mut model, Msg::Char(c));
    }
    let mut terminal = render(&mut model, 40, 16);
    assert_snapshot!("edit_view_long_title", terminal.backend().to_string());
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (2 + 11 + 24, 1).into(),
        "the cursor stays on the last value column"
    );
}

#[test]
fn calendar_picker() {
    let mut model = fixture(true).with_today(date("2026-10-05"));
    update(&mut model, Msg::Edit);
    for _ in 0..3 {
        update(&mut model, Msg::NextField);
    }
    update(&mut model, Msg::Enter);
    let mut terminal = render(&mut model, 120, 24);
    assert_snapshot!("calendar_picker", terminal.backend().to_string());
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (0, 0).into(),
        "no terminal cursor while the picker is open"
    );
    let buffer = terminal.backend().buffer();
    let (x, y) = find(&terminal, "October 2026", 0);
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    let (x, y) = find(&terminal, "Mo Tu We Th Fr Sa Su", 0);
    assert!(
        buffer[(x, y)].modifier.contains(Modifier::DIM),
        "the weekday header"
    );
    assert_eq!(buffer[(x - 3, y)].fg, Color::Cyan, "the border");
    // The popup's title, right of the Due box's own.
    let (x, y) = find(&terminal, "\u{250c} Due \u{2500}", 20);
    assert_eq!(buffer[(x + 2, y)].fg, Color::Cyan, "the title");
    assert!(buffer[(x + 2, y)].modifier.contains(Modifier::BOLD));
    // The week of the 5th: Mo 5 .. Su 11. The cursor is on the 10th (the
    // due date), reversed; today (the 5th) is bold; the 10th and 11th dim.
    let (x, y) = find(&terminal, " 5  6  7  8  9 10 11", 0);
    let cell = |dx: u16| buffer[(x + dx, y)].modifier;
    assert!(cell(1).contains(Modifier::BOLD), "today");
    assert!(!cell(1).intersects(Modifier::DIM | Modifier::REVERSED));
    assert!(
        !cell(7).intersects(Modifier::DIM | Modifier::BOLD | Modifier::REVERSED),
        "a weekday"
    );
    assert!(
        cell(16).contains(Modifier::REVERSED | Modifier::BOLD | Modifier::DIM),
        "the cursor on a Saturday"
    );
    assert!(!cell(13).contains(Modifier::REVERSED), "Friday");
    assert!(cell(19).contains(Modifier::DIM), "Sunday");
    assert!(!cell(19).intersects(Modifier::BOLD | Modifier::REVERSED));
    assert!(!cell(0).contains(Modifier::REVERSED), "the gutter");
    assert!(
        !cell(14).contains(Modifier::REVERSED),
        "the gutter before the cursor"
    );
    assert!(cell(15).contains(Modifier::REVERSED), "both digits");
    // Moving keeps the view underneath; a month jump redraws the grid.
    update(&mut model, Msg::PageDown);
    update(&mut model, Msg::Up);
    assert_snapshot!("calendar_picker_next_month", screen(&mut model, 120, 24));
    // Narrow: the popup sits over the compact view.
    assert_snapshot!("calendar_picker_narrow", screen(&mut model, 60, 16));
    // A command's result arriving meanwhile replaces the key bar, as in
    // the view; the next key clears it.
    update(&mut model, Msg::Failed("sync: boom".into()));
    let terminal = render(&mut model, 120, 24);
    let (x, y) = find(&terminal, "sync: boom", 0);
    assert_eq!((x, y), (0, 23));
    assert_eq!(terminal.backend().buffer()[(x, y)].fg, Color::Red);
    update(&mut model, Msg::Enter);
    let mut terminal = render(&mut model, 120, 24);
    let (x, y) = find(&terminal, "2026-11-03", 0);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (x + 10, y).into(),
        "back in the Due box, the cursor after the picked day"
    );
    // The border loses its colour with --color never; the modifiers stay.
    let mut plain = fixture(false).with_today(date("2026-10-05"));
    update(&mut plain, Msg::Edit);
    for _ in 0..3 {
        update(&mut plain, Msg::NextField);
    }
    update(&mut plain, Msg::Enter);
    let terminal = render(&mut plain, 120, 24);
    let buffer = terminal.backend().buffer();
    let (x, y) = find(&terminal, "Mo Tu", 0);
    assert_eq!(buffer[(x - 3, y)].fg, Color::Reset);
    let (x, y) = find(&terminal, " 5  6  7  8  9 10 11", 0);
    assert!(buffer[(x + 16, y)].modifier.contains(Modifier::REVERSED));
    // `ui.week_start = "sunday"`: the same month, the columns rotated.
    let mut sunday = fixture(true)
        .with_today(date("2026-10-05"))
        .with_week_start(Weekday::Sun);
    update(&mut sunday, Msg::Edit);
    for _ in 0..3 {
        update(&mut sunday, Msg::NextField);
    }
    update(&mut sunday, Msg::Enter);
    let terminal = render(&mut sunday, 120, 24);
    assert_snapshot!("calendar_picker_sunday", terminal.backend().to_string());
    let buffer = terminal.backend().buffer();
    let (x, y) = find(&terminal, " 4  5  6  7  8  9 10", 0);
    assert!(
        buffer[(x + 1, y)].modifier.contains(Modifier::DIM),
        "Sunday first"
    );
    assert!(
        buffer[(x + 4, y)].modifier.contains(Modifier::BOLD),
        "today"
    );
    assert!(
        buffer[(x + 19, y)].modifier.contains(Modifier::REVERSED),
        "the cursor"
    );
}

#[test]
fn edit_view_compact() {
    let mut model = fixture(true).with_today(date("2026-10-05"));
    update(&mut model, Msg::Edit);
    update(&mut model, Msg::NextField);
    let mut terminal = render(&mut model, 60, 16);
    assert_snapshot!("edit_view_compact_choice", terminal.backend().to_string());
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (0, 0).into(),
        "no cursor on a choice row"
    );
    let buffer = terminal.backend().buffer();
    let status = &buffer[find(&terminal, "Status", 0)];
    assert_eq!(status.fg, Color::Cyan);
    assert!(status.modifier.contains(Modifier::BOLD));
    let title = &buffer[find(&terminal, "Title", 0)];
    assert_eq!(title.fg, Color::Reset);
    assert!(!title.modifier.contains(Modifier::BOLD));
    let rule = &buffer[find(&terminal, "Description", 0)];
    assert_eq!(rule.fg, Color::Reset);
    update(&mut model, Msg::NextField);
    update(&mut model, Msg::NextField);
    let mut terminal = render(&mut model, 60, 16);
    let (x, y) = find(&terminal, "2026-10-10", 0);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (x + 10, y).into(),
        "the cursor after the due date, on its row"
    );
    for _ in 0..3 {
        update(&mut model, Msg::NextField);
    }
    let terminal = render(&mut model, 60, 16);
    let rule = &terminal.backend().buffer()[find(&terminal, "Description", 0)];
    assert_eq!(rule.fg, Color::Cyan);
    assert!(rule.modifier.contains(Modifier::BOLD));
}

#[test]
fn edit_view_styles() {
    let mut model = fixture(true);
    update(&mut model, Msg::Edit);
    let terminal = render(&mut model, 120, 24);
    let buffer = terminal.backend().buffer();
    let focused = &buffer[find(&terminal, "Title", 0)];
    assert_eq!(focused.fg, Color::Cyan);
    assert!(focused.modifier.contains(Modifier::BOLD));
    let plain = &buffer[find(&terminal, "Status", 0)];
    assert_eq!(plain.fg, Color::Reset);
    assert!(!plain.modifier.contains(Modifier::BOLD));
    let rule = &buffer[find(&terminal, "Description", 0)];
    assert_eq!(rule.fg, Color::Reset);
    let (x, y) = find(&terminal, "Tab/S-Tab", 0);
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    // Choice boxes: the chosen option bold, the others dim; reversed too
    // once the box has the focus.
    let chosen = &buffer[find(&terminal, "in-progress", 0)];
    assert!(chosen.modifier.contains(Modifier::BOLD));
    assert!(!chosen.modifier.contains(Modifier::REVERSED));
    assert!(!chosen.modifier.contains(Modifier::DIM));
    let other = &buffer[find(&terminal, "waiting", 0)];
    assert!(other.modifier.contains(Modifier::DIM));
    assert!(!other.modifier.contains(Modifier::BOLD));
    update(&mut model, Msg::NextField);
    let terminal = render(&mut model, 120, 24);
    let chosen = &terminal.backend().buffer()[find(&terminal, "in-progress", 0)];
    assert!(
        chosen
            .modifier
            .contains(Modifier::REVERSED | Modifier::BOLD)
    );
    let other = &terminal.backend().buffer()[find(&terminal, "waiting", 0)];
    assert!(other.modifier.contains(Modifier::DIM));
    assert!(!other.modifier.contains(Modifier::REVERSED));
    update(&mut model, Msg::PrevField);
    assert!(
        buffer[(x + 10, y)].modifier.contains(Modifier::DIM),
        "the label after the key"
    );
    for _ in 0..6 {
        update(&mut model, Msg::NextField);
    }
    let terminal = render(&mut model, 120, 24);
    let rule = &terminal.backend().buffer()[find(&terminal, "Description", 0)];
    assert_eq!(rule.fg, Color::Cyan);
    assert!(rule.modifier.contains(Modifier::BOLD));
    let mut plain = fixture(false);
    update(&mut plain, Msg::Edit);
    let terminal = render(&mut plain, 120, 24);
    let focused = &terminal.backend().buffer()[find(&terminal, "Title", 0)];
    assert_eq!(focused.fg, Color::Reset, "NO_COLOR keeps the bold only");
    assert!(focused.modifier.contains(Modifier::BOLD));
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
fn long_prompts_scroll_under_the_cursor() {
    let mut model = fixture(true);
    update(&mut model, Msg::BeginNote);
    let note = "the cache key hashes the lockfile but not the toolchain file, so a rustup bump reuses a stale cache";
    assert!(note.len() > 80);
    for c in note.chars() {
        update(&mut model, Msg::Char(c));
    }
    let mut terminal = render(&mut model, 80, 10);
    assert_snapshot!("long_note_scrolled", terminal.backend().to_string());
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (79, 9).into(),
        "the cursor on the bar's last column"
    );
    assert_eq!(find(&terminal, "log: ", 0), (0, 9), "the label stays");
    let (x, _) = find(&terminal, "reuses a stale cache", 0);
    assert_eq!(x, 79 - 20, "the tail of the note ends before the cursor");
    // Home scrolls back to the start; typing there shows where it went.
    update(&mut model, Msg::Home);
    update(&mut model, Msg::Char('>'));
    let mut terminal = render(&mut model, 80, 10);
    assert_eq!(terminal.get_cursor_position().unwrap(), (6, 9).into());
    assert_eq!(find(&terminal, "log: >the cache key", 0), (0, 9));
    // The other prompts put the terminal cursor on their input too.
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginFilter);
    update(&mut model, Msg::Char('r'));
    update(&mut model, Msg::Char('e'));
    update(&mut model, Msg::Left);
    let mut terminal = render(&mut model, 80, 10);
    assert_eq!(terminal.get_cursor_position().unwrap(), (2, 9).into());
    update(&mut model, Msg::Escape);
    update(&mut model, Msg::BeginCreate);
    let mut terminal = render(&mut model, 80, 10);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (u16::try_from("new task (ready): ".len()).unwrap(), 9).into()
    );
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

/// Where `text` first appears on screen, scanning rows from column
/// `from_x`; panics when it is not there.
fn find(terminal: &Terminal<TestBackend>, text: &str, from_x: u16) -> (u16, u16) {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (from_x..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(i) = row.find(text) {
            let x = u16::try_from(row[..i].chars().count()).unwrap();
            return (from_x + x, y);
        }
    }
    panic!("{text:?} is not on screen");
}

#[test]
fn detail_head_and_progress_heading() {
    use tasq_tui::view::detail_lines;
    let model = fixture(true);
    let text = |task: &Task| -> Vec<String> {
        detail_lines(&model, task)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    };
    // A done task without a status says so in the head; an open one does not.
    let mut done = Task::new(TaskId::from(7), "Shipped");
    done.done = true;
    assert_eq!(text(&done)[1], "[7]  #B  done");
    assert_eq!(
        text(&Task::new(TaskId::from(5), "Call the bank"))[1],
        "[5]  #B"
    );
    // The progress heading counts only when notes were left out.
    let clock = FixedClock::at("2026-10-04 09:30");
    let mut few = Task::new(TaskId::from(8), "Few");
    few.log("one", &clock);
    assert!(
        text(&few).contains(&"Progress".to_owned()),
        "{:?}",
        text(&few)
    );
    let mut many = Task::new(TaskId::from(9), "Many");
    for i in 1..=10 {
        many.log(format!("note {i}"), &clock);
    }
    let lines = text(&many);
    assert!(
        lines.contains(&"Progress (last 8 of 10)".to_owned()),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.ends_with("note 1")),
        "the first two notes are left out: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.ends_with("note 3")), "{lines:?}");
}

#[test]
fn styles_the_character_snapshots_cannot_see() {
    // Detail headings are bold; the tag chip has the CLI's colours.
    let mut model = fixture(true);
    update(&mut model, Msg::ShowDetail);
    let terminal = render(&mut model, 120, 28);
    let (x, y) = find(&terminal, "Merge requests", 60);
    assert!(
        terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::BOLD)
    );
    let (x, y) = find(&terminal, "#ci", 60);
    let chip = &terminal.backend().buffer()[(x, y)];
    assert_eq!(chip.bg, Color::Indexed(24));
    assert_eq!(chip.fg, Color::Indexed(231));
    let mut plain = fixture(false);
    update(&mut plain, Msg::ShowDetail);
    let terminal = render(&mut plain, 120, 28);
    let (x, y) = find(&terminal, "#ci", 60);
    assert_eq!(terminal.backend().buffer()[(x, y)].bg, Color::Reset);

    // An error in the status bar is red and bold; an info line is plain.
    update(&mut model, Msg::Failed("boom".into()));
    let terminal = render(&mut model, 120, 28);
    let cell = &terminal.backend().buffer()[(0, 27)];
    assert_eq!(cell.symbol(), "b");
    assert_eq!(cell.fg, Color::Red);
    assert!(cell.modifier.contains(Modifier::BOLD));
    update(&mut model, Msg::Info("fine".into()));
    let terminal = render(&mut model, 120, 28);
    let cell = &terminal.backend().buffer()[(0, 27)];
    assert_eq!(cell.symbol(), "f");
    assert_eq!(cell.fg, Color::Reset);
    assert!(!cell.modifier.contains(Modifier::BOLD));

    // Only the picker's cursor row is reversed.
    update(&mut model, Msg::BeginStatus);
    let terminal = render(&mut model, 120, 28);
    let (x, y) = find(&terminal, "1 in-progress", 0);
    assert!(
        terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    let (x, y) = find(&terminal, "2 ready", 0);
    assert!(
        !terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::REVERSED)
    );
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
    let mut themed = Model::new(Workflow::default(), Theme::from_config(&ui), true)
        .with_today(date("2026-10-06"));
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

#[test]
fn presets_restyle_every_role() {
    // Light: real colours where dark used attributes, so nothing is faint.
    let mut ui = UiConfig::default();
    ui.theme.preset = Preset::Light;
    let mut light = Model::new(Workflow::default(), Theme::from_config(&ui), true)
        .with_today(date("2026-10-06"));
    update(&mut light, Msg::Loaded(tasks()));
    update(&mut light, Msg::ShowDetail);
    let terminal = render(&mut light, 120, 28);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 1)].fg, Color::Indexed(25), "IN PROGRESS header");
    let row = &buffer[(3, 2)];
    assert_eq!(row.symbol(), "[");
    assert!(row.modifier.contains(Modifier::REVERSED));
    assert_eq!(row.fg, Color::Indexed(245), "the id in the light grey");
    let (x, y) = find(&terminal, "#ci", 60);
    assert_eq!(buffer[(x, y)].bg, Color::Indexed(153));
    assert_eq!(buffer[(x, y)].fg, Color::Indexed(17));
    for cell in buffer.content() {
        assert!(!cell.modifier.contains(Modifier::DIM), "{cell:?}");
    }
    update(&mut light, Msg::Failed("boom".into()));
    let terminal = render(&mut light, 120, 28);
    assert_eq!(terminal.backend().buffer()[(0, 27)].fg, Color::Indexed(124));
    update(&mut light, Msg::Edit);
    let terminal = render(&mut light, 120, 24);
    let focused = &terminal.backend().buffer()[find(&terminal, "Title", 0)];
    assert_eq!(focused.fg, Color::Indexed(25), "the focus role");
    assert!(focused.modifier.contains(Modifier::BOLD));

    // Mono: attributes only, the chip reversed; [ui.theme.colors] on top
    // turns the selection into a background colour.
    ui.theme.preset = Preset::Mono;
    ui.theme.colors.insert("selection".into(), "236".into());
    let mut mono = Model::new(Workflow::default(), Theme::from_config(&ui), true)
        .with_today(date("2026-10-06"));
    update(&mut mono, Msg::Loaded(tasks()));
    update(&mut mono, Msg::ShowDetail);
    let terminal = render(&mut mono, 120, 28);
    let buffer = terminal.backend().buffer();
    let header = &buffer[(1, 1)];
    assert_eq!(header.fg, Color::Reset);
    assert!(header.modifier.contains(Modifier::BOLD));
    let row = &buffer[(3, 2)];
    assert_eq!(row.bg, Color::Indexed(236), "selection as a background");
    assert!(!row.modifier.contains(Modifier::REVERSED));
    assert!(row.modifier.contains(Modifier::DIM), "the id stays dim");
    let (x, y) = find(&terminal, "#ci", 60);
    assert!(buffer[(x, y)].modifier.contains(Modifier::REVERSED));
    for cell in buffer.content() {
        assert_eq!(cell.fg, Color::Reset, "{cell:?}");
        assert!(
            cell.bg == Color::Reset || cell.bg == Color::Indexed(236),
            "{cell:?}"
        );
    }
    // The same text in every theme.
    let mut dark = fixture(true);
    update(&mut dark, Msg::ShowDetail);
    assert_eq!(
        screen(&mut mono, 120, 28),
        screen(&mut dark, 120, 28),
        "a theme must not change the layout"
    );
}
