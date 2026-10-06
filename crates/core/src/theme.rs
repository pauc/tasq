//! Colour semantics shared by the CLI and the TUI (plan section 8: "the
//! renderer is a separate module so the TUI and CLI share color semantics").
//!
//! A [`Color`] is a terminal colour name, a 256-colour palette index or one
//! of the three attribute pseudo-colours (`dim`, `reversed`, `none`), as
//! written in the config. A [`Role`] is one thing the front ends colour:
//! the status groups, the tag chip, the `#A` marker, the focus border, the
//! selection, errors, dim text, links and headings. A [`Theme`] maps every
//! role to a colour in three layers (ADR 0018): a built-in [`Preset`]
//! (`ui.theme.preset`), the role table `[ui.theme.colors]`, and the
//! per-status overrides of `[ui.colors]`. How a colour becomes escape codes
//! or widget styles is the front ends' business; this module holds only
//! the decisions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::UiConfig;
use crate::model::Status;

/// A colour a role can be rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// ANSI black.
    Black,
    /// ANSI red.
    Red,
    /// ANSI green.
    Green,
    /// ANSI yellow.
    Yellow,
    /// ANSI blue.
    Blue,
    /// ANSI magenta.
    Magenta,
    /// ANSI cyan.
    Cyan,
    /// ANSI white.
    White,
    /// Faint text rather than a colour.
    Dim,
    /// Swapped foreground and background rather than a colour.
    Reversed,
    /// No colour and no attribute: the terminal's default.
    Plain,
    /// A 256-colour palette index.
    Fixed(u8),
}

impl Color {
    /// Parses a colour name (`red`, `dim`, `grey`/`gray`, `reversed`,
    /// `none`, ...) or a palette index (`0`-`255`), as written under
    /// `[ui.colors]` and `[ui.theme.colors]`. Case and surrounding
    /// whitespace do not matter.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "black" => Self::Black,
            "red" => Self::Red,
            "green" => Self::Green,
            "yellow" => Self::Yellow,
            "blue" => Self::Blue,
            "magenta" => Self::Magenta,
            "cyan" => Self::Cyan,
            "white" => Self::White,
            "dim" | "grey" | "gray" => Self::Dim,
            "reversed" | "reverse" => Self::Reversed,
            "none" | "plain" | "default" => Self::Plain,
            number => Self::Fixed(number.parse().ok()?),
        })
    }
}

/// One thing the front ends colour. The `[ui.theme.colors]` key of each
/// role is [`Role::key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The `in-progress` status group.
    InProgress,
    /// The `ready` status group.
    Ready,
    /// The `waiting` status group.
    Waiting,
    /// The `blocked` status group.
    Blocked,
    /// The `later` status group.
    Later,
    /// Any other configured status.
    OtherStatus,
    /// The `NO STATUS` group.
    NoStatus,
    /// The `DONE` group of `tasq list --all` / `--done`.
    Done,
    /// The background of a tag chip.
    ChipBg,
    /// The text of a tag chip.
    ChipFg,
    /// The `#A` priority marker.
    PrioA,
    /// The focused field in the TUI: its border and label.
    Focus,
    /// The selected row and the chosen option in the TUI. A colour is a
    /// background; `reversed` swaps the colours instead.
    Selection,
    /// Error messages and the border of a refused field.
    Error,
    /// Secondary text: ids, dates, hints, weekends in the calendar.
    Dim,
    /// Hyperlinks in `tasq view`.
    Link,
    /// Titles and section headings in the detail pane and popups; bold is
    /// added by the front ends.
    Header,
}

impl Role {
    /// Every role, in `[ui.theme.colors]` documentation order.
    pub const ALL: [Self; 17] = [
        Self::InProgress,
        Self::Ready,
        Self::Waiting,
        Self::Blocked,
        Self::Later,
        Self::OtherStatus,
        Self::NoStatus,
        Self::Done,
        Self::ChipBg,
        Self::ChipFg,
        Self::PrioA,
        Self::Focus,
        Self::Selection,
        Self::Error,
        Self::Dim,
        Self::Link,
        Self::Header,
    ];

