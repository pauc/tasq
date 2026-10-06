//! Key bindings: a crossterm key event becomes a [`Msg`] according to the
//! current [`Mode`] and the configured [`KeyMap`] (ADR-0013). Pure, so
//! every binding is a one-line test.
//!
//! An [`Action`] is what a key does; `[ui.keys]` in the config maps action
//! names to key specs (`"ctrl+enter"`, `["j", "down"]`, `[]` to unbind),
//! which [`Chord::parse`] turns into [`Chord`]s. [`KeyMap::default`] is the
//! built-in map; [`KeyMap::from_config`] overlays the table on it. Typing
//! (the filter, a note, a title, with readline's cursor keys), the edit
//! view, the calendar, the help overlay and `Ctrl+C` are fixed.

use std::collections::BTreeMap;
use std::fmt;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tasq_core::config::KeySpec;

use crate::model::Mode;
use crate::msg::Msg;

/// Something a key can do. [`Action::name`] is the `[ui.keys]` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Move the selection or the picker cursor up.
    Up,
    /// Move the selection or the picker cursor down.
    Down,
    /// Move the selection a page up.
    PageUp,
    /// Move the selection a page down.
    PageDown,
    /// Select the first task.
    Top,
    /// Select the last task.
    Bottom,
    /// Start typing a filter.
    Filter,
    /// Open the status picker.
    Status,
    /// Open the priority picker.
    Priority,
    /// Start typing a progress note.
    Log,
    /// Start typing the final note, then close the task.
    Done,
    /// Start typing the title of a new task.
    Create,
    /// Open the edit form on the selected task.
    Edit,
    /// Open the task's file in the editor.
    Editor,
    /// Open a work session in this terminal.
    Launch,
    /// Open a work session in a new window and switch to it.
    LaunchDetached,
    /// Open a work session in a new window and stay.
    LaunchDetachedStay,
    /// Run the sources that run by default (`auto = true`).
    Sync,
    /// Choose the sources to run.
    Sources,
    /// Reload from the store.
    Reload,
    /// Show the help overlay.
    Help,
    /// Switch between list and detail.
    ToggleDetail,
    /// Show the selected task's detail.
    ShowDetail,
    /// Hide the detail.
    HideDetail,
    /// Clear the filter, close the detail or a dialog.
    Cancel,
    /// Apply the choice in a picker.
    Confirm,
    /// Leave; in a picker, close it.
    Quit,
}

/// The actions that apply in normal mode (the list), in table order.
pub const NORMAL: &[Action] = &[
    Action::Up,
    Action::Down,
    Action::PageUp,
    Action::PageDown,
    Action::Top,
    Action::Bottom,
    Action::Filter,
    Action::Status,
    Action::Priority,
    Action::Log,
    Action::Done,
    Action::Create,
    Action::Edit,
    Action::Editor,
    Action::Launch,
    Action::LaunchDetached,
    Action::LaunchDetachedStay,
    Action::Sync,
    Action::Sources,
    Action::Reload,
    Action::Help,
    Action::ToggleDetail,
    Action::ShowDetail,
    Action::HideDetail,
    Action::Cancel,
    Action::Quit,
];

/// The actions that apply in the status and priority pickers.
pub const PICKER: &[Action] = &[
    Action::Up,
    Action::Down,
    Action::Cancel,
    Action::Quit,
    Action::Confirm,
];

impl Action {
    /// Every action, in the order of the config reference.
    pub const ALL: &[Action] = &[
        Action::Up,
        Action::Down,
        Action::PageUp,
        Action::PageDown,
        Action::Top,
        Action::Bottom,
        Action::Filter,
        Action::Status,
        Action::Priority,
        Action::Log,
        Action::Done,
        Action::Create,
        Action::Edit,
        Action::Editor,
        Action::Launch,
        Action::LaunchDetached,
        Action::LaunchDetachedStay,
        Action::Sync,
        Action::Sources,
        Action::Reload,
        Action::Help,
        Action::ToggleDetail,
        Action::ShowDetail,
        Action::HideDetail,
        Action::Cancel,
        Action::Confirm,
        Action::Quit,
    ];

    /// The `[ui.keys]` name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::PageUp => "page-up",
            Self::PageDown => "page-down",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Filter => "filter",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::Log => "log",
            Self::Done => "done",
            Self::Create => "create",
            Self::Edit => "edit",
            Self::Editor => "editor",
            Self::Launch => "launch",
            Self::LaunchDetached => "launch-detached",
            Self::LaunchDetachedStay => "launch-detached-stay",
            Self::Sync => "sync",
            Self::Sources => "sources",
            Self::Reload => "reload",
            Self::Help => "help",
            Self::ToggleDetail => "toggle-detail",
            Self::ShowDetail => "show-detail",
            Self::HideDetail => "hide-detail",
            Self::Cancel => "cancel",
            Self::Confirm => "confirm",
            Self::Quit => "quit",
        }
    }

    /// The action called `name`, if any.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.name() == name)
    }

    /// The built-in keys, as specs.
    fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::Up => &["k", "up"],
            Self::Down => &["j", "down"],
            Self::PageUp => &["ctrl+u", "pgup"],
            Self::PageDown => &["ctrl+d", "pgdn"],
            Self::Top => &["g", "home"],
            Self::Bottom => &["G", "end"],
            Self::Filter => &["/"],
            Self::Status => &["t"],
            Self::Priority => &["p"],
            Self::Log => &["l"],
            Self::Done => &["d"],
            Self::Create => &["c"],
            Self::Edit => &["e"],
            Self::Editor => &["E"],
            Self::Launch | Self::Confirm => &["enter"],
            Self::LaunchDetached => &["ctrl+enter"],
            Self::LaunchDetachedStay => &["shift+enter"],
            Self::Sync => &["s"],
            Self::Sources => &["S"],
            Self::Reload => &["r"],
            Self::Help => &["?"],
            Self::ToggleDetail => &["tab"],
            Self::ShowDetail => &["right"],
            Self::HideDetail => &["left"],
            Self::Cancel => &["esc"],
            Self::Quit => &["q"],
        }
    }

    /// The message of the action in normal mode.
    fn msg(self) -> Msg {
        match self {
            Self::Up => Msg::Up,
            Self::Down => Msg::Down,
            Self::PageUp => Msg::PageUp,
            Self::PageDown => Msg::PageDown,
            Self::Top => Msg::Top,
            Self::Bottom => Msg::Bottom,
            Self::Filter => Msg::BeginFilter,
            Self::Status => Msg::BeginStatus,
            Self::Priority => Msg::BeginPriority,
            Self::Log => Msg::BeginNote,
            Self::Done => Msg::BeginDone,
            Self::Create => Msg::BeginCreate,
            Self::Edit => Msg::Edit,
            Self::Editor => Msg::Editor,
            Self::Launch => Msg::Launch,
            Self::LaunchDetached => Msg::LaunchDetached { focus: true },
            Self::LaunchDetachedStay => Msg::LaunchDetached { focus: false },
            Self::Sync => Msg::Sync,
            Self::Sources => Msg::BeginSources,
            Self::Reload => Msg::Reload,
            Self::Help => Msg::Help,
            Self::ToggleDetail => Msg::ToggleDetail,
            Self::ShowDetail => Msg::ShowDetail,
            Self::HideDetail => Msg::HideDetail,
            Self::Cancel => Msg::Escape,
            Self::Confirm => Msg::Enter,
            Self::Quit => Msg::Quit,
        }
    }
}

