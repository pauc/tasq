//! Key bindings: a crossterm key event becomes a [`Msg`] according to the
//! current [`Mode`]. Pure, so every binding is a one-line test.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::model::Mode;
use crate::msg::Msg;

/// The message for `key` in `mode`, or `None` when the key does nothing
/// there. Key releases never do anything.
pub fn translate(mode: &Mode, key: &KeyEvent) -> Option<Msg> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') {
        return Some(Msg::Quit);
    }
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match mode {
        Mode::Normal => normal(key.code, ctrl, shift),
        Mode::Filter { .. } | Mode::Note { .. } | Mode::Create { .. } => text(key.code, ctrl),
        Mode::Status { .. } | Mode::Priority { .. } => picker(key.code),
        Mode::Help => Some(Msg::Escape),
    }
}

/// `Ctrl+Enter` and `Shift+Enter` only arrive as such from a terminal
/// that speaks the kitty keyboard protocol (the runtime asks for it);
/// elsewhere both are a plain `Enter`.
fn normal(code: KeyCode, ctrl: bool, shift: bool) -> Option<Msg> {
    Some(match code {
        KeyCode::Char('j') | KeyCode::Down => Msg::Down,
        KeyCode::Char('k') | KeyCode::Up => Msg::Up,
        KeyCode::Char('d') if ctrl => Msg::PageDown,
        KeyCode::Char('u') if ctrl => Msg::PageUp,
        KeyCode::PageDown => Msg::PageDown,
        KeyCode::PageUp => Msg::PageUp,
        KeyCode::Char('g') | KeyCode::Home => Msg::Top,
        KeyCode::Char('G') | KeyCode::End => Msg::Bottom,
        KeyCode::Char('/') => Msg::BeginFilter,
        KeyCode::Char('s') => Msg::BeginStatus,
        KeyCode::Char('p') => Msg::BeginPriority,
        KeyCode::Char('l') => Msg::BeginNote,
        KeyCode::Char('d') => Msg::BeginDone,
        KeyCode::Char('c') => Msg::BeginCreate,
        KeyCode::Char('e') => Msg::Edit,
        KeyCode::Enter if ctrl => Msg::LaunchDetached { focus: true },
        KeyCode::Enter if shift => Msg::LaunchDetached { focus: false },
        KeyCode::Enter => Msg::Launch,
        KeyCode::Char('S') => Msg::Sync,
        KeyCode::Char('r') => Msg::Reload,
        KeyCode::Char('?') => Msg::Help,
        KeyCode::Tab => Msg::ToggleDetail,
        KeyCode::Char('q') => Msg::Quit,
        KeyCode::Esc => Msg::Escape,
        _ => return None,
    })
}

fn text(code: KeyCode, ctrl: bool) -> Option<Msg> {
    Some(match code {
        KeyCode::Enter => Msg::Enter,
        KeyCode::Esc => Msg::Escape,
        KeyCode::Backspace => Msg::Backspace,
        KeyCode::Char(c) if !ctrl => Msg::Char(c),
        _ => return None,
    })
}

