//! Human output: colour decision, ANSI styling, pager and JSON emission.
//!
//! Colour follows the usual rules: on when stdout is a terminal and
//! `NO_COLOR` is unset, forced on or off with `--color`. The pager
//! (`ui.pager`, default `less -RFX`) is only used on a terminal and only by
//! commands with potentially long output; `--no-pager` disables it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};

use serde::Serialize;
use tasq_core::config::UiConfig;
pub use tasq_core::theme::Color;

use crate::cli::{ColorChoice, GlobalArgs};
use crate::error::{CliError, Result};

/// Where and how a command writes.
#[derive(Debug, Clone)]
pub struct Output {
    color: bool,
    pager: Option<Vec<String>>,
    json: bool,
    verbose: u8,
}

impl Output {
    /// Decides colour and pager from the flags, the `[ui]` config, the
    /// environment and whether stdout is a terminal.
    pub fn new(
        global: &GlobalArgs,
        ui: &UiConfig,
        env: &BTreeMap<String, String>,
        stdout_is_tty: bool,
    ) -> Self {
        let choice = if global.no_color {
            ColorChoice::Never
        } else {
            global.color
        };
        Self {
            color: use_color(
                choice,
                env.get("NO_COLOR").map(String::as_str),
                stdout_is_tty,
            ),
            pager: pager_command(global.no_pager, stdout_is_tty, &ui.pager),
            json: global.json,
            verbose: global.verbose,
        }
    }

    /// Same decisions, taking the terminal state from the real stdout.
    pub fn from_process(
        global: &GlobalArgs,
        ui: &UiConfig,
        env: &BTreeMap<String, String>,
    ) -> Self {
        Self::new(global, ui, env, io::stdout().is_terminal())
    }

    /// Whether `--json` was given.
    pub fn json_mode(&self) -> bool {
        self.json
    }

    /// Whether colour escapes are emitted.
    pub fn color(&self) -> bool {
        self.color
    }

    /// Verbosity level (`-v` count).
    pub fn verbosity(&self) -> u8 {
        self.verbose
    }

    /// The styling helper matching the colour decision.
    pub fn style(&self) -> Style {
        Style {
            enabled: self.color,
        }
    }

    /// Writes `text` to stdout as is.
    pub fn print(&self, text: &str) -> Result<()> {
        let mut stdout = io::stdout().lock();
        write_all(&mut stdout, text.as_bytes())
    }

    /// Writes `text` through the pager when one is in use (a terminal and
    /// no `--no-pager`), otherwise to stdout. A pager that cannot be
    /// started is reported as a warning and the text is printed directly.
    pub fn page(&self, text: &str) -> Result<()> {
        let Some(argv) = &self.pager else {
            return self.print(text);
        };
        match spawn_pager(argv, text) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.warn(&format!(
                    "pager {:?} failed ({e}); printing directly",
                    argv.join(" ")
                ));
                self.print(text)
            }
        }
    }

    /// Serialises `value` as pretty JSON followed by a newline.
    pub fn json<T: Serialize>(&self, value: &T) -> Result<()> {
        let text = serde_json::to_string_pretty(value)
            .map_err(|e| CliError::Internal(anyhow::anyhow!("serialising JSON: {e}")))?;
        self.print(&format!("{text}\n"))
    }

    /// `tasq: warning: <message>` on stderr.
    pub fn warn(&self, message: &str) {
        eprintln!("tasq: warning: {message}");
    }

    /// `tasq: <message>` on stderr, only at `-v` or more.
    pub fn verbose(&self, message: &str) {
        if self.verbose > 0 {
            eprintln!("tasq: {message}");
        }
    }
}

/// Writes, treating a closed stdout (`head`, a quit pager) as success.
pub fn write_all(stdout: &mut impl Write, bytes: &[u8]) -> Result<()> {
    match stdout.write_all(bytes).and_then(|()| stdout.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn spawn_pager(argv: &[String], text: &str) -> io::Result<()> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| io::Error::other("empty pager command"))?;
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // The user quitting the pager early closes the pipe; that is fine.
        match stdin.write_all(text.as_bytes()) {
            Ok(()) | Err(_) => {}
        }
    }
    child.wait()?;
    Ok(())
}

