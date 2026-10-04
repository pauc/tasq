//! The command summarizer behind `tasq summary` (plan T-601): a configured
//! command (`report.summary.command`, by default `claude -p`) reads the
//! rendered prompt on stdin and prints the standup summary.
//!
//! The prompt is data: the built-in template is `templates/summary.md`,
//! `report.summary.prompt_file` replaces it. It is rendered with the same
//! tiny engine as the Claude launcher prompt ([`crate::prompt::render`]) and
//! knows three placeholders: `{{day}}` (`Friday 2026-10-02`), `{{date}}`
//! (`2026-10-02`) and `{{notes}}` (the raw notes, see
//! [`DaySummary::raw`]).

use std::collections::BTreeMap;
use std::path::Path;

use tasq_core::clock::format_date;
use tasq_core::config::{Summarizer as SummarizerKind, SummaryConfig};
use tasq_core::launch::LaunchError;
use tasq_core::report::{DaySummary, RawSummarizer, ReportError, Summarizer};

use crate::process::{run_with_input, which};
use crate::prompt::render;

/// The built-in summary prompt template.
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/summary.md");

/// The name of this summarizer in messages and `--json`.
pub const NAME: &str = "llm";

/// Runs `report.summary.command` with the rendered prompt on stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSummarizer {
    /// `report.summary.command`, split like a shell command line.
    pub command: String,
    /// `report.summary.model`, appended as `--model <model>` when set.
    pub model: Option<String>,
    /// The prompt template text (built-in or `report.summary.prompt_file`).
    pub template: String,
    /// The environment the command runs with.
    pub env: Vec<(String, String)>,
}

impl CommandSummarizer {
    /// The command line: the configured command plus `--model <model>`.
    /// An empty or unbalanced command is [`ReportError::Unavailable`].
    pub fn argv(&self) -> Result<Vec<String>, ReportError> {
        let unavailable = |reason: String| ReportError::Unavailable {
            summarizer: NAME.to_owned(),
            reason,
        };
        let mut argv = shell_words::split(&self.command)
            .map_err(|e| unavailable(format!("report.summary.command {:?}: {e}", self.command)))?;
        if argv.is_empty() {
            return Err(unavailable(
                "report.summary.command is empty (set it, or report.summary.summarizer = \"raw\")"
                    .to_owned(),
            ));
        }
        if let Some(model) = &self.model {
            argv.push("--model".to_owned());
            argv.push(model.clone());
        }
        Ok(argv)
    }

    /// What the command reads on stdin: the template with `{{day}}`,
    /// `{{date}}` and `{{notes}}` filled in.
    pub fn input(&self, summary: &DaySummary) -> Result<String, ReportError> {
        let vars = BTreeMap::from([
            ("day", summary.header()),
            ("date", format_date(summary.day)),
            ("notes", summary.raw().trim_end().to_owned()),
        ]);
        render(&self.template, &vars).map_err(|e| match e {
            LaunchError::Template(message) => ReportError::Template(message),
            other => ReportError::Template(other.to_string()),
        })
    }

    /// Checks that `program` can be run: looked up on this summarizer's
    /// `PATH` unless it carries a directory, in which case it must exist.
    pub fn check_program(&self, program: &str) -> Result<(), ReportError> {
        let found = if program.contains('/') {
            Path::new(program).is_file()
        } else {
            which(&self.env, program).is_some()
        };
        if found {
            Ok(())
        } else {
            Err(ReportError::Unavailable {
                summarizer: NAME.to_owned(),
                reason: format!(
                    "{program} is not on PATH (install it, use --raw for the notes themselves, \
                     or set report.summary.summarizer = \"raw\")"
                ),
            })
        }
    }
}

impl Summarizer for CommandSummarizer {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Runs the command. Its stdout, without trailing newlines, is the
    /// summary; a non-zero exit is [`ReportError::Failed`] with its stderr.
    fn summarize(&self, summary: &DaySummary) -> Result<String, ReportError> {
        let argv = self.argv()?;
        let input = self.input(summary)?;
        self.check_program(&argv[0])?;
        let finished = run_with_input(&argv, &input, &self.env);
        if finished.success {
            Ok(finished.stdout.trim_end().to_owned())
        } else {
            Err(ReportError::Failed {
                summarizer: NAME.to_owned(),
                reason: format!("{} failed: {}", argv.join(" "), finished.message()),
            })
        }
    }
}