    /// The `[ui.theme.colors]` key of the role.
    pub fn key(self) -> &'static str {
        match self {
            Self::InProgress => "in-progress",
            Self::Ready => "ready",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Later => "later",
            Self::OtherStatus => "other-status",
            Self::NoStatus => NO_STATUS_KEY,
            Self::Done => DONE_KEY,
            Self::ChipBg => "chip-bg",
            Self::ChipFg => "chip-fg",
            Self::PrioA => "prio-a",
            Self::Focus => "focus",
            Self::Selection => "selection",
            Self::Error => "error",
            Self::Dim => "dim",
            Self::Link => "link",
            Self::Header => "header",
        }
    }

    /// The role whose [`key`](Self::key) is `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.key() == key)
    }

    /// The role of a status group (`None` is the no-status group).
    pub fn of_status(status: Option<&Status>) -> Self {
        match status {
            None => Self::NoStatus,
            Some(s) if *s == Status::IN_PROGRESS => Self::InProgress,
            Some(s) if *s == Status::READY => Self::Ready,
            Some(s) if *s == Status::WAITING => Self::Waiting,
            Some(s) if *s == Status::BLOCKED => Self::Blocked,
            Some(s) if *s == Status::LATER => Self::Later,
            Some(_) => Self::OtherStatus,
        }
    }
}

/// A built-in theme: a complete role table (`ui.theme.preset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// The original script's colours on a dark background.
    #[default]
    Dark,
    /// Darker shades and a real grey for dim text, readable on a light
    /// background.
    Light,
    /// Solarized (dark background), 256-colour approximations.
    Solarized,
    /// Gruvbox (dark background), 256-colour approximations.
    Gruvbox,
    /// No colours: bold, dim and reversed only.
    Mono,
}

impl Preset {
    /// Every preset, in documentation order.
    pub const ALL: [Self; 5] = [
        Self::Dark,
        Self::Light,
        Self::Solarized,
        Self::Gruvbox,
        Self::Mono,
    ];

