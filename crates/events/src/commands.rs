use anyhow::{bail, Result};

use crate::types::TaskId;

/// A parsed slash command from any backend.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCommand {
    /// `/new <profile> <prompt>` — create a new task.
    New { profile: String, prompt: String },

    /// `/stop [task_id]` — stop the current (or specified) task.
    Stop { task_id: Option<TaskId> },

    /// `/status` — list all tasks.
    Status,

    /// `/cost [all]` — show usage/cost.
    Cost { all: bool },

    /// `/hibernate` — hibernate the current task.
    Hibernate,

    /// `/cancel` — interrupt the current Claude response (SIGINT), keeps session alive.
    Cancel,

    /// `/config <key> <value>` — update a task config option.
    Config { key: String, value: String },
}

/// Parse a text string beginning with `/` into a `ParsedCommand`.
pub fn parse(text: &str) -> Result<ParsedCommand> {
    let text = text.trim();
    if !text.starts_with('/') {
        bail!("not a command (must start with '/')");
    }

    let mut parts = text.splitn(2, char::is_whitespace);
    let raw_cmd = parts.next().unwrap_or("");
    // Telegram appends @BotName to commands in groups (e.g. /status@MyBot).
    let cmd = raw_cmd
        .split('@')
        .next()
        .unwrap_or(raw_cmd)
        .to_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    match cmd.as_str() {
        "/new" => {
            let mut iter = rest.splitn(2, char::is_whitespace);
            let profile = iter
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("base")
                .to_string();
            let prompt = iter.next().unwrap_or("").trim().to_string();
            Ok(ParsedCommand::New { profile, prompt })
        }

        "/stop" => {
            let task_id = if rest.is_empty() {
                None
            } else {
                Some(TaskId(rest.to_string()))
            };
            Ok(ParsedCommand::Stop { task_id })
        }

        "/status" => Ok(ParsedCommand::Status),

        "/cost" => Ok(ParsedCommand::Cost { all: rest == "all" }),

        "/hibernate" => Ok(ParsedCommand::Hibernate),

        "/cancel" => Ok(ParsedCommand::Cancel),

        "/config" => {
            let mut iter = rest.splitn(2, char::is_whitespace);
            let key = iter
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: /config <key> <value>"))?
                .to_string();
            let value = iter
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: /config <key> <value>"))?
                .to_string();
            Ok(ParsedCommand::Config { key, value })
        }

        other => bail!("unknown command '{}'", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_new() {
        let cmd = parse("/new rust fix the parser").unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::New {
                profile: "rust".to_string(),
                prompt: "fix the parser".to_string()
            }
        );
    }

    #[test]
    fn parse_stop_no_id() {
        let cmd = parse("/stop").unwrap();
        assert_eq!(cmd, ParsedCommand::Stop { task_id: None });
    }

    #[test]
    fn parse_stop_with_id() {
        let cmd = parse("/stop abc123").unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::Stop {
                task_id: Some(TaskId("abc123".to_string()))
            }
        );
    }

    #[test]
    fn parse_status() {
        assert_eq!(parse("/status").unwrap(), ParsedCommand::Status);
        assert_eq!(parse("/STATUS").unwrap(), ParsedCommand::Status);
    }

    #[test]
    fn parse_cost_all() {
        assert_eq!(parse("/cost all").unwrap(), ParsedCommand::Cost { all: true });
        assert_eq!(parse("/cost").unwrap(), ParsedCommand::Cost { all: false });
    }

    #[test]
    fn parse_config() {
        let cmd = parse("/config thinking on").unwrap();
        assert_eq!(
            cmd,
            ParsedCommand::Config {
                key: "thinking".to_string(),
                value: "on".to_string()
            }
        );
    }

    #[test]
    fn unknown_command_errors() {
        assert!(parse("/unknown").is_err());
    }

    #[test]
    fn not_a_command_errors() {
        assert!(parse("hello world").is_err());
    }
}