/// A key without its modifiers. A shifted character is the character
/// itself (`G`), as the terminal sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// A printable character, `Space` included.
    Char(char),
    /// `enter`.
    Enter,
    /// `esc`.
    Esc,
    /// `tab` (`shift+tab` is what crossterm calls `BackTab`).
    Tab,
    /// `backspace`.
    Backspace,
    /// `up`.
    Up,
    /// `down`.
    Down,
    /// `left`.
    Left,
    /// `right`.
    Right,
    /// `home`.
    Home,
    /// `end`.
    End,
    /// `pgup`.
    PageUp,
    /// `pgdn`.
    PageDown,
    /// `del`.
    Delete,
    /// `ins`.
    Insert,
    /// `f1` to `f12`.
    F(u8),
}

/// The named keys and their spellings in a spec.
const NAMES: &[(&str, Key)] = &[
    ("enter", Key::Enter),
    ("esc", Key::Esc),
    ("tab", Key::Tab),
    ("backspace", Key::Backspace),
    ("space", Key::Char(' ')),
    ("up", Key::Up),
    ("down", Key::Down),
    ("left", Key::Left),
    ("right", Key::Right),
    ("home", Key::Home),
    ("end", Key::End),
    ("pgup", Key::PageUp),
    ("pgdn", Key::PageDown),
    ("del", Key::Delete),
    ("ins", Key::Insert),
];

impl Key {
    fn from_code(code: KeyCode) -> Option<Self> {
        Some(match code {
            KeyCode::Char(c) => Self::Char(c),
            KeyCode::Enter => Self::Enter,
            KeyCode::Esc => Self::Esc,
            KeyCode::Tab | KeyCode::BackTab => Self::Tab,
            KeyCode::Backspace => Self::Backspace,
            KeyCode::Up => Self::Up,
            KeyCode::Down => Self::Down,
            KeyCode::Left => Self::Left,
            KeyCode::Right => Self::Right,
            KeyCode::Home => Self::Home,
            KeyCode::End => Self::End,
            KeyCode::PageUp => Self::PageUp,
            KeyCode::PageDown => Self::PageDown,
            KeyCode::Delete => Self::Delete,
            KeyCode::Insert => Self::Insert,
            KeyCode::F(n) => Self::F(n),
            _ => return None,
        })
    }

    /// `name`, lowercased by the caller, as a key: one character, a
    /// name from [`NAMES`] or `f1`..`f12`.
    fn parse(name: &str, original: &str) -> Option<Self> {
        let mut chars = original.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return (!c.is_whitespace() && !c.is_control()).then_some(Self::Char(c));
        }
        if let Some((_, key)) = NAMES.iter().find(|(n, _)| *n == name) {
            return Some(*key);
        }
        let n: u8 = name.strip_prefix('f')?.parse().ok()?;
        (1..=12).contains(&n).then_some(Self::F(n))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Char(' ') => f.write_str("Space"),
            Self::Char(c) => write!(f, "{c}"),
            Self::Enter => f.write_str("Enter"),
            Self::Esc => f.write_str("Esc"),
            Self::Tab => f.write_str("Tab"),
            Self::Backspace => f.write_str("Backspace"),
            Self::Up => f.write_str("Up"),
            Self::Down => f.write_str("Down"),
            Self::Left => f.write_str("Left"),
            Self::Right => f.write_str("Right"),
            Self::Home => f.write_str("Home"),
            Self::End => f.write_str("End"),
            Self::PageUp => f.write_str("PgUp"),
            Self::PageDown => f.write_str("PgDn"),
            Self::Delete => f.write_str("Del"),
            Self::Insert => f.write_str("Ins"),
            Self::F(n) => write!(f, "F{n}"),
        }
    }
}

/// A key with its modifiers: what one `[ui.keys]` string names and what
/// one key press is. Displays as `C-A-S-Key` (`C-Enter`, `S-Tab`, `G`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Chord {
    /// `ctrl+`.
    pub ctrl: bool,
    /// `alt+`.
    pub alt: bool,
    /// `shift+`; never set on a [`Key::Char`].
    pub shift: bool,
    /// The key.
    pub key: Key,
}