    /// The config name of the preset.
    pub fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Solarized => "solarized",
            Self::Gruvbox => "gruvbox",
            Self::Mono => "mono",
        }
    }

    /// The colour of `role` in this preset.
    // One row per preset and role: the table reads by preset, merging arms
    // across presets would hide which preset a colour belongs to.
    #[allow(clippy::match_same_arms)]
    pub fn color(self, role: Role) -> Color {
        use Color::{Blue, Cyan, Dim, Fixed, Green, Magenta, Plain, Red, Reversed, Yellow};
        match (self, role) {
            (Self::Dark, Role::InProgress) => Blue,
            (Self::Dark, Role::Ready) => Green,
            (Self::Dark, Role::Waiting) => Yellow,
            (Self::Dark, Role::Blocked) => Red,
            (Self::Dark, Role::Later) => Magenta,
            (Self::Dark, Role::OtherStatus) => Cyan,
            (Self::Dark, Role::NoStatus | Role::Done | Role::Dim) => Dim,
            (Self::Dark, Role::ChipBg) => Fixed(24),
            (Self::Dark, Role::ChipFg) => Fixed(231),
            (Self::Dark, Role::PrioA | Role::Error) => Red,
            (Self::Dark, Role::Focus) => Cyan,
            (Self::Dark, Role::Link) => Fixed(75),

            (Self::Light, Role::InProgress | Role::Focus | Role::Link) => Fixed(25),
            (Self::Light, Role::Ready) => Fixed(28),
            (Self::Light, Role::Waiting) => Fixed(130),
            (Self::Light, Role::Blocked | Role::PrioA | Role::Error) => Fixed(124),
            (Self::Light, Role::Later) => Fixed(90),
            (Self::Light, Role::OtherStatus) => Fixed(30),
            (Self::Light, Role::NoStatus | Role::Done | Role::Dim) => Fixed(245),
            (Self::Light, Role::ChipBg) => Fixed(153),
            (Self::Light, Role::ChipFg) => Fixed(17),

            (Self::Solarized, Role::InProgress | Role::Link) => Fixed(33),
            (Self::Solarized, Role::Ready) => Fixed(64),
            (Self::Solarized, Role::Waiting) => Fixed(136),
            (Self::Solarized, Role::Blocked | Role::PrioA | Role::Error) => Fixed(160),
            (Self::Solarized, Role::Later) => Fixed(125),
            (Self::Solarized, Role::OtherStatus | Role::Focus) => Fixed(37),
            (Self::Solarized, Role::NoStatus | Role::Done | Role::Dim) => Fixed(240),
            (Self::Solarized, Role::ChipBg) => Fixed(236),
            (Self::Solarized, Role::ChipFg) => Fixed(245),

            (Self::Gruvbox, Role::InProgress | Role::Link) => Fixed(109),
            (Self::Gruvbox, Role::Ready) => Fixed(142),
            (Self::Gruvbox, Role::Waiting) => Fixed(214),
            (Self::Gruvbox, Role::Blocked | Role::PrioA | Role::Error) => Fixed(167),
            (Self::Gruvbox, Role::Later) => Fixed(175),
            (Self::Gruvbox, Role::OtherStatus | Role::Focus) => Fixed(108),
            (Self::Gruvbox, Role::NoStatus | Role::Done | Role::Dim) => Fixed(245),
            (Self::Gruvbox, Role::ChipBg) => Fixed(237),
            (Self::Gruvbox, Role::ChipFg) => Fixed(223),

            (Self::Mono, Role::NoStatus | Role::Done | Role::Dim) => Dim,
            (Self::Mono, Role::ChipBg) => Reversed,

            (_, Role::Selection) => Reversed,
            (_, Role::Header) | (Self::Mono, _) => Plain,
        }
    }
}

/// The `[ui.colors]` key of the no-status group.
pub const NO_STATUS_KEY: &str = "no-status";
/// The `[ui.colors]` key of the done group (`tasq list --all` / `--done`).
pub const DONE_KEY: &str = "done";

/// The colour of every [`Role`]: a [`Preset`] under `[ui.theme.colors]`
/// under `[ui.colors]`. The last layer is keyed by status name, so it can
/// colour a status the preset does not know (`review = "red"`), and reads
/// [`NO_STATUS_KEY`] and [`DONE_KEY`] for the two groups that are not a
/// status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    preset: Preset,
    roles: BTreeMap<Role, Color>,
    statuses: BTreeMap<String, Color>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_config(&UiConfig::default())
    }
}

impl Theme {
    /// Reads `ui.theme.preset`, `[ui.theme.colors]` and `[ui.colors]`;
    /// entries whose value is not a colour, and `[ui.theme.colors]` keys
    /// that are not a role, are ignored.
    pub fn from_config(ui: &UiConfig) -> Self {
        Self {
            preset: ui.theme.preset,
            roles: ui
                .theme
                .colors
                .iter()
                .filter_map(|(key, spec)| Some((Role::from_key(key)?, Color::parse(spec)?)))
                .collect(),
            statuses: ui
                .colors
                .iter()
                .filter_map(|(name, spec)| Color::parse(spec).map(|c| (name.clone(), c)))
                .collect(),
        }
    }

    /// The preset the theme is built on.
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// The colour of a role: `[ui.theme.colors]`, else the preset.
    pub fn color(&self, role: Role) -> Color {
        self.roles
            .get(&role)
            .copied()
            .unwrap_or_else(|| self.preset.color(role))
    }

    /// The colour of a status group (`None` is the no-status group):
    /// `[ui.colors]` by status name, else [`Theme::color`] of
    /// [`Role::of_status`].
    pub fn status_color(&self, status: Option<&Status>) -> Color {
        let name = status.map_or(NO_STATUS_KEY, Status::as_str);
        self.statuses
            .get(name)
            .copied()
            .unwrap_or_else(|| self.color(Role::of_status(status)))
    }

