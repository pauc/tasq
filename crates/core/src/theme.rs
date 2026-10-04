//! Colour semantics shared by the CLI and the TUI (plan section 8: "the
//! renderer is a separate module so the TUI and CLI share color semantics").
//!
//! A [`Color`] is a terminal colour name or a 256-colour palette index, as
//! written under `[ui.colors]`; a [`Theme`] maps status groups to colours:
//! the five defaults of the original script, cyan for any other configured
//! status, dim for the no-status group, each overridable by status name.
//! How a colour becomes escape codes or widget styles is the front ends'
//! business; this module holds only the decisions.

use std::collections::BTreeMap;

use crate::config::UiConfig;
use crate::model::Status;

/// A colour a status group (or a check verdict) can be rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
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
    /// A 256-colour palette index.
    Fixed(u8),
}

impl Color {
    /// Parses a colour name (`red`, `dim`, `grey`/`gray`, ...) or a palette
    /// index (`0`-`255`), as written under `[ui.colors]`. Case and
    /// surrounding whitespace do not matter.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "red" => Self::Red,
            "green" => Self::Green,
            "yellow" => Self::Yellow,
            "blue" => Self::Blue,
            "magenta" => Self::Magenta,
            "cyan" => Self::Cyan,
            "white" => Self::White,
            "dim" | "grey" | "gray" => Self::Dim,
            number => Self::Fixed(number.parse().ok()?),
        })
    }
}

/// The `[ui.colors]` key of the no-status group.
pub const NO_STATUS_KEY: &str = "no-status";
/// The `[ui.colors]` key of the done group (`tasq list --all` / `--done`).
pub const DONE_KEY: &str = "done";

/// Colours of the status groups: the script's five plus `cyan` for any
/// other configured status and dim for `NO STATUS`, overridable per status
/// name under `[ui.colors]` ([`NO_STATUS_KEY`] for the last group).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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

    /// The colour of a status group (`None` is the no-status group).
    pub fn status_color(&self, status: Option<&Status>) -> Color {
        let name = status.map_or(NO_STATUS_KEY, Status::as_str);
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

impl Theme {
    /// The colour of the `DONE` group: `[ui.colors] done`, else dim.
    pub fn done_color(&self) -> Color {
        self.overrides.get(DONE_KEY).copied().unwrap_or(Color::Dim)
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
        assert_eq!(Color::parse(" 24 "), Some(Color::Fixed(24)));
        assert_eq!(Color::parse("0"), Some(Color::Fixed(0)));
        assert_eq!(Color::parse("255"), Some(Color::Fixed(255)));
        assert_eq!(Color::parse("256"), None);
        assert_eq!(Color::parse("-1"), None);
        assert_eq!(Color::parse("octarine"), None);
        assert_eq!(Color::parse(""), None);
    }

    #[test]
    fn theme_defaults() {
        let theme = Theme::from_config(&UiConfig::default());
        assert_eq!(theme, Theme::default());
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
        assert_eq!(DONE_LABEL, "DONE");
    }

    #[test]
    fn theme_overrides_by_status_name() {
        let mut ui = UiConfig::default();
        ui.colors.insert("ready".into(), "208".into());
        ui.colors.insert("no-status".into(), "white".into());
        ui.colors.insert("later".into(), "not-a-colour".into());
        ui.colors.insert("review".into(), "red".into());
        ui.colors.insert("done".into(), "green".into());
        let theme = Theme::from_config(&ui);
        assert_eq!(theme.done_color(), Color::Green);
        assert_eq!(theme.status_color(Some(&Status::READY)), Color::Fixed(208));
        assert_eq!(theme.status_color(None), Color::White);
        assert_eq!(theme.status_color(Some(&Status::LATER)), Color::Magenta);
        assert_eq!(
            theme.status_color(Some(&Status::new("review").unwrap())),
            Color::Red
        );
        assert_eq!(theme.status_color(Some(&Status::IN_PROGRESS)), Color::Blue);
        assert_eq!(theme.overrides.len(), 4, "{:?}", theme.overrides);
    }

    #[test]
    fn labels() {
        assert_eq!(group_label(Some(&Status::IN_PROGRESS)), "IN PROGRESS");
        assert_eq!(group_label(Some(&Status::READY)), "READY");
        assert_eq!(group_label(None), "NO STATUS");
    }
}