impl Chord {
    /// Parses `[ctrl+][alt+][shift+]<key>`, modifiers in any order and
    /// case; the key is one character or a lowercase-insensitive name.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        let (mods, name) = match spec.rsplit_once('+') {
            None => (None, spec),
            Some((mods, "")) => match mods.strip_suffix('+') {
                Some(mods) => (Some(mods), "+"),
                None if mods.is_empty() => (None, "+"),
                None => return Err(format!("\"{spec}\": no key after the last `+`")),
            },
            Some((mods, name)) => (Some(mods), name),
        };
        let mut chord = Self {
            ctrl: false,
            alt: false,
            shift: false,
            key: Key::Enter,
        };
        for m in mods.into_iter().flat_map(|m| m.split('+')) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" => chord.ctrl = true,
                "alt" => chord.alt = true,
                "shift" => chord.shift = true,
                other => {
                    return Err(format!(
                        "\"{spec}\": unknown modifier `{other}` (ctrl, alt, shift)"
                    ));
                }
            }
        }
        let Some(key) = Key::parse(&name.to_ascii_lowercase(), name) else {
            return Err(format!(
                "\"{spec}\": unknown key `{name}` (a character, or enter, esc, tab, backspace, \
                 space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)"
            ));
        };
        if chord.shift && matches!(key, Key::Char(_)) {
            return Err(format!(
                "\"{spec}\": shift goes with a named key; write the shifted character itself"
            ));
        }
        chord.key = key;
        Ok(chord)
    }

    /// The chord a key press is, or `None` for a key the map cannot name
    /// (media and modifier keys).
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        let key = Key::from_code(event.code)?;
        let shift = match event.code {
            KeyCode::Char(_) => false,
            KeyCode::BackTab => true,
            _ => event.modifiers.contains(KeyModifiers::SHIFT),
        };
        Some(Self {
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift,
            key,
        })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("C-")?;
        }
        if self.alt {
            f.write_str("A-")?;
        }
        if self.shift {
            f.write_str("S-")?;
        }
        write!(f, "{}", self.key)
    }
}

/// What is wrong with a `[ui.keys]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// A key that is not an action name.
    UnknownAction {
        /// The table key.
        name: String,
    },
    /// A key spec that does not parse.
    BadKey {
        /// The action it was given for.
        action: Action,
        /// Why, from [`Chord::parse`]; quotes the spec.
        reason: String,
    },
    /// One chord bound to two actions of the same mode.
    Conflict {
        /// The chord.
        chord: Chord,
        /// The action that has it first, in table order.
        first: Action,
        /// The action that also has it.
        second: Action,
    },
}

impl KeyError {
    /// The config path to blame: `ui.keys.<action>`, or `ui.keys` for a
    /// conflict.
    pub fn config_key(&self) -> String {
        match self {
            Self::UnknownAction { name } => format!("ui.keys.{name}"),
            Self::BadKey { action, .. } => format!("ui.keys.{}", action.name()),
            Self::Conflict { second, .. } => format!("ui.keys.{}", second.name()),
        }
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAction { name } => {
                write!(f, "ui.keys.{name}: unknown action (see `tasq ui --help`)")
            }
            Self::BadKey { action, reason } => write!(f, "ui.keys.{}: {reason}", action.name()),
            Self::Conflict {
                chord,
                first,
                second,
            } => write!(
                f,
                "ui.keys.{}: {chord} is already bound to {}",
                second.name(),
                first.name()
            ),
        }
    }
}

impl std::error::Error for KeyError {}

/// The keys of every action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMap {
    bindings: BTreeMap<Action, Vec<Chord>>,
}

impl Default for KeyMap {
    /// The built-in bindings.
    fn default() -> Self {
        let bindings = Action::ALL
            .iter()
            .map(|action| {
                let chords = action
                    .defaults()
                    .iter()
                    .map(|spec| Chord::parse(spec).expect("built-in key specs parse"))
                    .collect();
                (*action, chords)
            })
            .collect();
        Self { bindings }
    }
}

impl KeyMap {
    /// The built-in map with `keys` (the `[ui.keys]` table) laid over it:
    /// a listed action gets exactly the keys given, an unlisted one keeps
    /// its defaults.
    pub fn from_config(keys: &BTreeMap<String, KeySpec>) -> Result<Self, KeyError> {
        let mut map = Self::default();
        for (name, spec) in keys {
            let action = Action::parse(name)
                .ok_or_else(|| KeyError::UnknownAction { name: name.clone() })?;
            let chords = spec
                .keys()
                .iter()
                .map(|s| Chord::parse(s).map_err(|reason| KeyError::BadKey { action, reason }))
                .collect::<Result<Vec<_>, _>>()?;
            map.bindings.insert(action, chords);
        }
        map.check(NORMAL)?;
        map.check(PICKER)?;
        Ok(map)
    }

    /// Errors when two of `actions` share a chord.
    fn check(&self, actions: &[Action]) -> Result<(), KeyError> {
        let mut seen: BTreeMap<Chord, Action> = BTreeMap::new();
        for &action in actions {
            for &chord in self.keys(action) {
                if let Some(&first) = seen.get(&chord) {
                    return Err(KeyError::Conflict {
                        chord,
                        first,
                        second: action,
                    });
                }
                seen.insert(chord, action);
            }
        }
        Ok(())
    }

    /// The keys of `action`, in config order; empty when unbound.
    pub fn keys(&self, action: Action) -> &[Chord] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    /// The keys of `action` for the help overlay: `j/Down`, or `none`.
    pub fn display(&self, action: Action) -> String {
        let keys = self.keys(action);
        if keys.is_empty() {
            return "none".to_owned();
        }
        keys.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("/")
    }

    /// The first key of `action` for the status-bar hints, when bound.
    pub fn hint(&self, action: Action) -> Option<String> {
        self.keys(action).first().map(ToString::to_string)
    }