fn picker(code: KeyCode) -> Option<Msg> {
    Some(match code {
        KeyCode::Char('j') | KeyCode::Down => Msg::Down,
        KeyCode::Char('k') | KeyCode::Up => Msg::Up,
        KeyCode::Enter => Msg::Enter,
        KeyCode::Esc | KeyCode::Char('q') => Msg::Escape,
        KeyCode::Char(c) => Msg::Char(c),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NoteTarget;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    #[test]
    fn normal_mode_bindings() {
        let m = Mode::Normal;
        let cases = [
            (ch('j'), Msg::Down),
            (key(KeyCode::Down), Msg::Down),
            (ch('k'), Msg::Up),
            (key(KeyCode::Up), Msg::Up),
            (ctrl('d'), Msg::PageDown),
            (key(KeyCode::PageDown), Msg::PageDown),
            (ctrl('u'), Msg::PageUp),
            (key(KeyCode::PageUp), Msg::PageUp),
            (ch('g'), Msg::Top),
            (key(KeyCode::Home), Msg::Top),
            (ch('G'), Msg::Bottom),
            (key(KeyCode::End), Msg::Bottom),
            (ch('/'), Msg::BeginFilter),
            (ch('s'), Msg::BeginStatus),
            (ch('p'), Msg::BeginPriority),
            (ch('l'), Msg::BeginNote),
            (ch('d'), Msg::BeginDone),
            (ch('c'), Msg::BeginCreate),
            (ch('e'), Msg::Edit),
            (key(KeyCode::Enter), Msg::Launch),
            (
                KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
                Msg::LaunchDetached { focus: true },
            ),
            (
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
                Msg::LaunchDetached { focus: false },
            ),
            (
                KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                Msg::LaunchDetached { focus: true },
            ),
            (ch('S'), Msg::Sync),
            (ch('r'), Msg::Reload),
            (ch('?'), Msg::Help),
            (key(KeyCode::Tab), Msg::ToggleDetail),
            (ch('q'), Msg::Quit),
            (ctrl('c'), Msg::Quit),
            (key(KeyCode::Esc), Msg::Escape),
        ];
        for (event, expected) in cases {
            assert_eq!(translate(&m, &event), Some(expected), "{event:?}");
        }
        assert_eq!(translate(&m, &ch('x')), None);
        assert_eq!(translate(&m, &ch('u')), None, "only Ctrl-u pages");
        assert_eq!(translate(&m, &key(KeyCode::F(1))), None);
        assert_eq!(translate(&m, &ctrl('x')), None);
    }

    #[test]
    fn releases_are_ignored() {
        let mut release = ch('j');
        release.kind = KeyEventKind::Release;
        assert_eq!(translate(&Mode::Normal, &release), None);
        let mut repeat = ch('j');
        repeat.kind = KeyEventKind::Repeat;
        assert_eq!(translate(&Mode::Normal, &repeat), Some(Msg::Down));
    }

    #[test]
    fn text_modes_take_characters() {
        for mode in [
            Mode::Filter {
                input: String::new(),
            },
            Mode::Note {
                input: String::new(),
                target: NoteTarget::Log,
            },
            Mode::Create {
                input: String::new(),
            },
        ] {
            assert_eq!(translate(&mode, &ch('j')), Some(Msg::Char('j')));
            assert_eq!(translate(&mode, &ch('c')), Some(Msg::Char('c')));
            assert_eq!(translate(&mode, &ch('q')), Some(Msg::Char('q')));
            assert_eq!(translate(&mode, &ch(' ')), Some(Msg::Char(' ')));
            assert_eq!(translate(&mode, &key(KeyCode::Enter)), Some(Msg::Enter));
            assert_eq!(translate(&mode, &key(KeyCode::Esc)), Some(Msg::Escape));
            assert_eq!(
                translate(&mode, &key(KeyCode::Backspace)),
                Some(Msg::Backspace)
            );
            assert_eq!(translate(&mode, &ctrl('c')), Some(Msg::Quit));
            assert_eq!(translate(&mode, &ctrl('a')), None);
            assert_eq!(translate(&mode, &key(KeyCode::Up)), None);
            assert_eq!(translate(&mode, &key(KeyCode::Tab)), None);
        }
    }

    #[test]
    fn picker_modes() {
        for mode in [Mode::Status { cursor: 0 }, Mode::Priority { cursor: 0 }] {
            assert_eq!(translate(&mode, &ch('j')), Some(Msg::Down));
            assert_eq!(translate(&mode, &key(KeyCode::Down)), Some(Msg::Down));
            assert_eq!(translate(&mode, &ch('k')), Some(Msg::Up));
            assert_eq!(translate(&mode, &key(KeyCode::Up)), Some(Msg::Up));
            assert_eq!(translate(&mode, &key(KeyCode::Enter)), Some(Msg::Enter));
            assert_eq!(translate(&mode, &key(KeyCode::Esc)), Some(Msg::Escape));
            assert_eq!(translate(&mode, &ch('q')), Some(Msg::Escape));
            assert_eq!(translate(&mode, &ch('2')), Some(Msg::Char('2')));
            assert_eq!(translate(&mode, &ch('A')), Some(Msg::Char('A')));
            assert_eq!(translate(&mode, &ctrl('c')), Some(Msg::Quit));
            assert_eq!(translate(&mode, &key(KeyCode::Tab)), None);
        }
    }

    #[test]
    fn help_closes_on_any_key() {
        assert_eq!(translate(&Mode::Help, &ch('?')), Some(Msg::Escape));
        assert_eq!(
            translate(&Mode::Help, &key(KeyCode::Esc)),
            Some(Msg::Escape)
        );
        assert_eq!(translate(&Mode::Help, &ch('j')), Some(Msg::Escape));
        assert_eq!(translate(&Mode::Help, &ctrl('c')), Some(Msg::Quit));
    }
}