    /// The colour of the `DONE` group: `[ui.colors] done`, else
    /// [`Role::Done`].
    pub fn done_color(&self) -> Color {
        self.statuses
            .get(DONE_KEY)
            .copied()
            .unwrap_or_else(|| self.color(Role::Done))
    }
}

/// The label of the done group.
pub const DONE_LABEL: &str = "DONE";

/// The `IN PROGRESS` / `NO STATUS` header of a status group, as the
/// script printed it: upper case, hyphens as spaces.
pub fn group_label(status: Option<&Status>) -> String {
    status.map_or_else(
        || "NO STATUS".to_owned(),
        |s| s.as_str().to_uppercase().replace('-', " "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_names() {
        assert_eq!(Color::parse("Black"), Some(Color::Black));
        assert_eq!(Color::parse("Red"), Some(Color::Red));
        assert_eq!(Color::parse("green"), Some(Color::Green));
        assert_eq!(Color::parse("YELLOW"), Some(Color::Yellow));
        assert_eq!(Color::parse("blue"), Some(Color::Blue));
        assert_eq!(Color::parse("magenta"), Some(Color::Magenta));
        assert_eq!(Color::parse("cyan"), Some(Color::Cyan));
        assert_eq!(Color::parse("white"), Some(Color::White));
        assert_eq!(Color::parse("dim"), Some(Color::Dim));
        assert_eq!(Color::parse("gray"), Some(Color::Dim));
        assert_eq!(Color::parse("grey"), Some(Color::Dim));
        assert_eq!(Color::parse("reversed"), Some(Color::Reversed));
        assert_eq!(Color::parse("reverse"), Some(Color::Reversed));
        assert_eq!(Color::parse("none"), Some(Color::Plain));
        assert_eq!(Color::parse("plain"), Some(Color::Plain));
        assert_eq!(Color::parse("default"), Some(Color::Plain));
        assert_eq!(Color::parse(" 24 "), Some(Color::Fixed(24)));
        assert_eq!(Color::parse("0"), Some(Color::Fixed(0)));
        assert_eq!(Color::parse("255"), Some(Color::Fixed(255)));
        assert_eq!(Color::parse("256"), None);
        assert_eq!(Color::parse("-1"), None);
        assert_eq!(Color::parse("octarine"), None);
        assert_eq!(Color::parse(""), None);
    }

    #[test]
    fn role_keys_round_trip_and_are_distinct() {
        let keys: Vec<&str> = Role::ALL.iter().map(|r| r.key()).collect();
        assert_eq!(
            keys,
            vec![
                "in-progress",
                "ready",
                "waiting",
                "blocked",
                "later",
                "other-status",
                "no-status",
                "done",
                "chip-bg",
                "chip-fg",
                "prio-a",
                "focus",
                "selection",
                "error",
                "dim",
                "link",
                "header",
            ]
        );
        for role in Role::ALL {
            assert_eq!(Role::from_key(role.key()), Some(role));
        }
        assert_eq!(Role::from_key("chip"), None);
        assert_eq!(Role::from_key(""), None);
    }

    #[test]
    fn role_of_status() {
        assert_eq!(Role::of_status(None), Role::NoStatus);
        assert_eq!(
            Role::of_status(Some(&Status::IN_PROGRESS)),
            Role::InProgress
        );
        assert_eq!(Role::of_status(Some(&Status::READY)), Role::Ready);
        assert_eq!(Role::of_status(Some(&Status::WAITING)), Role::Waiting);
        assert_eq!(Role::of_status(Some(&Status::BLOCKED)), Role::Blocked);
        assert_eq!(Role::of_status(Some(&Status::LATER)), Role::Later);
        assert_eq!(
            Role::of_status(Some(&Status::new("review").unwrap())),
            Role::OtherStatus
        );
    }

    #[test]
    fn preset_names() {
        #[derive(Deserialize)]
        struct Doc {
            preset: Preset,
        }
        let names: Vec<&str> = Preset::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["dark", "light", "solarized", "gruvbox", "mono"]);
        assert_eq!(Preset::default(), Preset::Dark);
        for preset in Preset::ALL {
            // The serde name and `name()` agree.
            let doc = format!("preset = \"{}\"", preset.name());
            let parsed: Doc = toml::from_str(&doc).unwrap();
            assert_eq!(parsed.preset, preset);
        }
        assert!(toml::from_str::<Doc>("preset = \"nord\"").is_err());
    }

    /// Every preset's full table, in [`Role::ALL`] order.
    fn table(preset: Preset) -> Vec<Color> {
        Role::ALL.iter().map(|&r| preset.color(r)).collect()
    }

    #[test]
    fn dark_is_the_script() {
        use Color::{Blue, Cyan, Dim, Fixed, Green, Magenta, Plain, Red, Reversed, Yellow};
        assert_eq!(
            table(Preset::Dark),
            vec![
                Blue,
                Green,
                Yellow,
                Red,
                Magenta,
                Cyan,
                Dim,
                Dim,
                Fixed(24),
                Fixed(231),
                Red,
                Cyan,
                Reversed,
                Red,
                Dim,
                Fixed(75),
                Plain,
            ]
        );
    }

    #[test]
    fn light_has_no_faint_text() {
        use Color::{Fixed, Plain, Reversed};
        assert_eq!(
            table(Preset::Light),
            vec![
                Fixed(25),
                Fixed(28),
                Fixed(130),
                Fixed(124),
                Fixed(90),
                Fixed(30),
                Fixed(245),
                Fixed(245),
                Fixed(153),
                Fixed(17),
                Fixed(124),
                Fixed(25),
                Reversed,
                Fixed(124),
                Fixed(245),
                Fixed(25),
                Plain,
            ]
        );
        assert!(!table(Preset::Light).contains(&Color::Dim));
    }

    #[test]
    fn solarized_table() {
        use Color::{Fixed, Plain, Reversed};
        assert_eq!(
            table(Preset::Solarized),
            vec![
                Fixed(33),
                Fixed(64),
                Fixed(136),
                Fixed(160),
                Fixed(125),
                Fixed(37),
                Fixed(240),
                Fixed(240),
                Fixed(236),
                Fixed(245),
                Fixed(160),
                Fixed(37),
                Reversed,
                Fixed(160),
                Fixed(240),
                Fixed(33),
                Plain,
            ]
        );
    }

    #[test]
    fn gruvbox_table() {
        use Color::{Fixed, Plain, Reversed};
        assert_eq!(
            table(Preset::Gruvbox),
            vec![
                Fixed(109),
                Fixed(142),
                Fixed(214),
                Fixed(167),
                Fixed(175),
                Fixed(108),
                Fixed(245),
                Fixed(245),
                Fixed(237),
                Fixed(223),
                Fixed(167),
                Fixed(108),
                Reversed,
                Fixed(167),
                Fixed(245),
                Fixed(109),
                Plain,
            ]
        );
    }

    #[test]
    fn mono_has_attributes_only() {
        use Color::{Dim, Plain, Reversed};
        assert_eq!(
            table(Preset::Mono),
            vec![
                Plain, Plain, Plain, Plain, Plain, Plain, Dim, Dim, Reversed, Plain, Plain, Plain,
                Reversed, Plain, Dim, Plain, Plain,
            ]
        );
    }

    #[test]
    fn theme_defaults() {
        let theme = Theme::from_config(&UiConfig::default());
        assert_eq!(theme, Theme::default());
        assert_eq!(theme.preset(), Preset::Dark);
        assert_eq!(theme.status_color(Some(&Status::IN_PROGRESS)), Color::Blue);
        assert_eq!(theme.status_color(Some(&Status::READY)), Color::Green);
        assert_eq!(theme.status_color(Some(&Status::WAITING)), Color::Yellow);
        assert_eq!(theme.status_color(Some(&Status::BLOCKED)), Color::Red);
        assert_eq!(theme.status_color(Some(&Status::LATER)), Color::Magenta);
        assert_eq!(
            theme.status_color(Some(&Status::new("review").unwrap())),
            Color::Cyan
        );
        assert_eq!(theme.status_color(None), Color::Dim);
        assert_eq!(theme.done_color(), Color::Dim);
        for role in Role::ALL {
            assert_eq!(theme.color(role), Preset::Dark.color(role), "{role:?}");
        }
        assert_eq!(DONE_LABEL, "DONE");
    }

    #[test]
    fn theme_picks_the_preset() {
        let mut ui = UiConfig::default();
        ui.theme.preset = Preset::Gruvbox;
        let theme = Theme::from_config(&ui);
        assert_eq!(theme.preset(), Preset::Gruvbox);
        assert_eq!(theme.status_color(Some(&Status::READY)), Color::Fixed(142));
        assert_eq!(theme.status_color(None), Color::Fixed(245));
        assert_eq!(theme.done_color(), Color::Fixed(245));
        assert_eq!(theme.color(Role::ChipBg), Color::Fixed(237));
        assert_eq!(theme.color(Role::Link), Color::Fixed(109));
    }

    #[test]
    fn theme_colors_override_roles_over_the_preset() {
        let mut ui = UiConfig::default();
        ui.theme.preset = Preset::Light;
        ui.theme.colors.insert("ready".into(), "green".into());
        ui.theme.colors.insert("chip-bg".into(), "none".into());
        ui.theme.colors.insert("selection".into(), "236".into());
        ui.theme.colors.insert("dim".into(), "not-a-colour".into());
        ui.theme.colors.insert("not-a-role".into(), "red".into());
        let theme = Theme::from_config(&ui);
        assert_eq!(theme.color(Role::Ready), Color::Green);
        assert_eq!(theme.status_color(Some(&Status::READY)), Color::Green);
        assert_eq!(theme.color(Role::ChipBg), Color::Plain);
        assert_eq!(theme.color(Role::Selection), Color::Fixed(236));
        assert_eq!(theme.color(Role::Dim), Color::Fixed(245));
        assert_eq!(theme.color(Role::Blocked), Color::Fixed(124));
        assert_eq!(theme.roles.len(), 3, "{:?}", theme.roles);
    }

    #[test]
    fn ui_colors_override_by_status_name_on_top() {
        let mut ui = UiConfig::default();
        ui.theme.colors.insert("ready".into(), "green".into());
        ui.theme.colors.insert("no-status".into(), "blue".into());
        ui.colors.insert("ready".into(), "208".into());
        ui.colors.insert("no-status".into(), "white".into());
        ui.colors.insert("later".into(), "not-a-colour".into());
        ui.colors.insert("review".into(), "red".into());
        ui.colors.insert("done".into(), "green".into());
        let theme = Theme::from_config(&ui);
        assert_eq!(theme.done_color(), Color::Green);
        assert_eq!(theme.status_color(Some(&Status::READY)), Color::Fixed(208));
        assert_eq!(theme.color(Role::Ready), Color::Green, "roles keep theirs");
        assert_eq!(theme.status_color(None), Color::White);
        assert_eq!(theme.color(Role::NoStatus), Color::Blue);
        assert_eq!(theme.status_color(Some(&Status::LATER)), Color::Magenta);
        assert_eq!(
            theme.status_color(Some(&Status::new("review").unwrap())),
            Color::Red
        );
        assert_eq!(theme.status_color(Some(&Status::IN_PROGRESS)), Color::Blue);
        assert_eq!(theme.statuses.len(), 4, "{:?}", theme.statuses);
    }

    #[test]
    fn labels() {
        assert_eq!(group_label(Some(&Status::IN_PROGRESS)), "IN PROGRESS");
        assert_eq!(group_label(Some(&Status::READY)), "READY");
        assert_eq!(group_label(None), "NO STATUS");
    }
}