    /// The action among `actions` that `event` is bound to.
    fn lookup(&self, actions: &[Action], event: &KeyEvent) -> Option<Action> {
        let chord = Chord::from_event(event)?;
        actions
            .iter()
            .copied()
            .find(|action| self.keys(*action).contains(&chord))
    }
}

/// The message for `key` in `mode`, or `None` when the key does nothing
/// there. Key releases never do anything; `Ctrl+C` always quits.
pub fn translate(keys: &KeyMap, mode: &Mode, key: &KeyEvent) -> Option<Msg> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') {
        return Some(Msg::Quit);
    }
    match mode {
        Mode::Normal => keys.lookup(NORMAL, key).map(Action::msg),
        Mode::Filter { .. } | Mode::Note { .. } | Mode::Create { .. } => prompt(key.code, ctrl),
        Mode::Status { .. } | Mode::Priority { .. } | Mode::Sources { .. } => picker(keys, key),
        Mode::Form(_) => form(key.code, ctrl),
        Mode::Calendar { .. } => calendar(key.code, ctrl),
        Mode::Help => Some(Msg::Escape),
    }
}

/// The calendar picker's fixed keys: the arrows move by day and week,
/// `PageUp`/`PageDown` by month, `t` to today, `Enter` picks, `Esc`
/// closes; nothing else does anything.
fn calendar(code: KeyCode, ctrl: bool) -> Option<Msg> {
    Some(match code {
        KeyCode::Up => Msg::Up,
        KeyCode::Down => Msg::Down,
        KeyCode::Left => Msg::Left,
        KeyCode::Right => Msg::Right,
        KeyCode::PageUp => Msg::PageUp,
        KeyCode::PageDown => Msg::PageDown,
        KeyCode::Char('t') if !ctrl => Msg::Today,
        KeyCode::Enter => Msg::Enter,
        KeyCode::Esc => Msg::Escape,
        _ => return None,
    })
}

/// The edit view's fixed keys: a prompt's keys plus `Up`/`Down`,
/// `Tab`/`Shift+Tab` between the rows, `Ctrl+S` to save.
fn form(code: KeyCode, ctrl: bool) -> Option<Msg> {
    Some(match code {
        KeyCode::Tab => Msg::NextField,
        KeyCode::BackTab => Msg::PrevField,
        KeyCode::Up => Msg::Up,
        KeyCode::Down => Msg::Down,
        KeyCode::Char('s') if ctrl => Msg::Save,
        _ => return prompt(code, ctrl),
    })
}

/// A one-line prompt's fixed keys (the filter, a note, a title): typing,
/// `Backspace` and `Delete`, `Left`/`Right`, `Home`/`End` (also
/// `Ctrl+A`/`Ctrl+E`, as in readline), `Enter` and `Esc`.
fn prompt(code: KeyCode, ctrl: bool) -> Option<Msg> {
    Some(match code {
        KeyCode::Enter => Msg::Enter,
        KeyCode::Esc => Msg::Escape,
        KeyCode::Backspace => Msg::Backspace,
        KeyCode::Delete => Msg::Delete,
        KeyCode::Left => Msg::Left,
        KeyCode::Right => Msg::Right,
        KeyCode::Home => Msg::Home,
        KeyCode::End => Msg::End,
        KeyCode::Char('a') if ctrl => Msg::Home,
        KeyCode::Char('e') if ctrl => Msg::End,
        KeyCode::Char(c) if !ctrl => Msg::Char(c),
        _ => return None,
    })
}