/// The summarizer `[report.summary]` asks for: [`RawSummarizer`] for `raw`
/// (or when `force_raw`, the `--raw` flag), else a [`CommandSummarizer`]
/// with `template` as its prompt.
pub fn summarizer_for(
    config: &SummaryConfig,
    template: String,
    env: Vec<(String, String)>,
    force_raw: bool,
) -> Box<dyn Summarizer> {
    if force_raw || config.summarizer == SummarizerKind::Raw {
        Box::new(RawSummarizer)
    } else {
        Box::new(CommandSummarizer {
            command: config.command.clone(),
            model: config.model.clone(),
            template,
            env,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tasq_core::clock::parse_date;
    use tasq_core::model::TaskId;
    use tasq_core::report::TaskNotes;

    use super::*;

    fn summary() -> DaySummary {
        DaySummary {
            day: parse_date("2026-10-02").unwrap(),
            tasks: vec![
                TaskNotes {
                    id: TaskId::from(1),
                    title: "Parser".into(),
                    done: false,
                    notes: vec!["parser done".into()],
                },
                TaskNotes {
                    id: TaskId::from(3),
                    title: "Release".into(),
                    done: true,
                    notes: vec!["tagged".into(), "shipped".into()],
                },
            ],
        }
    }

    /// A directory holding fake programs, and the env pointing `PATH` at it.
    struct Bin {
        dir: tempfile::TempDir,
    }

    impl Bin {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn install(&self, name: &str, body: &str) -> String {
            let path = self.dir.path().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path.display().to_string()
        }

        fn env(&self) -> Vec<(String, String)> {
            vec![("PATH".to_owned(), self.dir.path().display().to_string())]
        }
    }

    fn summarizer(command: &str, env: Vec<(String, String)>) -> CommandSummarizer {
        CommandSummarizer {
            command: command.to_owned(),
            model: None,
            template: DEFAULT_TEMPLATE.to_owned(),
            env,
        }
    }

    #[test]
    fn argv_splits_the_command_and_appends_the_model() {
        let mut s = summarizer("claude -p", Vec::new());
        assert_eq!(s.argv().unwrap(), vec!["claude", "-p"]);
        s.model = Some("sonnet".to_owned());
        assert_eq!(s.argv().unwrap(), vec!["claude", "-p", "--model", "sonnet"]);
        s.command = "llm 'two words'".to_owned();
        assert_eq!(
            s.argv().unwrap(),
            vec!["llm", "two words", "--model", "sonnet"]
        );
    }

    #[test]
    fn argv_rejects_empty_and_unbalanced_commands() {
        let s = summarizer("", Vec::new());
        assert_eq!(
            s.argv().unwrap_err().to_string(),
            "summarizer llm: report.summary.command is empty (set it, or report.summary.summarizer = \"raw\")"
        );
        let s = summarizer("claude 'oops", Vec::new());
        let msg = s.argv().unwrap_err().to_string();
        assert!(
            msg.starts_with("summarizer llm: report.summary.command \"claude 'oops\": "),
            "{msg}"
        );
    }

    #[test]
    fn input_renders_the_default_template_with_day_and_notes() {
        let s = summarizer("claude -p", Vec::new());
        let input = s.input(&summary()).unwrap();
        assert!(
            input.starts_with(
                "Below are the raw progress notes from my task tracker for Friday 2026-10-02, "
            ),
            "{input}"
        );
        assert!(
            input.ends_with(
                "The notes:\n\n- [1] Parser — parser done\n- [3] Release (done)\n    - tagged\n    - shipped\n"
            ),
            "{input}"
        );
        assert!(!input.contains("{{"), "{input}");
    }

    #[test]
    fn input_uses_a_custom_template_and_reports_bad_placeholders() {
        let mut s = summarizer("claude -p", Vec::new());
        s.template = "{{date}}|{{day}}|{{notes}}".to_owned();
        assert_eq!(
            s.input(&summary()).unwrap(),
            "2026-10-02|Friday 2026-10-02|- [1] Parser — parser done\n- [3] Release (done)\n    - tagged\n    - shipped"
        );
        s.template = "{{notes}} {{model}}".to_owned();
        assert_eq!(
            s.input(&summary()),
            Err(ReportError::Template(
                "unknown placeholder {{model}}".into()
            ))
        );
        s.template = "{{#notes}}x{{/day}}".to_owned();
        assert_eq!(
            s.input(&summary()),
            Err(ReportError::Template(
                "section {{#notes}} is closed by {{/day}}".into()
            ))
        );
    }

    #[test]
    fn missing_program_is_unavailable_with_the_fix() {
        let bin = Bin::new();
        let s = summarizer("claude -p", bin.env());
        assert_eq!(
            s.summarize(&summary()).unwrap_err().to_string(),
            "summarizer llm: claude is not on PATH (install it, use --raw for the notes themselves, or set report.summary.summarizer = \"raw\")"
        );
        assert_eq!(
            s.check_program("/nonexistent/claude").unwrap_err(),
            ReportError::Unavailable {
                summarizer: "llm".into(),
                reason: "/nonexistent/claude is not on PATH (install it, use --raw for the notes themselves, or set report.summary.summarizer = \"raw\")".into()
            }
        );
        assert_eq!(s.name(), "llm");
    }

    #[test]
    fn runs_the_command_with_the_prompt_on_stdin() {
        let bin = Bin::new();
        bin.install(
            "claude",
            "[ \"$1\" = -p ] || exit 9\necho \"args:$*\"\nIFS= read -r first\necho \"$first\"\nwhile IFS= read -r line || [ -n \"$line\" ]; do last=$line; done\necho \"$last\"\n",
        );
        let mut s = summarizer("claude -p", bin.env());
        s.model = Some("haiku".to_owned());
        s.template = "first line\n{{notes}}".to_owned();
        assert_eq!(
            s.summarize(&summary()).unwrap(),
            "args:-p --model haiku\nfirst line\n    - shipped"
        );
    }

    #[test]
    fn an_absolute_program_path_works_and_failures_carry_stderr() {
        let bin = Bin::new();
        let ok = bin.install("ok", "echo summary; echo");
        let s = summarizer(&ok, Vec::new());
        assert_eq!(s.summarize(&summary()).unwrap(), "summary");
        let failing = bin.install("failing", "echo 'rate limited' >&2\nexit 3");
        let s = summarizer(&format!("{failing} -p"), Vec::new());
        assert_eq!(
            s.summarize(&summary()).unwrap_err(),
            ReportError::Failed {
                summarizer: "llm".into(),
                reason: format!("{failing} -p failed: rate limited")
            }
        );
    }

    #[test]
    fn summarizer_for_honours_config_and_the_raw_flag() {
        let config = SummaryConfig::default();
        let s = summarizer_for(&config, "t".into(), Vec::new(), false);
        assert_eq!(s.name(), "llm");
        let s = summarizer_for(&config, "t".into(), Vec::new(), true);
        assert_eq!(s.name(), "raw");
        let raw = SummaryConfig {
            summarizer: SummarizerKind::Raw,
            ..SummaryConfig::default()
        };
        let s = summarizer_for(&raw, "t".into(), Vec::new(), false);
        assert_eq!(s.name(), "raw");
        assert_eq!(s.summarize(&summary()).unwrap(), summary().raw());
        let llm = SummaryConfig {
            command: "echo".into(),
            model: Some("m".into()),
            ..SummaryConfig::default()
        };
        let s = summarizer_for(
            &llm,
            "{{date}}".into(),
            vec![("PATH".into(), "/bin:/usr/bin".into())],
            false,
        );
        assert_eq!(s.summarize(&summary()).unwrap(), "--model m");
    }
}
