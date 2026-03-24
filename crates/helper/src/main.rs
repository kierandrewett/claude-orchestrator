use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use tracing::{debug, info, warn};

#[derive(Parser)]
#[command(name = "claude-orchestrator-helper", about = "Helper CLI for Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Rename the current conversation topic.
    RenameConversation {
        /// The new title for the conversation.
        title: String,
    },
    /// Run as a stdio MCP server (used by Claude Code via --mcp-config).
    Mcp,
}

#[derive(Serialize)]
struct ActionRequest {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

fn call_rename_conversation(title: &str) -> Result<(), String> {
    let orchestrator_url = std::env::var("ORCHESTRATOR_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8765".to_string());

    let session_id = match std::env::var("ORCHESTRATOR_SESSION_ID") {
        Ok(v) if !v.is_empty() => v,
        _ => return Err("ORCHESTRATOR_SESSION_ID environment variable is not set".to_string()),
    };

    let token = std::env::var("ORCHESTRATOR_TOKEN").ok();

    let body = ActionRequest {
        action: "rename_conversation".to_string(),
        title: Some(title.to_string()),
    };

    let url = format!(
        "{}/api/session/{}/action",
        orchestrator_url.trim_end_matches('/'),
        session_id
    );

    let client = reqwest::blocking::Client::new();
    let mut req = client.post(&url).json(&body);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }

    match req.send() {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            if status.is_success() {
                Ok(())
            } else {
                Err(format!("server returned {status}: {body}"))
            }
        }
        Err(e) => Err(format!("request failed: {e}")),
    }
}

fn run_mcp() {
    let session_id = std::env::var("ORCHESTRATOR_SESSION_ID").unwrap_or_default();
    let suppress: std::collections::HashSet<String> = std::env::var("ORCHESTRATOR_SUPPRESS_TOOLS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let allowed_emojis: Vec<String> = std::env::var("ORCHESTRATOR_ALLOWED_EMOJIS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .collect();
    info!(session_id = %session_id, suppress = ?suppress, allowed_emojis = %allowed_emojis.len(), "MCP server started");

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => { warn!("stdin read error: {e}"); break; }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => { warn!("failed to parse JSON-RPC message: {e}"); continue; }
        };

        let method = req["method"].as_str().unwrap_or("");
        let id = req.get("id").cloned();
        debug!(method = %method, "received request");

        let response: Option<Value> = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "claude-orchestrator", "version": "0.1.0"}
                }
            })),

            "notifications/initialized" | "notifications/cancelled" => None,

            "tools/list" => {
                let emoji_desc = if allowed_emojis.is_empty() {
                    "Use any relevant emoji.".to_string()
                } else {
                    format!(
                        "You MUST choose the emoji from this exact list: {}",
                        allowed_emojis.join(", ")
                    )
                };
                let all_tools = vec![json!({
                    "name": "rename_conversation",
                    "description": format!(
                        "Rename the current conversation in the chat backend. \
                         Call this once after your first substantive response, and again when the topic shifts. \
                         Format: <emoji> <short phrase> (3-5 words). {emoji_desc} \
                         IMPORTANT: Call this tool directly by name — do NOT use ToolSearch to find it first. \
                         After calling this tool, your turn is complete. Do not generate any further text."
                    ),
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Title with leading emoji, e.g. \"🎯 Fix the parser bug\""
                            }
                        },
                        "required": ["title"]
                    }
                })];
                let visible: Vec<_> = all_tools
                    .into_iter()
                    .filter(|t| !suppress.contains(t["name"].as_str().unwrap_or("")))
                    .collect();
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "tools": visible }
                }))
            }

            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                // Reject suppressed tools even if somehow called.
                if suppress.contains(name) {
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("Tool '{name}' is not available in this session")}
                    }))
                } else { match name {
                    "rename_conversation" => {
                        let title = req["params"]["arguments"]["title"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        info!(title = %title, "rename_conversation called");

                        // If an allowed emoji list is configured, validate that the title
                        // starts with one of the allowed emojis. Return an error so Claude
                        // is forced to retry with a valid emoji rather than silently accepting.
                        let emoji_error: Option<String> = if !allowed_emojis.is_empty() {
                            let first_token = title.split_whitespace().next().unwrap_or("");
                            if allowed_emojis.iter().any(|e| e == first_token) {
                                None
                            } else {
                                warn!(title = %title, "rename_conversation: emoji not in allowed list");
                                Some(format!(
                                    "Invalid emoji. The title must begin with one of the allowed emojis: {}\n\
                                     Please retry with a title starting with an emoji from that list.",
                                    allowed_emojis.join(", ")
                                ))
                            }
                        } else {
                            None
                        };

                        Some(if let Some(err_msg) = emoji_error {
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{"type": "text", "text": err_msg}],
                                    "isError": true
                                }
                            })
                        } else {
                            match call_rename_conversation(&title) {
                                Ok(()) => { info!("rename_conversation succeeded"); json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "content": [],
                                        "isError": false
                                    }
                                }) },
                                Err(e) => { warn!(error = %e, "rename_conversation failed"); json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": {
                                        "content": [{"type": "text", "text": format!("Error: {e}")}],
                                        "isError": true
                                    }
                                }) },
                            }
                        })
                    }
                    _ => Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("Unknown tool: {name}")}
                    })),
                } }
            }

            _ => id.as_ref().map(|_| json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "Method not found"}
            })),
        };

        if let Some(resp) = response {
            let s = serde_json::to_string(&resp).unwrap_or_default();
            let _ = writeln!(stdout, "{s}");
            let _ = stdout.flush();
        }
    }
}

fn main() {
    // Always write logs to stderr — stdout is reserved for the MCP JSON-RPC stream.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Mcp => run_mcp(),

        Commands::RenameConversation { title } => {
            match call_rename_conversation(&title) {
                Ok(()) => println!("ok"),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
