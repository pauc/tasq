//! `tasq view <id>`: the task file, rendered with `glow` on a terminal.
//!
//! The three awk passes of the original script are reimplemented as pure
//! functions so they can be unit tested:
//!
//! - [`linkify_pre`] replaces every markdown link and bare URL with a
//!   marker (`U+E000 n U+E001 label U+E002`) before glow runs, so glow
//!   styles the label as text and the URL never reaches the screen.
//! - [`linkify_post`] turns the markers in glow's output into OSC 8
//!   hyperlinks (underlined, light blue) around the label.
//! - [`unwrap_urls`] is the fallback for terminals without OSC 8 support
//!   (`ui.no_osc8`): on lines that would wrap, links become bold labels and
//!   the URLs are listed below the line, each as `<url>` on its own line so
//!   the terminal can still match it.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use tasq_core::store::{Store, StoreError};
use tasq_core::theme::{Color, Role, Theme};
use tasq_store_nb::nb::find_in_path;

use crate::app::App;
use crate::error::Result;
use crate::json;
use crate::output::sgr;

/// Opens a link marker: `M0 <n> M1 <label> M2`.
pub const M0: char = '\u{E000}';
/// Separates the link number from its label.
pub const M1: char = '\u{E001}';
/// Closes a link marker.
pub const M2: char = '\u{E002}';

/// Width assumed when the terminal does not say.
pub const DEFAULT_WIDTH: usize = 100;

static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(?:[^\[\]]|\[[^\[\]]*\])*\]\(https?://[^)]*\)").unwrap());
static BARE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<?https?://[^ \t<>)]+>?").unwrap());
static MR_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/merge_requests/(\d+)").unwrap());
static ISSUE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(?:work_items|issues)/(\d+)").unwrap());
static REOPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("{M0}([0-9]+){M1}")).unwrap());
static BULLET_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*(?:[-*][ \t])?[ \t]*$").unwrap());
static BULLET_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t]*[-*]?[ \t]*$").unwrap());
static SPACE_BEFORE_PUNCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[ \t]+([.,;])").unwrap());

/// Runs `view`.
pub fn run(app: &App, id: &str, raw: bool) -> Result<()> {
    let id = App::task_id(id)?;
    let store = app.open_store()?;
    let path = store.path_of(&id)?;
    let text = std::fs::read_to_string(&path).map_err(|source| StoreError::Io {
        path: path.clone(),
        source,
    })?;
    if app.out.json_mode() {
        let task = store.get(&id)?;
        return app
            .out
            .json(&json::document([("task", json::to_value(&task))]));
    }
    if raw {
        return app.out.print(&text);
    }
    show_markdown(app, &text)
}

/// Shows `markdown` the way `view` does: rendered by `glow` with OSC 8
/// links when stdout is a terminal and glow is on the `PATH`, otherwise
/// as is; paged either way. `tasq summary` uses it for the distilled
/// summary. A glow failure is a warning and the plain markdown is shown.
pub fn show_markdown(app: &App, markdown: &str) -> Result<()> {
    let env = app.env_vec();
    let path_var = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map_or("", |(_, v)| v.as_str());
    let glow = if std::io::stdout().is_terminal() {
        find_in_path(path_var, "glow")
    } else {
        None
    };
    let Some(glow) = glow else {
        return app.out.page(markdown);
    };
    let width = terminal_width(&env, stdout_columns());
    let ui = &app.config().ui;
    let rendered = if ui.no_osc8 {
        render_with_glow(&glow, &unwrap_urls(markdown, width), &ui.glow_style, width)
    } else {
        let (marked, urls) = linkify_pre(markdown);
        let link = Theme::from_config(ui).color(Role::Link);
        render_with_glow(&glow, &marked, &ui.glow_style, width)
            .map(|r| linkify_post(&r, &urls, link))
    };
    match rendered {
        Ok(rendered) => app.out.page(&rendered),
        Err(e) => {
            app.out
                .warn(&format!("glow failed ({e}); showing the plain markdown"));
            app.out.page(markdown)
        }
    }
}

