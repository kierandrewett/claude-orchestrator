use crate::runner::{native::NativeRunner, Runner};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures_util::SinkExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::protocol::{AttachedFile, SessionStats, C2S, S2C};

/// A message waiting to be delivered to Claude's stdin once it becomes idle.
enum PendingMessage {
    Text { text: String, msg_ref_opaque_id: Option<String> },
    WithFiles { text: String, files: Vec<AttachedFile>, msg_ref_opaque_id: Option<String> },
}

pub struct SessionConfig {
    pub session_id: String,
    pub initial_prompt: Option<String>,
    pub extra_args: Vec<String>,
    pub claude_session_id: String, // pre-generated UUID for --session-id or --resume
    pub is_resume: bool,           // true = use --resume, false = use --session-id
    pub default_cwd: String,       // working directory for spawning claude
    pub system_prompt: Option<String>,
}

/// Type alias for the write-half of a tokio-tungstenite WebSocket stream.
pub type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;

/// Entry point called from the connection loop.  Creates a [`NativeRunner`]
/// and drives the session until Claude exits or the server kills it.
pub async fn run_session(
    config: SessionConfig,
    ws_tx: Arc<Mutex<WsSink>>,
    cmd_rx: mpsc::Receiver<S2C>,
) {
    let runner = NativeRunner;
    let session_id = config.session_id.clone();

    if let Err(e) = do_run(config, &ws_tx, cmd_rx, &runner).await {
        warn!("session {session_id} error: {e:#}");
        ws_send(
            &ws_tx,
            &C2S::SessionEnded {
                session_id,
                exit_code: -1,
                stats: SessionStats::default(),
                error: Some(e.to_string()),
            },
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Generic session loop — runner-agnostic
// ---------------------------------------------------------------------------

async fn do_run<R: Runner>(
    config: SessionConfig,
    ws_tx: &Arc<Mutex<WsSink>>,
    mut cmd_rx: mpsc::Receiver<S2C>,
    runner: &R,
) -> anyhow::Result<()> {
    let session_id = &config.session_id;
    info!("session {session_id}: spawning");

    let mut child = runner.spawn(&config).await?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Drain stderr in the background so it doesn't block the process.
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    {
        let buf = Arc::clone(&stderr_buf);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!("claude stderr: {line}");
                let mut b = buf.lock().await;
                b.push_str(&line);
                b.push('\n');
            }
        });
    }

    // Claude is considered busy as soon as we send the initial prompt.
    // If there's no initial prompt it starts idle, waiting for the first message.
    let mut claude_busy = if let Some(ref prompt) = config.initial_prompt {
        let msg = format_user_message(prompt);
        log_ndjson_out(session_id, &msg);
        stdin
            .write_all(msg.as_bytes())
            .await
            .context("writing initial prompt")?;
        true
    } else {
        false
    };

    // Messages that arrived while Claude was busy, waiting to be delivered.
    let mut queue: VecDeque<PendingMessage> = VecDeque::new();

    ws_send(
        ws_tx,
        &C2S::SessionStarted {
            session_id: session_id.clone(),
            pid: child.id().unwrap_or(0),
            cwd: config.default_cwd.clone(),
        },
    )
    .await;

    let mut stats = SessionStats::default();
    let mut stdout_lines = BufReader::new(stdout).lines();

    const IDLE_TIMEOUT: Duration = Duration::from_secs(12 * 3600);
    let idle_sleep = tokio::time::sleep(IDLE_TIMEOUT);
    tokio::pin!(idle_sleep);

    loop {
        tokio::select! {
            biased;

            cmd = cmd_rx.recv() => match cmd {
                Some(S2C::SendInput { text, message_ref_opaque_id, .. }) => {
                    idle_sleep.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                    if claude_busy {
                        info!("session {session_id}: Claude busy — queuing message ({} already queued)", queue.len());
                        queue.push_back(PendingMessage::Text { text, msg_ref_opaque_id: message_ref_opaque_id });
                    } else {
                        send_to_stdin(&mut stdin, session_id, format_user_message(&text)).await?;
                        claude_busy = true;
                    }
                }
                Some(S2C::SendInputWithFiles { text, files, message_ref_opaque_id, .. }) => {
                    idle_sleep.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                    if claude_busy {
                        info!("session {session_id}: Claude busy — queuing message with files ({} already queued)", queue.len());
                        queue.push_back(PendingMessage::WithFiles { text, files, msg_ref_opaque_id: message_ref_opaque_id });
                    } else {
                        let msg = format_user_message_with_files(&text, &files).await;
                        if !msg.is_empty() {
                            send_to_stdin(&mut stdin, session_id, msg).await?;
                            claude_busy = true;
                        }
                    }
                }
                Some(S2C::CancelQueuedInput { message_ref_opaque_id, .. }) => {
                    let before = queue.len();
                    queue.retain(|item| {
                        let id = match item {
                            PendingMessage::Text { msg_ref_opaque_id, .. } => msg_ref_opaque_id.as_deref(),
                            PendingMessage::WithFiles { msg_ref_opaque_id, .. } => msg_ref_opaque_id.as_deref(),
                        };
                        id != Some(message_ref_opaque_id.as_str())
                    });
                    let removed = before - queue.len();
                    info!("session {session_id}: CancelQueuedInput removed {removed} item(s) matching {message_ref_opaque_id:?}");
                }
                Some(S2C::KillSession { .. }) => {
                    info!("session {session_id}: KillSession — clearing queue ({} pending)", queue.len());
                    queue.clear();
                    runner.kill(&mut child, &config).await;
                    break;
                }
                Some(S2C::InterruptSession { .. }) => {
                    // Discard all queued messages and interrupt the current response.
                    // Claude returns to idle and the user can send a fresh message.
                    let dropped = queue.len();
                    queue.clear();
                    info!("session {session_id}: InterruptSession — dropped {dropped} queued message(s), sending SIGINT");
                    runner.interrupt(&mut child, &config).await;
                    // Mark as idle immediately — after SIGINT Claude returns to prompt.
                    claude_busy = false;
                }
                Some(_) | None => break,
            },

            line_result = stdout_lines.next_line() => match line_result {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    log_ndjson_in(session_id, &line);
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(event) => {
                            // A "result" event means Claude finished a turn and is idle.
                            // Deliver the next queued message if any.
                            if event.get("type").and_then(|t| t.as_str()) == Some("result") {
                                claude_busy = false;
                                if let Some(next) = queue.pop_front() {
                                    info!("session {session_id}: turn complete — delivering queued message ({} remaining)", queue.len());
                                    let msg = match next {
                                        PendingMessage::Text { text, .. } => format_user_message(&text),
                                        PendingMessage::WithFiles { text, files, .. } => {
                                            format_user_message_with_files(&text, &files).await
                                        }
                                    };
                                    if !msg.is_empty() {
                                        send_to_stdin(&mut stdin, session_id, msg).await?;
                                        claude_busy = true;
                                    }
                                } else {
                                    // Queue is empty — signal the server that Claude is idle.
                                    ws_send(ws_tx, &C2S::ClaudeIdle {
                                        session_id: session_id.clone(),
                                    }).await;
                                }
                            }

                            update_stats(&mut stats, &event);
                            ws_send(ws_tx, &C2S::SessionEvent {
                                session_id: session_id.clone(),
                                event,
                            })
                            .await;
                        }
                        Err(e) => warn!("session {session_id}: JSON parse error: {e}\nline: {line}"),
                    }
                }
                Ok(None) => {
                    info!("session {session_id}: stdout EOF");
                    break;
                }
                Err(e) => {
                    warn!("session {session_id}: stdout read error: {e}");
                    break;
                }
            },

            _ = &mut idle_sleep => {
                warn!("session {session_id}: idle for 12h — killing");
                runner.kill(&mut child, &config).await;
                break;
            }
        }
    }

    let exit_code = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(s)) => s.code().unwrap_or(-1),
        _ => {
            let _ = child.kill().await;
            -1
        }
    };

    // Brief yield so the stderr task can flush any remaining lines.
    tokio::task::yield_now().await;
    let stderr_content = stderr_buf.lock().await.clone();

    let error = if exit_code != 0 {
        Some(if stderr_content.is_empty() {
            format!("claude exited with code {exit_code}")
        } else {
            format!("claude exited with code {exit_code}:\n{stderr_content}")
        })
    } else {
        None
    };

    ws_send(
        ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
            stats,
            error,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Stdin write helper
// ---------------------------------------------------------------------------

async fn send_to_stdin(
    stdin: &mut ChildStdin,
    session_id: &str,
    msg: String,
) -> anyhow::Result<()> {
    log_ndjson_out(session_id, &msg);
    stdin
        .write_all(msg.as_bytes())
        .await
        .with_context(|| format!("writing to claude stdin for session {session_id}"))
}

// ---------------------------------------------------------------------------
// NDJSON trace logging
// ---------------------------------------------------------------------------

/// Log an NDJSON line being written to Claude's stdin.
fn log_ndjson_out(session_id: &str, line: &str) {
    let label = ndjson_label(line);
    tracing::debug!(target: "ndjson", "→ [{session_id}] {label}");
}

/// Log an NDJSON line received from Claude's stdout.
fn log_ndjson_in(session_id: &str, line: &str) {
    let label = ndjson_label(line);
    tracing::debug!(target: "ndjson", "← [{session_id}] {label}");
}

/// Extract a short human-readable label from an NDJSON line.
fn ndjson_label(line: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => {
            let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("?");
            match msg_type {
                "user" => {
                    let content = v
                        .pointer("/message/content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("…");
                    let preview: String = content.chars().take(80).collect();
                    let ellipsis = if content.len() > 80 { "…" } else { "" };
                    format!("user: \"{preview}{ellipsis}\"")
                }
                "assistant" => {
                    let text = v
                        .pointer("/message/content/0/text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    let preview: String = text.chars().take(60).collect();
                    let ellipsis = if text.len() > 60 { "…" } else { "" };
                    format!("assistant: \"{preview}{ellipsis}\"")
                }
                "result" => {
                    let cost = v.get("total_cost_usd").and_then(|c| c.as_f64());
                    let turns = v.get("num_turns").and_then(|t| t.as_u64());
                    match (turns, cost) {
                        (Some(t), Some(c)) => format!("result: {t} turns, ${c:.4}"),
                        (Some(t), None) => format!("result: {t} turns"),
                        _ => "result".to_string(),
                    }
                }
                "tool_use" => {
                    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    format!("tool_use: {name}")
                }
                "tool_result" => {
                    let id = v.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("?");
                    format!("tool_result: {id}")
                }
                other => other.to_string(),
            }
        }
        Err(_) => {
            let preview: String = line.chars().take(80).collect();
            format!("(unparsed) {preview}")
        }
    }
}

// ---------------------------------------------------------------------------
// Message formatting helpers
// ---------------------------------------------------------------------------

pub async fn format_user_message_with_files(text: &str, files: &[AttachedFile]) -> String {
    let mut blocks: Vec<serde_json::Value> = Vec::new();
    let mut saved_paths: Vec<String> = Vec::new();

    for file in files {
        if file.mime_type.starts_with("image/") {
            let media_type = match file.mime_type.as_str() {
                "image/jpeg" | "image/png" | "image/gif" | "image/webp" => file.mime_type.clone(),
                _ => "image/jpeg".to_string(),
            };
            blocks.push(serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": file.data_base64 }
            }));
        } else if file.mime_type == "application/pdf" {
            blocks.push(serde_json::json!({
                "type": "document",
                "source": { "type": "base64", "media_type": "application/pdf", "data": file.data_base64 }
            }));
        } else {
            let save_dir = std::path::Path::new("/tmp/telegram_attachments");
            if let Err(e) = tokio::fs::create_dir_all(save_dir).await {
                warn!("format_user_message_with_files: failed to create dir: {e}");
                continue;
            }
            let path = save_dir.join(&file.filename);
            match STANDARD.decode(&file.data_base64) {
                Ok(bytes) => {
                    if let Err(e) = tokio::fs::write(&path, &bytes).await {
                        warn!("failed to write {}: {e}", file.filename);
                    } else {
                        saved_paths.push(path.display().to_string());
                    }
                }
                Err(e) => warn!("base64 decode error for {}: {e}", file.filename),
            }
        }
    }

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

    if blocks.len() == 1 {
        if let Some(t) = blocks[0].get("text").and_then(|v| v.as_str()) {
            return format_user_message(t);
        }
    }

    let content_json =
        serde_json::to_string(&blocks).unwrap_or_else(|_| r#""[serialisation error]""#.to_string());
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}

fn format_user_message(text: &str) -> String {
    let content_json = serde_json::to_string(text).unwrap_or_else(|_| format!("{text:?}"));
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}

// ---------------------------------------------------------------------------
// Stats accumulation
// ---------------------------------------------------------------------------

pub fn update_stats(stats: &mut SessionStats, event: &serde_json::Value) {
    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

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
            if let Some(cost) = event.get("total_cost_usd").and_then(|c| c.as_f64()) {
                stats.cost_usd = Some(cost);
            }
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

// ---------------------------------------------------------------------------
// WebSocket send helper
// ---------------------------------------------------------------------------

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
        Err(e) => warn!("failed to serialise C2S message: {e}"),
    }
}