/// Navigation from the map (`quit` closes the picker), any other
/// character picks by number or letter (in the source picker: toggles,
/// `Space` included).
fn picker(keys: &KeyMap, key: &KeyEvent) -> Option<Msg> {
    match keys.lookup(PICKER, key) {
        Some(Action::Quit) => Some(Msg::Escape),
        Some(action) => Some(action.msg()),
        None => match key.code {
            KeyCode::Char(c) => Some(Msg::Char(c)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::Text;
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

    fn chord(spec: &str) -> Chord {
        Chord::parse(spec).unwrap()
    }

    fn config(entries: &[(&str, KeySpec)]) -> BTreeMap<String, KeySpec> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn one(s: &str) -> KeySpec {
        KeySpec::One(s.to_owned())
    }

    fn many(s: &[&str]) -> KeySpec {
        KeySpec::Many(s.iter().map(|s| (*s).to_owned()).collect())
    }

    #[test]
    fn normal_mode_bindings() {
        let m = Mode::Normal;
        let keys = KeyMap::default();
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
            (
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                Msg::Bottom,
            ),
            (key(KeyCode::End), Msg::Bottom),
            (ch('/'), Msg::BeginFilter),
            (ch('t'), Msg::BeginStatus),
            (ch('p'), Msg::BeginPriority),
            (ch('l'), Msg::BeginNote),
            (ch('d'), Msg::BeginDone),
            (ch('c'), Msg::BeginCreate),
            (ch('e'), Msg::Edit),
            (ch('E'), Msg::Editor),
            (key(KeyCode::Enter), Msg::Launch),
            (
                KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
                Msg::LaunchDetached { focus: true },
            ),
            (
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
                Msg::LaunchDetached { focus: false },
            ),
            (ch('s'), Msg::Sync),
            (ch('S'), Msg::BeginSources),
            (ch('r'), Msg::Reload),
            (ch('?'), Msg::Help),
            (key(KeyCode::Tab), Msg::ToggleDetail),
            (key(KeyCode::Right), Msg::ShowDetail),
            (key(KeyCode::Left), Msg::HideDetail),
            (ch('q'), Msg::Quit),
            (ctrl('c'), Msg::Quit),
            (key(KeyCode::Esc), Msg::Escape),
        ];
        for (event, expected) in cases {
            assert_eq!(translate(&keys, &m, &event), Some(expected), "{event:?}");
        }
        for unbound in [
            ch('x'),
            ch('u'),
            key(KeyCode::F(1)),
            ctrl('x'),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT),
            key(KeyCode::BackTab),
            key(KeyCode::Null),
        ] {
            assert_eq!(translate(&keys, &m, &unbound), None, "{unbound:?}");
        }
    }

    #[test]
    fn releases_are_ignored() {
        let keys = KeyMap::default();
        let mut release = ch('j');
        release.kind = KeyEventKind::Release;
        assert_eq!(translate(&keys, &Mode::Normal, &release), None);
        let mut repeat = ch('j');
        repeat.kind = KeyEventKind::Repeat;
        assert_eq!(translate(&keys, &Mode::Normal, &repeat), Some(Msg::Down));
    }

    #[test]
    fn text_modes_take_characters() {
        let keys = KeyMap::default();
        for mode in [
            Mode::Filter {
                input: Text::single(""),
            },
            Mode::Note {
                input: Text::single(""),
                target: NoteTarget::Log,
            },
            Mode::Create {
                input: Text::single(""),
            },
        ] {
            let t = |event: &KeyEvent| translate(&keys, &mode, event);
            assert_eq!(t(&ch('j')), Some(Msg::Char('j')));
            assert_eq!(t(&ch('c')), Some(Msg::Char('c')));
            assert_eq!(t(&ch('q')), Some(Msg::Char('q')));
            assert_eq!(t(&ch(' ')), Some(Msg::Char(' ')));
            assert_eq!(t(&key(KeyCode::Enter)), Some(Msg::Enter));
            assert_eq!(t(&key(KeyCode::Esc)), Some(Msg::Escape));
            assert_eq!(t(&key(KeyCode::Backspace)), Some(Msg::Backspace));
            assert_eq!(t(&ctrl('c')), Some(Msg::Quit));
            assert_eq!(t(&key(KeyCode::Up)), None);
            assert_eq!(t(&key(KeyCode::Tab)), None);
            assert_eq!(t(&ctrl('s')), None, "no save outside the edit view");
            assert_eq!(t(&ctrl('x')), None);
        }
    }

    #[test]
    fn text_modes_take_readline_cursor_keys() {
        let keys =
            KeyMap::from_config(&config(&[("hide-detail", many(&[])), ("top", one("x"))])).unwrap();
        for mode in [
            Mode::Filter {
                input: Text::single("ab"),
            },
            Mode::Note {
                input: Text::single("ab"),
                target: NoteTarget::Done,
            },
            Mode::Create {
                input: Text::single("ab"),
            },
        ] {
            for (event, expected) in [
                (key(KeyCode::Left), Msg::Left),
                (key(KeyCode::Right), Msg::Right),
                (key(KeyCode::Home), Msg::Home),
                (key(KeyCode::End), Msg::End),
                (key(KeyCode::Delete), Msg::Delete),
                (ctrl('a'), Msg::Home),
                (ctrl('e'), Msg::End),
                (ch('a'), Msg::Char('a')),
                (ch('e'), Msg::Char('e')),
            ] {
                assert_eq!(
                    translate(&keys, &mode, &event),
                    Some(expected),
                    "{mode:?} {event:?}"
                );
            }
        }
    }

    #[test]
    fn form_mode_keys_are_fixed() {
        let keys = KeyMap::from_config(&config(&[("up", one("x")), ("down", many(&[]))])).unwrap();
        let task = tasq_core::model::Task::new(tasq_core::model::TaskId::from(1), "T");
        let mode = Mode::Form(Box::new(crate::form::Form::of(
            &task,
            &tasq_core::model::Workflow::default(),
        )));
        for (event, expected) in [
            (key(KeyCode::Up), Msg::Up),
            (key(KeyCode::BackTab), Msg::PrevField),
            (key(KeyCode::Down), Msg::Down),
            (key(KeyCode::Tab), Msg::NextField),
            (key(KeyCode::Left), Msg::Left),
            (key(KeyCode::Right), Msg::Right),
            (key(KeyCode::Home), Msg::Home),
            (key(KeyCode::End), Msg::End),
            (ctrl('a'), Msg::Home),
            (ctrl('e'), Msg::End),
            (key(KeyCode::Delete), Msg::Delete),
            (ctrl('s'), Msg::Save),
            (ch('a'), Msg::Char('a')),
            (ch('e'), Msg::Char('e')),
            (key(KeyCode::Enter), Msg::Enter),
            (key(KeyCode::Esc), Msg::Escape),
            (key(KeyCode::Backspace), Msg::Backspace),
            (ch('x'), Msg::Char('x')),
            (ch('j'), Msg::Char('j')),
            (ch('s'), Msg::Char('s')),
            (ch(' '), Msg::Char(' ')),
            (ctrl('c'), Msg::Quit),
        ] {
            assert_eq!(translate(&keys, &mode, &event), Some(expected), "{event:?}");
        }
        assert_eq!(translate(&keys, &mode, &ctrl('u')), None);
        assert_eq!(translate(&keys, &mode, &key(KeyCode::F(1))), None);
    }

    #[test]
    fn calendar_mode_keys_are_fixed() {
        let keys = KeyMap::from_config(&config(&[("up", one("x")), ("down", many(&[]))])).unwrap();
        let task = tasq_core::model::Task::new(tasq_core::model::TaskId::from(1), "T");
        let mode = Mode::Calendar {
            form: Box::new(crate::form::Form::of(
                &task,
                &tasq_core::model::Workflow::default(),
            )),
            calendar: crate::calendar::Calendar {
                day: chrono::NaiveDate::default(),
            },
        };
        for (event, expected) in [
            (key(KeyCode::Up), Msg::Up),
            (key(KeyCode::Down), Msg::Down),
            (key(KeyCode::Left), Msg::Left),
            (key(KeyCode::Right), Msg::Right),
            (key(KeyCode::PageUp), Msg::PageUp),
            (key(KeyCode::PageDown), Msg::PageDown),
            (ch('t'), Msg::Today),
            (key(KeyCode::Enter), Msg::Enter),
            (key(KeyCode::Esc), Msg::Escape),
            (ctrl('c'), Msg::Quit),
        ] {
            assert_eq!(translate(&keys, &mode, &event), Some(expected), "{event:?}");
        }
        for event in [
            ch('x'),
            ch('T'),
            ch(' '),
            ch('j'),
            ctrl('t'),
            ctrl('s'),
            key(KeyCode::Tab),
            key(KeyCode::BackTab),
            key(KeyCode::Home),
            key(KeyCode::End),
            key(KeyCode::Backspace),
            key(KeyCode::Delete),
            key(KeyCode::F(1)),
        ] {
            assert_eq!(translate(&keys, &mode, &event), None, "{event:?}");
        }
    }

    #[test]
    fn picker_modes() {
        let keys = KeyMap::default();
        for mode in [Mode::Status { cursor: 0 }, Mode::Priority { cursor: 0 }] {
            let t = |event: &KeyEvent| translate(&keys, &mode, event);
            assert_eq!(t(&ch('j')), Some(Msg::Down));
            assert_eq!(t(&key(KeyCode::Down)), Some(Msg::Down));
            assert_eq!(t(&ch('k')), Some(Msg::Up));
            assert_eq!(t(&key(KeyCode::Up)), Some(Msg::Up));
            assert_eq!(t(&key(KeyCode::Enter)), Some(Msg::Enter));
            assert_eq!(t(&key(KeyCode::Esc)), Some(Msg::Escape));
            assert_eq!(t(&ch('q')), Some(Msg::Escape), "quit closes the picker");
            assert_eq!(t(&ch('2')), Some(Msg::Char('2')));
            assert_eq!(t(&ch('A')), Some(Msg::Char('A')));
            assert_eq!(t(&ch('s')), Some(Msg::Char('s')), "list-only keys type");
            assert_eq!(t(&ctrl('c')), Some(Msg::Quit));
            assert_eq!(t(&key(KeyCode::Tab)), None);
            assert_eq!(t(&key(KeyCode::Null)), None);
        }
    }

    #[test]
    fn help_closes_on_any_key() {
        let keys = KeyMap::default();
        assert_eq!(translate(&keys, &Mode::Help, &ch('?')), Some(Msg::Escape));
        assert_eq!(
            translate(&keys, &Mode::Help, &key(KeyCode::Esc)),
            Some(Msg::Escape)
        );
        assert_eq!(translate(&keys, &Mode::Help, &ch('j')), Some(Msg::Escape));
        assert_eq!(translate(&keys, &Mode::Help, &ctrl('c')), Some(Msg::Quit));
    }

    #[test]
    fn action_names_round_trip() {
        for action in Action::ALL {
            assert_eq!(Action::parse(action.name()), Some(*action));
        }
        assert_eq!(
            Action::parse("launch-detached"),
            Some(Action::LaunchDetached)
        );
        assert_eq!(Action::parse("Launch"), None);
        assert_eq!(Action::parse(""), None);
        let mut names: Vec<_> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Action::ALL.len(), "names are unique");
        assert_eq!(Action::ALL.len(), 27);
        assert_eq!(NORMAL.len(), 26);
        assert!(!NORMAL.contains(&Action::Confirm));
        assert_eq!(
            PICKER,
            [
                Action::Up,
                Action::Down,
                Action::Cancel,
                Action::Quit,
                Action::Confirm
            ]
        );
    }

    fn mods(ctrl: bool, alt: bool, shift: bool, key: Key) -> Chord {
        Chord {
            ctrl,
            alt,
            shift,
            key,
        }
    }

    #[test]
    fn chords_parse_and_display() {
        let plain = |key| mods(false, false, false, key);
        let cases: &[(&str, Chord, &str)] = &[
            ("j", plain(Key::Char('j')), "j"),
            ("G", plain(Key::Char('G')), "G"),
            ("/", plain(Key::Char('/')), "/"),
            ("?", plain(Key::Char('?')), "?"),
            ("+", plain(Key::Char('+')), "+"),
            ("ctrl++", mods(true, false, false, Key::Char('+')), "C-+"),
            ("space", plain(Key::Char(' ')), "Space"),
            (" enter ", plain(Key::Enter), "Enter"),
            ("Enter", plain(Key::Enter), "Enter"),
            ("esc", plain(Key::Esc), "Esc"),
            ("tab", plain(Key::Tab), "Tab"),
            ("backspace", plain(Key::Backspace), "Backspace"),
            ("up", plain(Key::Up), "Up"),
            ("down", plain(Key::Down), "Down"),
            ("left", plain(Key::Left), "Left"),
            ("right", plain(Key::Right), "Right"),
            ("home", plain(Key::Home), "Home"),
            ("end", plain(Key::End), "End"),
            ("pgup", plain(Key::PageUp), "PgUp"),
            ("pgdn", plain(Key::PageDown), "PgDn"),
            ("del", plain(Key::Delete), "Del"),
            ("ins", plain(Key::Insert), "Ins"),
            ("f1", plain(Key::F(1)), "F1"),
            ("F12", plain(Key::F(12)), "F12"),
            (
                "ctrl+enter",
                mods(true, false, false, Key::Enter),
                "C-Enter",
            ),
            (
                "Ctrl+Enter",
                mods(true, false, false, Key::Enter),
                "C-Enter",
            ),
            (
                "shift+enter",
                mods(false, false, true, Key::Enter),
                "S-Enter",
            ),
            ("alt+enter", mods(false, true, false, Key::Enter), "A-Enter"),
            ("ctrl+u", mods(true, false, false, Key::Char('u')), "C-u"),
            ("alt+j", mods(false, true, false, Key::Char('j')), "A-j"),
            (
                "shift+alt+ctrl+tab",
                mods(true, true, true, Key::Tab),
                "C-A-S-Tab",
            ),
        ];
        for (spec, expected, shown) in cases {
            let chord = Chord::parse(spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
            assert_eq!(chord, *expected, "{spec}");
            assert_eq!(chord.to_string(), *shown, "{spec}");
        }
    }

    #[test]
    fn bad_chords() {
        let cases = [
            (
                "",
                "\"\": unknown key `` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            ("ctrl+", "\"ctrl+\": no key after the last `+`"),
            (
                "meta+x",
                "\"meta+x\": unknown modifier `meta` (ctrl, alt, shift)",
            ),
            ("ctrl+ctrl+", "\"ctrl+ctrl+\": no key after the last `+`"),
            (
                "ctrl++x",
                "\"ctrl++x\": unknown modifier `` (ctrl, alt, shift)",
            ),
            ("+x", "\"+x\": unknown modifier `` (ctrl, alt, shift)"),
            ("++", "\"++\": unknown modifier `` (ctrl, alt, shift)"),
            (
                "enterr",
                "\"enterr\": unknown key `enterr` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            (
                "f0",
                "\"f0\": unknown key `f0` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            (
                "f13",
                "\"f13\": unknown key `f13` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            (
                "fx",
                "\"fx\": unknown key `fx` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            (
                "\t",
                "\"\": unknown key `` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)",
            ),
            (
                "shift+g",
                "\"shift+g\": shift goes with a named key; write the shifted character itself",
            ),
            (
                "shift+G",
                "\"shift+G\": shift goes with a named key; write the shifted character itself",
            ),
            (
                "shift+space",
                "\"shift+space\": shift goes with a named key; write the shifted character itself",
            ),
        ];
        for (spec, message) in cases {
            assert_eq!(Chord::parse(spec), Err(message.to_owned()), "{spec}");
        }
        assert_eq!(
            Chord::parse("\u{7f}").unwrap_err(),
            "\"\u{7f}\": unknown key `\u{7f}` (a character, or enter, esc, tab, backspace, space, up, down, left, right, home, end, pgup, pgdn, del, ins, f1..f12)"
        );
    }

    #[test]
    fn chords_from_events() {
        let ev = |code, mods| Chord::from_event(&KeyEvent::new(code, mods));
        assert_eq!(ev(KeyCode::Char('j'), KeyModifiers::NONE), Some(chord("j")));
        assert_eq!(
            ev(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(chord("G")),
            "shift is in the character"
        );
        assert_eq!(
            ev(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(chord("ctrl+u"))
        );
        assert_eq!(
            ev(KeyCode::Char('j'), KeyModifiers::ALT),
            Some(chord("alt+j"))
        );
        assert_eq!(
            ev(KeyCode::Enter, KeyModifiers::SHIFT),
            Some(chord("shift+enter"))
        );
        assert_eq!(
            ev(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            Some(chord("ctrl+shift+enter"))
        );
        assert_eq!(
            ev(KeyCode::BackTab, KeyModifiers::NONE),
            Some(chord("shift+tab"))
        );
        assert_eq!(
            ev(KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(chord("shift+tab"))
        );
        assert_eq!(ev(KeyCode::Tab, KeyModifiers::NONE), Some(chord("tab")));
        assert_eq!(ev(KeyCode::F(5), KeyModifiers::NONE), Some(chord("f5")));
        assert_eq!(ev(KeyCode::Delete, KeyModifiers::NONE), Some(chord("del")));
        assert_eq!(ev(KeyCode::Insert, KeyModifiers::NONE), Some(chord("ins")));
        assert_eq!(ev(KeyCode::Left, KeyModifiers::NONE), Some(chord("left")));
        assert_eq!(ev(KeyCode::Right, KeyModifiers::NONE), Some(chord("right")));
        assert_eq!(
            ev(KeyCode::Backspace, KeyModifiers::NONE),
            Some(chord("backspace"))
        );
        assert_eq!(ev(KeyCode::Null, KeyModifiers::NONE), None);
        assert_eq!(ev(KeyCode::CapsLock, KeyModifiers::NONE), None);
    }

    #[test]
    fn default_map() {
        let keys = KeyMap::default();
        assert_eq!(keys.keys(Action::Up), [chord("k"), chord("up")]);
        assert_eq!(keys.keys(Action::Down), [chord("j"), chord("down")]);
        assert_eq!(keys.keys(Action::PageUp), [chord("ctrl+u"), chord("pgup")]);
        assert_eq!(
            keys.keys(Action::PageDown),
            [chord("ctrl+d"), chord("pgdn")]
        );
        assert_eq!(keys.keys(Action::Top), [chord("g"), chord("home")]);
        assert_eq!(keys.keys(Action::Bottom), [chord("G"), chord("end")]);
        assert_eq!(keys.keys(Action::Filter), [chord("/")]);
        assert_eq!(keys.keys(Action::Status), [chord("t")]);
        assert_eq!(keys.keys(Action::Priority), [chord("p")]);
        assert_eq!(keys.keys(Action::Log), [chord("l")]);
        assert_eq!(keys.keys(Action::Done), [chord("d")]);
        assert_eq!(keys.keys(Action::Create), [chord("c")]);
        assert_eq!(keys.keys(Action::Edit), [chord("e")]);
        assert_eq!(keys.keys(Action::Editor), [chord("E")]);
        assert_eq!(keys.keys(Action::Launch), [chord("enter")]);
        assert_eq!(keys.keys(Action::LaunchDetached), [chord("ctrl+enter")]);
        assert_eq!(
            keys.keys(Action::LaunchDetachedStay),
            [chord("shift+enter")]
        );
        assert_eq!(keys.keys(Action::Sync), [chord("s")]);
        assert_eq!(keys.keys(Action::Sources), [chord("S")]);
        assert_eq!(keys.keys(Action::Reload), [chord("r")]);
        assert_eq!(keys.keys(Action::Help), [chord("?")]);
        assert_eq!(keys.keys(Action::ToggleDetail), [chord("tab")]);
        assert_eq!(keys.keys(Action::Cancel), [chord("esc")]);
        assert_eq!(keys.keys(Action::Confirm), [chord("enter")]);
        assert_eq!(keys.keys(Action::Quit), [chord("q")]);
        assert_eq!(KeyMap::from_config(&BTreeMap::new()), Ok(keys));
    }

    #[test]
    fn display_and_hint() {
        let keys = KeyMap::from_config(&config(&[("sync", many(&[]))])).unwrap();
        assert_eq!(keys.display(Action::Up), "k/Up");
        assert_eq!(keys.display(Action::PageDown), "C-d/PgDn");
        assert_eq!(keys.display(Action::LaunchDetached), "C-Enter");
        assert_eq!(keys.display(Action::Sync), "none");
        assert_eq!(keys.hint(Action::Up), Some("k".to_owned()));
        assert_eq!(keys.hint(Action::Launch), Some("Enter".to_owned()));
        assert_eq!(keys.hint(Action::Sync), None);
        assert_eq!(keys.keys(Action::Sync), []);
    }

    #[test]
    fn config_rebinds_unbinds_and_keeps_the_rest() {
        let keys = KeyMap::from_config(&config(&[
            ("launch-detached", one("alt+enter")),
            (
                "launch-detached-stay",
                many(&["shift+enter", "alt+shift+enter"]),
            ),
            ("sync", many(&[])),
            ("quit", many(&["x"])),
        ]))
        .unwrap();
        assert_eq!(keys.keys(Action::LaunchDetached), [chord("alt+enter")]);
        assert_eq!(
            keys.keys(Action::LaunchDetachedStay),
            [chord("shift+enter"), chord("alt+shift+enter")]
        );
        assert_eq!(keys.keys(Action::Sync), []);
        assert_eq!(keys.keys(Action::Quit), [chord("x")]);
        assert_eq!(keys.keys(Action::Down), [chord("j"), chord("down")]);
        let m = Mode::Normal;
        let t = |event: &KeyEvent| translate(&keys, &m, event);
        assert_eq!(
            t(&KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
            Some(Msg::LaunchDetached { focus: true })
        );
        assert_eq!(
            t(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            None,
            "the default is replaced, not added to"
        );
        assert_eq!(
            t(&KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            Some(Msg::LaunchDetached { focus: false })
        );
        assert_eq!(t(&ch('s')), None, "unbound");
        assert_eq!(
            t(&ch('S')),
            Some(Msg::BeginSources),
            "its neighbour keeps its default"
        );
        assert_eq!(t(&ch('x')), Some(Msg::Quit));
        assert_eq!(t(&ch('q')), None);
        assert_eq!(t(&ctrl('c')), Some(Msg::Quit), "Ctrl+C is not configurable");
        assert_eq!(
            translate(&keys, &Mode::Status { cursor: 0 }, &ch('x')),
            Some(Msg::Escape),
            "the configured quit closes a picker"
        );
        assert_eq!(
            translate(&keys, &Mode::Status { cursor: 0 }, &ch('q')),
            Some(Msg::Char('q'))
        );
    }

    #[test]
    fn config_errors() {
        let err = |entries: &[(&str, KeySpec)]| KeyMap::from_config(&config(entries)).unwrap_err();
        let unknown = err(&[("lunch", one("enter"))]);
        assert_eq!(
            unknown,
            KeyError::UnknownAction {
                name: "lunch".to_owned()
            }
        );
        assert_eq!(unknown.config_key(), "ui.keys.lunch");
        assert_eq!(
            unknown.to_string(),
            "ui.keys.lunch: unknown action (see `tasq ui --help`)"
        );

        let bad = err(&[("quit", many(&["q", "meta+q"]))]);
        assert_eq!(
            bad,
            KeyError::BadKey {
                action: Action::Quit,
                reason: "\"meta+q\": unknown modifier `meta` (ctrl, alt, shift)".to_owned()
            }
        );
        assert_eq!(bad.config_key(), "ui.keys.quit");
        assert_eq!(
            bad.to_string(),
            "ui.keys.quit: \"meta+q\": unknown modifier `meta` (ctrl, alt, shift)"
        );

        let conflict = err(&[("sync", one("j"))]);
        assert_eq!(
            conflict,
            KeyError::Conflict {
                chord: chord("j"),
                first: Action::Down,
                second: Action::Sync
            }
        );
        assert_eq!(conflict.config_key(), "ui.keys.sync");
        assert_eq!(
            conflict.to_string(),
            "ui.keys.sync: j is already bound to down"
        );

        let picker = err(&[("confirm", one("esc"))]);
        assert_eq!(
            picker,
            KeyError::Conflict {
                chord: chord("esc"),
                first: Action::Cancel,
                second: Action::Confirm
            },
            "picker-only conflicts are caught too"
        );
        assert_eq!(picker.config_key(), "ui.keys.confirm");
        assert_eq!(
            err(&[("quit", one("enter"))]),
            KeyError::Conflict {
                chord: chord("enter"),
                first: Action::Launch,
                second: Action::Quit
            },
            "normal mode is checked before the pickers"
        );
        assert!(
            KeyMap::from_config(&config(&[("confirm", one("space"))])).is_ok(),
            "confirm may share a key with a list-only action"
        );
        assert!(
            KeyMap::from_config(&config(&[("confirm", one("l"))])).is_ok(),
            "confirm only conflicts inside the pickers"
        );
    }
}
