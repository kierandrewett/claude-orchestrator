use anyhow::{bail, Result};

use crate::types::TaskId;

/// A parsed slash command from any backend.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCommand {
    /// Create a new task with a profile and optional prompt.
    /// Used by the web API and Discord; not exposed as a Telegram text command.
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

    /// `/mcp [list]` — list configured MCP servers.
    McpList,

    /// `/mcp add <name> <command> [args...]` — add a new MCP server.
    McpAdd { name: String, command: String, args: Vec<String> },

    /// `/mcp remove <name>` — remove a custom MCP server.
    McpRemove { name: String },

    /// `/mcp disable <name>` — disable an MCP server (including built-ins).
    McpDisable { name: String },

    /// `/mcp enable <name>` — re-enable a disabled MCP server.
    McpEnable { name: String },

    /// `/events` or `/events list` — list all scheduled events.
    EventsList,
    /// `/events info <id>` — details for one event.
    EventsInfo { id: String },
    /// `/events enable <id>` — enable a paused event.
    EventsEnable { id: String },
    /// `/events disable <id>` — disable an event.
    EventsDisable { id: String },
    /// `/events delete <id>` — permanently delete an event.
    EventsDelete { id: String },
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
                .ok_or_else(|| anyhow::anyhow!("usage: /config KEY VALUE"))?
                .to_string();
            let value = iter
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("usage: /config KEY VALUE"))?
                .to_string();
            Ok(ParsedCommand::Config { key, value })
        }

        "/mcp" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            match parts.first().map(|s| s.to_lowercase()).as_deref() {
                None | Some("list") => Ok(ParsedCommand::McpList),
                Some("add") => {
                    let name = parts
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("usage: /mcp add NAME COMMAND [args...]"))?
                        .to_string();
                    let command = parts
                        .get(2)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("usage: /mcp add NAME COMMAND [args...]"))?
                        .to_string();
                    let args = parts[3..].iter().map(|s| s.to_string()).collect();
                    Ok(ParsedCommand::McpAdd { name, command, args })
                }
                Some("remove") => {
                    let name = parts
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("usage: /mcp remove NAME"))?
                        .to_string();
                    Ok(ParsedCommand::McpRemove { name })
                }
                Some("disable") => {
                    let name = parts
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("usage: /mcp disable NAME"))?
                        .to_string();
                    Ok(ParsedCommand::McpDisable { name })
                }
                Some("enable") => {
                    let name = parts
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("usage: /mcp enable NAME"))?
                        .to_string();
                    Ok(ParsedCommand::McpEnable { name })
                }
                Some(other) => bail!("unknown /mcp subcommand '{}'. Use: list, add, remove, disable, enable", other),
            }
        }

        "/events" => {
            let parts: Vec<String> = rest.split_whitespace().map(|s| s.to_string()).collect();
            match parts.get(0).map(|s| s.as_str()) {
                None | Some("list") => Ok(ParsedCommand::EventsList),
                Some("info") => {
                    let id = parts.get(1).cloned()
                        .ok_or_else(|| anyhow::anyhow!("usage: /events info ID"))?;
                    Ok(ParsedCommand::EventsInfo { id })
                }
                Some("enable") => {
                    let id = parts.get(1).cloned()
                        .ok_or_else(|| anyhow::anyhow!("usage: /events enable ID"))?;
                    Ok(ParsedCommand::EventsEnable { id })
                }
                Some("disable") => {
                    let id = parts.get(1).cloned()
                        .ok_or_else(|| anyhow::anyhow!("usage: /events disable ID"))?;
                    Ok(ParsedCommand::EventsDisable { id })
                }
                Some("delete") => {
                    let id = parts.get(1).cloned()
                        .ok_or_else(|| anyhow::anyhow!("usage: /events delete ID"))?;
                    Ok(ParsedCommand::EventsDelete { id })
                }
                Some(other) => bail!("unknown /events subcommand '{}'. Use: list, info, enable, disable, delete", other),
            }
        }

        other => bail!("unknown command '{}'", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