/// The width of the terminal stdout is on, when it is one. zsh does not
/// export `$COLUMNS` and `tput cols` answers 80 when its stdout is a pipe,
/// so this is the only reliable source.
///
/// Not unit-tested: an ioctl on the process's real stdout.
pub fn stdout_columns() -> Option<usize> {
    rustix::termios::tcgetwinsize(std::io::stdout())
        .ok()
        .map(|size| usize::from(size.ws_col))
}

/// `$COLUMNS`, else `columns` (from [`stdout_columns`]), else `tput cols`,
/// else [`DEFAULT_WIDTH`].
pub fn terminal_width(env: &[(String, String)], columns: Option<usize>) -> usize {
    if let Some(cols) = env
        .iter()
        .find(|(k, _)| k == "COLUMNS")
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .filter(|c| *c > 0)
        .or(columns.filter(|c| *c > 0))
    {
        return cols;
    }
    let mut command = Command::new("tput");
    command.arg("cols").env_clear();
    for (k, v) in env {
        command.env(k, v);
    }
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .filter(|c| *c > 0)
        .unwrap_or(DEFAULT_WIDTH)
}

/// Pipes `markdown` through `glow -s <style> -w <width>`.
pub fn render_with_glow(
    glow: &Path,
    markdown: &str,
    style: &str,
    width: usize,
) -> std::io::Result<String> {
    let mut child = Command::new(glow)
        .args(["-s", style, "-w", &width.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // glow can exit without reading (a bad flag): its exit status and
        // stderr below say why, a broken pipe here would hide it.
        match stdin.write_all(markdown.as_bytes()) {
            Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => return Err(e),
            _ => {}
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The label a URL gets when it has none: `!123` for a merge request,
/// `#123` for an issue or work item, otherwise the URL without scheme and
/// `www.`, shortened in the middle past 60 characters.
pub fn shortref(url: &str) -> String {
    if let Some(c) = MR_REF.captures(url) {
        return format!("!{}", &c[1]);
    }
    if let Some(c) = ISSUE_REF.captures(url) {
        return format!("#{}", &c[1]);
    }
    let u = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let u = u.strip_prefix("www.").unwrap_or(u);
    let chars: Vec<char> = u.chars().collect();
    if chars.len() > 60 {
        let head: String = chars[..45].iter().collect();
        let tail: String = chars[chars.len() - 10..].iter().collect();
        format!("{head}...{tail}")
    } else {
        u.to_owned()
    }
}

/// A markdown link's `(label, url)`.
fn split_md_link(seg: &str) -> (&str, &str) {
    let inner = &seg[1..seg.len() - 1];
    let (label, url) = inner.rsplit_once("](").expect("matched the link regex");
    (label, url)
}

/// A bare URL match without `<>` and with trailing punctuation separated:
/// `(url, punctuation)`.
fn split_bare_url(seg: &str) -> (&str, &str) {
    let url = seg.strip_prefix('<').unwrap_or(seg);
    let url = url.strip_suffix('>').unwrap_or(url);
    let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    (trimmed, &url[trimmed.len()..])
}

/// Replaces links with markers, returning the marked text and the URLs in
/// marker order (marker `n` is `urls[n - 1]`). Fenced code is untouched.
pub fn linkify_pre(markdown: &str) -> (String, Vec<String>) {
    let mut urls = Vec::new();
    let mut out = String::with_capacity(markdown.len());
    let mut fence = false;
    for line in markdown.lines() {
        if line.starts_with("```") {
            fence = !fence;
        } else if !fence {
            out.push_str(&mark_line(line, &mut urls));
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, urls)
}

fn mark_line(line: &str, urls: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pos = 0;
    while pos < line.len() {
        let md = MD_LINK.find_at(line, pos);
        let bare = BARE_URL.find_at(line, pos);
        let Some((m, is_md)) = (match (md, bare) {
            (Some(a), Some(b)) if b.start() < a.start() => Some((b, false)),
            (Some(a), _) => Some((a, true)),
            (None, Some(b)) => Some((b, false)),
            (None, None) => None,
        }) else {
            break;
        };
        out.push_str(&line[pos..m.start()]);
        if is_md {
            let (label, url) = split_md_link(m.as_str());
            let label = if label.is_empty() {
                shortref(url)
            } else {
                label.to_owned()
            };
            out.push_str(&marker(urls, url, &label));
        } else {
            let (url, punct) = split_bare_url(m.as_str());
            out.push_str(&marker(urls, url, &shortref(url)));
            out.push_str(punct);
        }
        pos = m.end();
    }
    out.push_str(&line[pos..]);
    out
}

fn marker(urls: &mut Vec<String>, url: &str, label: &str) -> String {
    urls.push(url.to_owned());
    format!("{M0}{}{M1}{label}{M2}", urls.len())
}

/// Turns the markers in glow's output into OSC 8 hyperlinks, underlined
/// in the theme's `link` colour. The closing code undoes exactly what the
/// opening one set, so glow's own styling around the link survives.
pub fn linkify_post(rendered: &str, urls: &[String], link: Color) -> String {
    let (open, close) = link_codes(link);
    let opened = REOPEN.replace_all(rendered, |c: &Captures<'_>| {
        let url = c[1]
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| urls.get(i))
            .map_or("", String::as_str);
        format!("\x1b]8;;{url}\x07\x1b[{open}m")
    });
    opened.replace(M2, &format!("\x1b[{close}m\x1b]8;;\x07"))
}

/// The SGR parameters that open and close a link in `color`: underline
/// plus the colour, and the resets of exactly those.
fn link_codes(color: Color) -> (String, String) {
    let reset = match color {
        Color::Dim => "22",
        Color::Reversed => "27",
        Color::Plain => return ("4".to_owned(), "24".to_owned()),
        _ => "39",
    };
    let open = sgr(color).map_or_else(|| "4".to_owned(), |code| format!("{code};4"));
    (open, format!("24;{reset}"))
}

/// The no-OSC-8 fallback: on lines longer than `width - 6` (outside fenced
/// code), markdown links become `**label**` and bare URLs are removed, and
/// the URLs are listed under the line as `<url>` items. A line that is only
/// a (possibly bulleted) URL is kept as `<url>`.
pub fn unwrap_urls(markdown: &str, width: usize) -> String {
    let limit = width.saturating_sub(6);
    let mut out = String::with_capacity(markdown.len());
    let mut fence = false;
    for line in markdown.lines() {
        if line.starts_with("```") {
            fence = !fence;
        }
        if fence || line.starts_with("```") || line.chars().count() <= limit {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        out.push_str(&unwrap_line(line));
    }
    out
}

fn unwrap_line(line: &str) -> String {
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut urls: Vec<String> = Vec::new();
    let bolded = MD_LINK.replace_all(line, |c: &Captures<'_>| {
        let (label, url) = split_md_link(&c[0]);
        urls.push(url.to_owned());
        format!("**{label}**")
    });
    let mut rest = bolded.into_owned();
    let mut out = String::new();
    while let Some(m) = BARE_URL.find(&rest) {
        let pre = rest[..m.start()].to_owned();
        let (url, punct) = split_bare_url(m.as_str());
        let after = format!("{punct}{}", &rest[m.end()..]);
        if urls.is_empty()
            && out.is_empty()
            && BULLET_PREFIX.is_match(&pre)
            && after.trim().is_empty()
        {
            return format!("{pre}<{url}>\n");
        }
        urls.push(url.to_owned());
        out.push_str(&pre);
        rest = after;
    }
    out.push_str(&rest);
    let tidy = SPACE_BEFORE_PUNCT.replace_all(&out, "$1");
    let tidy = tidy.trim_end();
    let mut result = String::new();
    if !BULLET_ONLY.is_match(tidy) {
        result.push_str(tidy);
        result.push('\n');
    }
    for url in urls {
        let _ = writeln!(result, "{indent}  - <{url}>");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const GL: &str = "https://gitlab.example.com/group/project/-/merge_requests/123";
    const ISSUE: &str = "https://gitlab.example.com/group/project/-/issues/42";

    #[test]
    fn shortrefs() {
        assert_eq!(shortref(GL), "!123");
        assert_eq!(shortref(&format!("{GL}/diffs")), "!123");
        assert_eq!(shortref(ISSUE), "#42");
        assert_eq!(
            shortref("https://gitlab.example.com/groups/g/-/work_items/7"),
            "#7"
        );
        assert_eq!(shortref("https://www.example.com/x"), "example.com/x");
        assert_eq!(shortref("http://example.com"), "example.com");
        let long = format!("https://example.com/{}", "a".repeat(70));
        let short = shortref(&long);
        assert_eq!(short.chars().count(), 45 + 3 + 10);
        assert!(short.starts_with("example.com/aaaa"));
        assert!(short.ends_with("...aaaaaaaaaa"));
    }

    #[test]
    fn linkify_pre_marks_markdown_links_and_bare_urls() {
        let md = format!(
            "# [ ] T\n\n- [Add parser]({GL})\n- {ISSUE}.\n- see <https://example.com/doc>, ok\n- []({GL})\n"
        );
        let (marked, urls) = linkify_pre(&md);
        assert_eq!(
            marked,
            format!(
                "# [ ] T\n\n- {M0}1{M1}Add parser{M2}\n- {M0}2{M1}#42{M2}.\n- see {M0}3{M1}example.com/doc{M2}, ok\n- {M0}4{M1}!123{M2}\n"
            )
        );
        assert_eq!(
            urls,
            vec![
                GL.to_owned(),
                ISSUE.to_owned(),
                "https://example.com/doc".to_owned(),
                GL.to_owned()
            ]
        );
    }

    #[test]
    fn linkify_pre_handles_nested_brackets_and_labels_with_urls() {
        let md = format!("[see [x] here]({GL}) and [{ISSUE}]({ISSUE})\n");
        let (marked, urls) = linkify_pre(&md);
        assert_eq!(
            marked,
            format!("{M0}1{M1}see [x] here{M2} and {M0}2{M1}{ISSUE}{M2}\n")
        );
        assert_eq!(urls, vec![GL.to_owned(), ISSUE.to_owned()]);
    }

    #[test]
    fn fenced_code_is_untouched_and_trailing_punctuation_stays_outside() {
        let md = "```\nhttps://example.com/in/code.\n```\nhttps://example.com/out!?\n";
        let (marked, urls) = linkify_pre(md);
        assert_eq!(
            marked,
            format!("```\nhttps://example.com/in/code.\n```\n{M0}1{M1}example.com/out{M2}!?\n")
        );
        assert_eq!(urls, vec!["https://example.com/out".to_owned()]);
    }

    #[test]
    fn linkify_post_emits_osc8() {
        let rendered = format!("  • {M0}1{M1}Add parser{M2} done\n");
        let urls = vec![GL.to_owned()];
        assert_eq!(
            linkify_post(&rendered, &urls, Color::Fixed(75)),
            format!("  • \x1b]8;;{GL}\x07\x1b[38;5;75;4mAdd parser\x1b[24;39m\x1b]8;;\x07 done\n")
        );
        // Unknown numbers degrade to an empty target rather than panicking.
        assert_eq!(
            linkify_post(&format!("{M0}9{M1}x{M2}"), &urls, Color::Fixed(75)),
            "\x1b]8;;\x07\x1b[38;5;75;4mx\x1b[24;39m\x1b]8;;\x07"
        );
        // The theme's link colour, and the closing code that undoes it.
        assert_eq!(
            linkify_post(&format!("{M0}1{M1}x{M2}"), &urls, Color::Blue),
            format!("\x1b]8;;{GL}\x07\x1b[34;4mx\x1b[24;39m\x1b]8;;\x07")
        );
        assert_eq!(
            linkify_post(&format!("{M0}1{M1}x{M2}"), &urls, Color::Dim),
            format!("\x1b]8;;{GL}\x07\x1b[2;4mx\x1b[24;22m\x1b]8;;\x07")
        );
        assert_eq!(
            linkify_post(&format!("{M0}1{M1}x{M2}"), &urls, Color::Reversed),
            format!("\x1b]8;;{GL}\x07\x1b[7;4mx\x1b[24;27m\x1b]8;;\x07")
        );
        assert_eq!(
            linkify_post(&format!("{M0}1{M1}x{M2}"), &urls, Color::Plain),
            format!("\x1b]8;;{GL}\x07\x1b[4mx\x1b[24m\x1b]8;;\x07")
        );
    }

    #[test]
    fn unwrap_urls_leaves_short_lines_and_code_alone() {
        let md = format!("- [a]({GL})\n```\n{}\n```\n", "x".repeat(200));
        assert_eq!(unwrap_urls(&md, 100), md);
    }

    #[test]
    fn unwrap_urls_lists_links_below_long_lines() {
        let long_label = "L".repeat(80);
        let md = format!("  - Review [{long_label}]({GL}) and {ISSUE}, then {ISSUE}.\n");
        assert_eq!(
            unwrap_urls(&md, 80),
            format!(
                "  - Review **{long_label}** and, then.\n    - <{GL}>\n    - <{ISSUE}>\n    - <{ISSUE}>\n"
            )
        );
    }

    #[test]
    fn unwrap_urls_keeps_a_lone_url_line_as_an_autolink() {
        let url = format!("https://example.com/{}", "p".repeat(90));
        // A trailing period is not part of the URL; like the awk original,
        // the leftover "." then stays as its own line above the URL item.
        let md = format!(
            "- {url}
{url}
{url}.
"
        );
        assert_eq!(
            unwrap_urls(&md, 80),
            format!(
                "- <{url}>
<{url}>
.
  - <{url}>
"
            )
        );
        // An empty label is bolded as is (`****`), exactly as the script did.
        let md = format!(
            "- []({url})
"
        );
        assert_eq!(
            unwrap_urls(&md, 80),
            format!(
                "- ****
  - <{url}>
"
            )
        );
    }

    #[test]
    fn terminal_width_reads_columns() {
        let env = vec![("COLUMNS".to_owned(), "132".to_owned())];
        assert_eq!(terminal_width(&env, Some(90)), 132);
        let env = vec![
            ("COLUMNS".to_owned(), "0".to_owned()),
            ("PATH".to_owned(), "/nonexistent".to_owned()),
        ];
        assert_eq!(terminal_width(&env, Some(90)), 90);
        assert_eq!(terminal_width(&env, Some(0)), DEFAULT_WIDTH);
        assert_eq!(terminal_width(&env, None), DEFAULT_WIDTH);
    }

    #[test]
    fn render_with_glow_runs_the_program_and_reports_failures() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let glow = dir.path().join("glow");
        std::fs::write(
            &glow,
            "#!/bin/sh\n[ \"$1\" = -s ] || exit 9\necho \"style=$2 width=$4\"\ncat\n",
        )
        .unwrap();
        std::fs::set_permissions(&glow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = render_with_glow(&glow, "# hi\n", "light", 88).unwrap();
        assert_eq!(out, "style=light width=88\n# hi\n");
        let failing = dir.path().join("glow-fail");
        std::fs::write(&failing, "#!/bin/sh\necho 'bad flag' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = render_with_glow(&failing, "x", "dark", 80).unwrap_err();
        assert_eq!(err.to_string(), "bad flag");
        // More than a pipe buffer: the write always meets the closed pipe,
        // and the error is still glow's, not "Broken pipe".
        let big = "x".repeat(1 << 20);
        let err = render_with_glow(&failing, &big, "dark", 80).unwrap_err();
        assert_eq!(err.to_string(), "bad flag");
        assert!(render_with_glow(Path::new("/nonexistent/glow"), "x", "dark", 80).is_err());
    }
}
