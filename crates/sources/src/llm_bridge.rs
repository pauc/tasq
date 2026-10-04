//! The LLM bridge (plan T-505): a configured command (typically
//! `claude -p --output-format json` reading a prompt file) prints a JSON
//! array of items; this source parses and deduplicates it. The contract is
//! in `docs/sources.md`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;
use tasq_core::model::Origin;
use tasq_core::source::{ItemState, Source, SourceError, SourceItem, SourceItemState, SyncContext};

use crate::http::excerpt;

/// Environment variable holding the known external ids as a JSON array, so
/// the prompt can avoid re-reporting items the notebook already tracks.
pub const ENV_KNOWN: &str = "TASQ_SYNC_KNOWN";

/// A command that prints items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmBridge {
    /// Source name.
    pub name: String,
    /// `[[source]] command`, split like a shell command line.
    pub command: String,
    /// `[[source]] prompt_file`, piped to the command's stdin when set.
    pub prompt_file: Option<PathBuf>,
    /// Environment the command runs with.
    pub env: Vec<(String, String)>,
}

/// A finished command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit status was zero.
    pub success: bool,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Runs `argv` with `stdin` as its input and `env` (plus `extra`) as its
/// environment.
///
/// Reason: process I/O; parsing and deduplication are tested on their own.
#[mutants::skip]
pub fn run_command(
    argv: &[String],
    stdin: &str,
    env: &[(String, String)],
    extra: &[(String, String)],
) -> Result<CommandOutput, String> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| "empty command".to_owned())?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env.iter().chain(extra) {
        command.env(k, v);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if let Some(mut pipe) = child.stdin.take() {
        // The command may exit without reading; that is not our error.
        let _ = pipe.write_all(stdin.as_bytes());
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Drops a surrounding Markdown code fence (` ```json ... ``` `).
pub fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let body = rest.split_once('\n').map_or("", |(_, body)| body);
    body.trim_end().strip_suffix("```").unwrap_or(body).trim()
}

/// Items from the command's output: a JSON array of items; or an object
/// with `"items"` (an array) or with `"result"` (a string holding the
/// array, as `claude --output-format json` prints).
pub fn parse_items(text: &str) -> Result<Vec<SourceItem>, SourceError> {
    let invalid = |message: String| SourceError::InvalidOutput {
        message,
        excerpt: excerpt(text),
    };
    let cleaned = strip_fences(text);
    let value: Value =
        serde_json::from_str(cleaned).map_err(|e| invalid(format!("not JSON: {e}")))?;
    let array = match value {
        Value::Array(items) => items,
        Value::Object(mut map) => {
            if let Some(Value::Array(items)) = map.remove("items") {
                items
            } else if let Some(Value::String(inner)) = map.remove("result") {
                return parse_items(&inner);
            } else {
                return Err(invalid(
                    "expected a JSON array of items (or an object with \"items\" or \"result\")"
                        .to_owned(),
                ));
            }
        }
        _ => return Err(invalid("expected a JSON array of items".to_owned())),
    };
    array
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            serde_json::from_value::<SourceItem>(v).map_err(|e| invalid(format!("item {i}: {e}")))
        })
        .collect()
}

/// A title as a deduplication key: lowercase, whitespace collapsed.
pub fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Drops repeats: same external id, else same URL, else same normalised
/// title. First occurrence wins.
pub fn dedupe(items: Vec<SourceItem>) -> Vec<SourceItem> {
    let mut ids: Vec<String> = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for item in items {
        let title = normalize_title(&item.title);
        if ids.contains(&item.external_id)
            || item.url.as_ref().is_some_and(|u| urls.contains(u))
            || titles.contains(&title)
        {
            continue;
        }
        ids.push(item.external_id.clone());
        if let Some(url) = &item.url {
            urls.push(url.clone());
        }
        titles.push(title);
        out.push(item);
    }
    out
}

impl Source for LlmBridge {
    fn name(&self) -> &str {
        &self.name
    }