/// The colour decision: `Always`/`Never` win; `Auto` means a terminal with
/// `NO_COLOR` unset or empty (<https://no-color.org>).
pub fn use_color(choice: ColorChoice, no_color_env: Option<&str>, stdout_is_tty: bool) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => stdout_is_tty && no_color_env.is_none_or(str::is_empty),
    }
}

/// The pager argv, or `None` when output goes straight to stdout: not a
/// terminal, `--no-pager`, or an empty / `cat` pager setting.
pub fn pager_command(no_pager: bool, stdout_is_tty: bool, pager: &str) -> Option<Vec<String>> {
    if no_pager || !stdout_is_tty {
        return None;
    }
    let argv = shell_words::split(pager).ok()?;
    match argv.first().map(String::as_str) {
        None | Some("cat") => None,
        Some(_) => Some(argv),
    }
}

/// ANSI styling that renders as plain text when disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Whether escapes are emitted.
    pub enabled: bool,
}

impl Style {
    /// Always-on styling.
    pub const ON: Self = Self { enabled: true };
    /// Styling that leaves text untouched.
    pub const OFF: Self = Self { enabled: false };

    fn wrap(self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    /// Bold.
    pub fn bold(self, text: &str) -> String {
        self.wrap("1", text)
    }

    /// Dim (faint).
    pub fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }

    /// Bold in `color`.
    pub fn bold_color(self, color: Color, text: &str) -> String {
        match sgr(color) {
            Some(code) => self.wrap(&format!("1;{code}"), text),
            None => self.wrap("1", text),
        }
    }

    /// Text in `color` (`none` leaves it as it is).
    pub fn color(self, color: Color, text: &str) -> String {
        match sgr(color) {
            Some(code) => self.wrap(&code, text),
            None => text.to_owned(),
        }
    }

    /// A tag chip: `text` in `fg` on `bg` (the original script's white on
    /// dark blue), with a space of padding on each side.
    pub fn chip(self, bg: Color, fg: Color, text: &str) -> String {
        let mut codes = String::new();
        if self.enabled {
            for code in [sgr_bg(bg), sgr(fg)].into_iter().flatten() {
                let _ = write!(codes, "\x1b[{code}m");
            }
        }
        if codes.is_empty() {
            format!(" {text} ")
        } else {
            format!("{codes} {text} \x1b[0m")
        }
    }
}

/// The SGR parameter(s) selecting `color` as the foreground; `none` has
/// none.
pub fn sgr(color: Color) -> Option<String> {
    Some(match color {
        Color::Black => "30".to_owned(),
        Color::Red => "31".to_owned(),
        Color::Green => "32".to_owned(),
        Color::Yellow => "33".to_owned(),
        Color::Blue => "34".to_owned(),
        Color::Magenta => "35".to_owned(),
        Color::Cyan => "36".to_owned(),
        Color::White => "37".to_owned(),
        Color::Dim => "2".to_owned(),
        Color::Reversed => "7".to_owned(),
        Color::Plain => return None,
        Color::Fixed(n) => format!("38;5;{n}"),
    })
}

