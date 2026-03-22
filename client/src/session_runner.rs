use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::SinkExt;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::warn;

use crate::protocol::{AttachedFile, SessionStats, C2S, S2C};

pub struct SessionConfig {
    pub session_id: String,
    pub initial_prompt: Option<String>,
    pub extra_args: Vec<String>,
    pub claude_path: String,
    pub claude_session_id: String, // pre-generated UUID for --session-id or --resume
    pub is_resume: bool,           // true = use --resume, false = use --session-id
    pub default_cwd: String,       // working directory for spawning claude
}

/// Type alias for the write-half of a tokio-tungstenite WebSocket stream.
pub type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;

/// Spawns a Claude process for this session, streams its output back as
/// `C2S::SessionEvent` messages, and forwards stdin commands from the server.
/// Runs a Claude session inside a Firecracker microVM.
///
/// Claude is **never** spawned on the host. If the VM config cannot be loaded
/// or is missing, the session is aborted with an error.
pub async fn run_session(
    config: SessionConfig,
    ws_tx: Arc<Mutex<WsSink>>,
    cmd_rx: mpsc::Receiver<S2C>,
) {
    let vm_cfg = match crate::vm::config::VmConfig::load() {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            tracing::error!(
                "session {}: no VM config found at {} — refusing to run on host",
                config.session_id,
                crate::vm::config::VmConfig::config_path().display(),
            );
            abort_session(&ws_tx, config.session_id).await;
            return;
        }
        Err(e) => {
            tracing::error!(
                "session {}: failed to load VM config — refusing to run on host: {e}",
                config.session_id,
            );
            abort_session(&ws_tx, config.session_id).await;
            return;
        }
    };

    crate::vm::run_vm_session(config, ws_tx, cmd_rx, vm_cfg).await;
}

async fn abort_session(ws_tx: &Arc<Mutex<WsSink>>, session_id: String) {
    ws_send(
        ws_tx,
        &crate::protocol::C2S::SessionEnded {
            session_id,
            exit_code: 1,
            stats: Default::default(),
        },
    )
    .await;
}


// -----------------------------------------------------------------------------
// Helper: format a user message with attached files for Claude's stream-json
// (pub so vm/mod.rs can reuse it)
// stdin format. Images and PDFs are embedded as base64 content blocks;
// other file types are saved to /tmp/telegram_attachments/ and referenced
// by path so Claude can read them with its Read tool.
// -----------------------------------------------------------------------------
pub async fn format_user_message_with_files(text: &str, files: &[AttachedFile]) -> String {
    let mut blocks: Vec<serde_json::Value> = Vec::new();
    let mut saved_paths: Vec<String> = Vec::new();

    for file in files {
        if file.mime_type.starts_with("image/") {
            // Claude supports image/jpeg, image/png, image/gif, image/webp.
            let media_type = match file.mime_type.as_str() {
                "image/jpeg" | "image/png" | "image/gif" | "image/webp" => {
                    file.mime_type.clone()
                }
                _ => "image/jpeg".to_string(),
            };
            blocks.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": file.data_base64
                }
            }));
        } else if file.mime_type == "application/pdf" {
            blocks.push(serde_json::json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": file.data_base64
                }
            }));
        } else {
            // For other file types: decode and save to disk, tell Claude the path.
            let save_dir = std::path::Path::new("/tmp/telegram_attachments");
            if let Err(e) = tokio::fs::create_dir_all(save_dir).await {
                warn!("format_user_message_with_files: failed to create dir: {e}");
                continue;
            }
            let path = save_dir.join(&file.filename);
            match STANDARD.decode(&file.data_base64) {
                Ok(bytes) => {
                    if let Err(e) = tokio::fs::write(&path, &bytes).await {
                        warn!("format_user_message_with_files: failed to write {}: {e}", file.filename);
                    } else {
                        saved_paths.push(path.display().to_string());
                    }
                }
                Err(e) => {
                    warn!("format_user_message_with_files: base64 decode error for {}: {e}", file.filename);
                }
            }
        }
    }

    // Build the text portion, appending any saved file paths.
    let mut full_text = text.to_string();
    if !saved_paths.is_empty() {
        if !full_text.is_empty() {
            full_text.push('\n');
        }
        full_text.push_str("Attached file(s) available at: ");
        full_text.push_str(&saved_paths.join(", "));
    }

    if !full_text.is_empty() {
        blocks.push(serde_json::json!({ "type": "text", "text": full_text }));
    }

    if blocks.is_empty() {
        return String::new();
    }

    // Plain text-only — use the simple string format for clarity.
    if blocks.len() == 1 {
        if let Some(t) = blocks[0].get("text").and_then(|v| v.as_str()) {
            return format_user_message(t);
        }
    }

    let content_json =
        serde_json::to_string(&blocks).unwrap_or_else(|_| r#""[serialisation error]""#.to_string());
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}