    fn fetch(&self, ctx: &SyncContext) -> Result<Vec<SourceItem>, SourceError> {
        let argv = shell_words::split(&self.command)
            .map_err(|e| SourceError::Unavailable(format!("command {:?}: {e}", self.command)))?;
        let prompt = match &self.prompt_file {
            Some(file) => std::fs::read_to_string(file).map_err(|e| {
                SourceError::Unavailable(format!("prompt_file {}: {e}", file.display()))
            })?,
            None => String::new(),
        };
        let known: Vec<&str> = ctx.known.iter().map(|o| o.external_id.as_str()).collect();
        let extra = vec![(
            ENV_KNOWN.to_owned(),
            serde_json::to_string(&known).unwrap_or_else(|_| "[]".to_owned()),
        )];
        let output =
            run_command(&argv, &prompt, &self.env, &extra).map_err(SourceError::Unavailable)?;
        if !output.success {
            return Err(SourceError::Unavailable(format!(
                "command {:?} failed: {}",
                self.command,
                excerpt(if output.stderr.trim().is_empty() {
                    &output.stdout
                } else {
                    &output.stderr
                })
            )));
        }
        Ok(dedupe(parse_items(&output.stdout)?))
    }

    /// The bridge cannot look items up again, so every origin is reported
    /// as still open (a configured `flag` still applies); nothing is ever
    /// closed by it.
    fn check(&self, origins: &[Origin]) -> Result<Vec<SourceItemState>, SourceError> {
        Ok(origins
            .iter()
            .map(|origin| SourceItemState {
                origin: origin.clone(),
                state: ItemState::Open,
                note: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasq_core::model::Tag;

    #[test]
    fn fences() {
        assert_eq!(strip_fences("  [1]  "), "[1]");
        assert_eq!(strip_fences("```json\n[1]\n```"), "[1]");
        assert_eq!(strip_fences("```\n[1]\n```\n"), "[1]");
        assert_eq!(strip_fences("```json\n[1]"), "[1]");
        assert_eq!(strip_fences("```"), "");
    }

    #[test]
    fn parses_arrays_objects_and_claude_envelopes() {
        let items = parse_items("[{\"external_id\":\"a\",\"title\":\"A\"}]").unwrap();
        assert_eq!(items, vec![SourceItem::new("a", "A")]);
        let items =
            parse_items("{\"items\":[{\"external_id\":\"a\",\"title\":\"A\",\"state\":\"done\"}]}")
                .unwrap();
        assert_eq!(items[0].state, ItemState::Done);
        let envelope = serde_json::json!({
            "type": "result",
            "result": "```json\n[{\"external_id\":\"b\",\"title\":\"B\",\"tags\":[\"slack\"]}]\n```"
        })
        .to_string();
        let items = parse_items(&envelope).unwrap();
        assert_eq!(items[0].tags, vec![Tag::new("slack").unwrap()]);
        assert_eq!(parse_items("[]").unwrap(), Vec::new());
    }

    #[test]
    fn invalid_output_names_the_problem_with_an_excerpt() {
        let err = |t: &str| match parse_items(t).unwrap_err() {
            SourceError::InvalidOutput { message, excerpt } => (message, excerpt),
            other => panic!("{other:?}"),
        };
        let (m, e) = err("Sure! Here are your tasks");
        assert!(m.starts_with("not JSON:"), "{m}");
        assert_eq!(e, "Sure! Here are your tasks");
        let (m, _) = err("{\"foo\":1}");
        assert_eq!(
            m,
            "expected a JSON array of items (or an object with \"items\" or \"result\")"
        );
        let (m, _) = err("{\"result\": 3}");
        assert!(m.starts_with("expected a JSON array"));
        let (m, _) = err("42");
        assert_eq!(m, "expected a JSON array of items");
        let (m, _) = err("[{\"external_id\":\"a\"}]");
        assert!(m.starts_with("item 0: missing field `title`"), "{m}");
        let long = format!("[{}", "x".repeat(400));
        let (_, e) = err(&long);
        assert_eq!(e.chars().count(), 200);
    }

    #[test]
    fn deduplication() {
        let items = vec![
            SourceItem::new("a", "Fix  the build").with_url("https://x/1"),
            SourceItem::new("a", "Other"),
            SourceItem::new("b", "Different").with_url("https://x/1"),
            SourceItem::new("c", "fix the BUILD"),
            SourceItem::new("d", "Kept").with_url("https://x/2"),
        ];
        let deduped = dedupe(items);
        let kept: Vec<&str> = deduped.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(kept, vec!["a", "d"]);
        assert_eq!(normalize_title("  Fix   The\tBuild "), "fix the build");
    }

    fn bridge(dir: &std::path::Path, body: &str, prompt: Option<&str>) -> LlmBridge {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("bridge");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..200 {
            if Command::new(&script)
                .env("BRIDGE_PROBE", "1")
                .output()
                .is_ok()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let prompt_file = prompt.map(|p| {
            let f = dir.join("prompt.md");
            std::fs::write(&f, p).unwrap();
            f
        });
        LlmBridge {
            name: "inbox".into(),
            command: format!("{} --flag", script.display()),
            prompt_file,
            env: vec![("PATH".into(), "/nonexistent".into())],
        }
    }

    #[test]
    fn fetch_runs_the_command_with_prompt_and_known_ids() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("bridge.log");
        let source = bridge(
            dir.path(),
            &format!(
                "[ -n \"${{BRIDGE_PROBE:-}}\" ] && exit 0\nread -r prompt\nprintf '%s\\n' \"$TASQ_SYNC_KNOWN\" > '{}'\necho \"[{{\\\"external_id\\\":\\\"$1 $prompt\\\",\\\"title\\\":\\\"T\\\"}}]\"",
                log.display()
            ),
            Some("hello\n"),
        );
        assert_eq!(source.name(), "inbox");
        let ctx = SyncContext {
            known: vec![Origin {
                source: "inbox".into(),
                external_id: "k1".into(),
                url: None,
            }],
        };
        let items = source.fetch(&ctx).unwrap();
        assert_eq!(items[0].external_id, "--flag hello");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "[\"k1\"]\n");
        let states = source.check(&ctx.known).unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(
            (states[0].state, states[0].note.as_deref()),
            (ItemState::Open, None)
        );
        assert_eq!(states[0].origin, ctx.known[0]);
        assert_eq!(source.check(&[]).unwrap(), Vec::new());
    }

    #[test]
    fn fetch_failures() {
        let dir = tempfile::tempdir().unwrap();
        let failing = bridge(
            dir.path(),
            "[ -n \"${BRIDGE_PROBE:-}\" ] && exit 0\necho 'rate limited' >&2\nexit 1",
            None,
        );
        assert_eq!(
            failing.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::Unavailable(format!(
                "command {:?} failed: rate limited",
                failing.command
            ))
        );
        let garbage = bridge(
            dir.path(),
            "[ -n \"${BRIDGE_PROBE:-}\" ] && exit 0\necho 'nope'",
            None,
        );
        assert!(matches!(
            garbage.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::InvalidOutput { .. }
        ));
        let missing_prompt = LlmBridge {
            prompt_file: Some(dir.path().join("absent.md")),
            ..garbage.clone()
        };
        assert!(matches!(
            missing_prompt.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::Unavailable(m) if m.starts_with("prompt_file")
        ));
        let bad_command = LlmBridge {
            command: "x 'unbalanced".into(),
            ..garbage.clone()
        };
        assert!(matches!(
            bad_command.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::Unavailable(m) if m.starts_with("command")
        ));
        let absent = LlmBridge {
            command: "no-such-bridge".into(),
            ..garbage
        };
        assert!(matches!(
            absent.fetch(&SyncContext::default()).unwrap_err(),
            SourceError::Unavailable(m) if m.contains("could not run")
        ));
    }
}