/// The SGR parameter(s) selecting `color` as the background; the
/// attributes (`dim`, `reversed`) apply as they are.
pub fn sgr_bg(color: Color) -> Option<String> {
    Some(match color {
        Color::Black => "40".to_owned(),
        Color::Red => "41".to_owned(),
        Color::Green => "42".to_owned(),
        Color::Yellow => "43".to_owned(),
        Color::Blue => "44".to_owned(),
        Color::Magenta => "45".to_owned(),
        Color::Cyan => "46".to_owned(),
        Color::White => "47".to_owned(),
        Color::Fixed(n) => format!("48;5;{n}"),
        Color::Dim | Color::Reversed | Color::Plain => return sgr(color),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_decision() {
        assert!(use_color(ColorChoice::Auto, None, true));
        assert!(use_color(ColorChoice::Auto, Some(""), true));
        assert!(!use_color(ColorChoice::Auto, Some("1"), true));
        assert!(!use_color(ColorChoice::Auto, None, false));
        assert!(use_color(ColorChoice::Always, Some("1"), false));
        assert!(!use_color(ColorChoice::Never, None, true));
    }

    #[test]
    fn no_color_flag_wins_over_color_auto() {
        let global = GlobalArgs {
            no_color: true,
            ..GlobalArgs::default()
        };
        let out = Output::new(&global, &UiConfig::default(), &BTreeMap::new(), true);
        assert!(!out.color());
        let global = GlobalArgs {
            color: ColorChoice::Always,
            ..GlobalArgs::default()
        };
        let out = Output::new(&global, &UiConfig::default(), &BTreeMap::new(), false);
        assert!(out.color());
        assert_eq!(out.style(), Style::ON);
    }

    #[test]
    fn pager_only_on_a_terminal() {
        assert_eq!(
            pager_command(false, true, "less -RFX"),
            Some(vec!["less".to_owned(), "-RFX".to_owned()])
        );
        assert_eq!(pager_command(true, true, "less -RFX"), None);
        assert_eq!(pager_command(false, false, "less -RFX"), None);
        assert_eq!(pager_command(false, true, ""), None);
        assert_eq!(pager_command(false, true, "cat"), None);
        assert_eq!(
            pager_command(false, true, "bat -p --paging=always"),
            Some(vec![
                "bat".to_owned(),
                "-p".to_owned(),
                "--paging=always".to_owned()
            ])
        );
        assert_eq!(pager_command(false, true, "unbalanced 'quote"), None);
    }

    #[test]
    fn styles() {
        assert_eq!(Style::ON.bold("x"), "\x1b[1mx\x1b[0m");
        assert_eq!(Style::ON.dim("x"), "\x1b[2mx\x1b[0m");
        assert_eq!(Style::ON.color(Color::Blue, "x"), "\x1b[34mx\x1b[0m");
        assert_eq!(Style::ON.bold_color(Color::Red, "x"), "\x1b[1;31mx\x1b[0m");
        assert_eq!(Style::ON.bold_color(Color::Dim, "x"), "\x1b[1;2mx\x1b[0m");
        assert_eq!(Style::ON.bold_color(Color::Plain, "x"), "\x1b[1mx\x1b[0m");
        assert_eq!(Style::ON.color(Color::Black, "x"), "\x1b[30mx\x1b[0m");
        assert_eq!(Style::ON.color(Color::Reversed, "x"), "\x1b[7mx\x1b[0m");
        assert_eq!(Style::ON.color(Color::Plain, "x"), "x");
        assert_eq!(
            Style::ON.color(Color::Fixed(208), "x"),
            "\x1b[38;5;208mx\x1b[0m"
        );
        assert_eq!(
            Style::ON.chip(Color::Fixed(24), Color::Fixed(231), "#gitlab"),
            "\x1b[48;5;24m\x1b[38;5;231m #gitlab \x1b[0m"
        );
        assert_eq!(
            Style::ON.chip(Color::Blue, Color::Plain, "#gitlab"),
            "\x1b[44m #gitlab \x1b[0m"
        );
        assert_eq!(
            Style::ON.chip(Color::Reversed, Color::Plain, "#gitlab"),
            "\x1b[7m #gitlab \x1b[0m"
        );
        assert_eq!(
            Style::ON.chip(Color::Plain, Color::Plain, "#gitlab"),
            " #gitlab "
        );
        assert_eq!(Style::OFF.bold("x"), "x");
        assert_eq!(
            Style::OFF.chip(Color::Fixed(24), Color::Fixed(231), "#gitlab"),
            " #gitlab "
        );
        assert_eq!(Style::OFF.bold_color(Color::Red, "x"), "x");
        for color in [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::White,
        ] {
            let fg: u8 = sgr(color).unwrap().parse().unwrap();
            let bg: u8 = sgr_bg(color).unwrap().parse().unwrap();
            assert_eq!(bg, fg + 10, "{color:?}");
        }
        assert_eq!(sgr_bg(Color::Dim).as_deref(), Some("2"));
        assert_eq!(sgr_bg(Color::Reversed).as_deref(), Some("7"));
        assert_eq!(sgr_bg(Color::Plain), None);
    }
}