// -----------------------------------------------------------------------------
// Helper: format a user message for Claude's stream-json stdin format
// -----------------------------------------------------------------------------
fn format_user_message(text: &str) -> String {
    // Serialise the content string via serde_json so any special characters
    // are properly escaped inside the JSON output.
    let content_json = serde_json::to_string(text).unwrap_or_else(|_| format!("{text:?}"));
    // content_json is already a valid JSON string literal (with surrounding quotes).
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}

// -----------------------------------------------------------------------------
// Stats accumulation
// -----------------------------------------------------------------------------
pub fn update_stats(stats: &mut SessionStats, event: &serde_json::Value) {
    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

    // With --include-partial-messages, Anthropic streaming events are wrapped
    // in a stream_event envelope — recurse into the inner event.
    if event_type == "stream_event" {
        if let Some(inner) = event.get("event") {
            update_stats(stats, inner);
        }
        return;
    }

    match event_type {
        "message_delta" => {
            if let Some(reason) = event.pointer("/delta/stop_reason").and_then(|r| r.as_str()) {
                stats.stop_reason = Some(reason.to_string());
            }
        }
        "assistant" => {
            // Accumulate per-turn token usage.
            if let Some(tokens) = event
                .pointer("/message/usage/input_tokens")
                .and_then(|v| v.as_u64())
            {
                stats.input_tokens = stats.input_tokens.saturating_add(tokens);
            }
            if let Some(tokens) = event
                .pointer("/message/usage/output_tokens")
                .and_then(|v| v.as_u64())
            {
                stats.output_tokens = stats.output_tokens.saturating_add(tokens);
            }
            // Scan content array for tool_use blocks.
            if let Some(content) = event.pointer("/message/content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                            *stats.tool_calls.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        "result" => {
            // Authoritative final cost — field changed to total_cost_usd in Claude Code 2.x.
            if let Some(cost) = event.get("total_cost_usd").and_then(|c| c.as_f64()) {
                stats.cost_usd = Some(cost);
            }
            // Authoritative final token totals override accumulated per-turn counts.
            if let Some(tokens) = event.pointer("/usage/input_tokens").and_then(|v| v.as_u64()) {
                stats.input_tokens = tokens;
            }
            if let Some(tokens) = event.pointer("/usage/output_tokens").and_then(|v| v.as_u64()) {
                stats.output_tokens = tokens;
            }
            if let Some(turns) = event.get("num_turns").and_then(|t| t.as_u64()) {
                stats.turns = turns as u32;
            }
        }
        _ => {}
    }
}

// -----------------------------------------------------------------------------
// WebSocket send helper
// -----------------------------------------------------------------------------
pub async fn ws_send(ws_tx: &Mutex<WsSink>, msg: &C2S) {
    match serde_json::to_string(msg) {
        Ok(json) => {
            let mut sink = ws_tx.lock().await;
            if let Err(e) = sink
                .send(tokio_tungstenite::tungstenite::Message::Text(json))
                .await
            {
                warn!("ws_send error: {e}");
            }
        }
        Err(e) => {
            warn!("failed to serialise C2S message: {e}");
        }
    }
}
