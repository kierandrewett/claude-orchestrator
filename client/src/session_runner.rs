use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::SinkExt;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

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
pub async fn run_session(
    config: SessionConfig,
    ws_tx: Arc<Mutex<WsSink>>,
    mut cmd_rx: mpsc::Receiver<S2C>,
) {
    let session_id = config.session_id.clone();
    info!(
        "session {session_id}: starting in cwd={}",
        config.default_cwd
    );

    // -------------------------------------------------------------------------
    // 1. Build and spawn the Claude child process
    // -------------------------------------------------------------------------
    let mut cmd = Command::new(&config.claude_path);
    cmd.args([
        "--print",
        "--verbose",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--dangerously-skip-permissions",
    ]);

    if config.is_resume {
        cmd.args(["--resume", &config.claude_session_id]);
    } else {
        cmd.args(["--session-id", &config.claude_session_id]);
    }

    for arg in &config.extra_args {
        cmd.arg(arg);
    }
    cmd.current_dir(&config.default_cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("session {session_id}: failed to spawn claude: {e}");
            ws_send(
                &ws_tx,
                &C2S::SessionEnded {
                    session_id: session_id.clone(),
                    exit_code: -1,
                    stats: SessionStats::default(),
                },
            )
            .await;
            return;
        }
    };

    // -------------------------------------------------------------------------
    // 2. Extract pid and I/O handles
    // -------------------------------------------------------------------------
    let pid = match child.id() {
        Some(p) => p,
        None => {
            warn!("session {session_id}: could not get child PID");
            0
        }
    };

    let stdin_pipe = child.stdin.take().expect("stdin was piped");
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // -------------------------------------------------------------------------
    // 3. Report SessionStarted
    // -------------------------------------------------------------------------
    ws_send(
        &ws_tx,
        &C2S::SessionStarted {
            session_id: session_id.clone(),
            pid,
            cwd: config.default_cwd.clone(),
        },
    )
    .await;

    // -------------------------------------------------------------------------
    // 4. Stdin task
    //    The main loop forwards text to this channel; the task writes it to Claude.
    // -------------------------------------------------------------------------
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(32);

    let initial_prompt = config.initial_prompt.clone();
    let session_id_stdin = session_id.clone();
    tokio::spawn(async move {
        let mut stdin = stdin_pipe;

        // Send the initial prompt first, if any
        if let Some(prompt) = initial_prompt {
            let line = format_user_message(&prompt);
            debug!("session {session_id_stdin}: writing initial prompt to stdin");
            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                warn!("session {session_id_stdin}: stdin write (initial prompt) error: {e}");
                return;
            }
            if let Err(e) = stdin.flush().await {
                warn!("session {session_id_stdin}: stdin flush error: {e}");
                return;
            }
        }

        // Forward subsequent messages from the channel
        while let Some(text) = stdin_rx.recv().await {
            let line = format_user_message(&text);
            debug!("session {session_id_stdin}: writing user input to stdin");
            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                warn!("session {session_id_stdin}: stdin write error: {e}");
                break;
            }
            if let Err(e) = stdin.flush().await {
                warn!("session {session_id_stdin}: stdin flush error: {e}");
                break;
            }
        }
        debug!("session {session_id_stdin}: stdin task exiting");
    });

    // -------------------------------------------------------------------------
    // 5. Stderr task: log lines at debug level
    // -------------------------------------------------------------------------
    let session_id_stderr = session_id.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr_pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            warn!("session {session_id_stderr} [stderr]: {line}");
        }
    });

    // -------------------------------------------------------------------------
    // 6. Stats accumulator
    // -------------------------------------------------------------------------
    let mut stats = SessionStats::default();

    // -------------------------------------------------------------------------
    // 7. Main select! loop: stdout lines vs incoming commands
    // -------------------------------------------------------------------------
    let mut stdout_lines = BufReader::new(stdout_pipe).lines();

    loop {
        tokio::select! {
            biased;

            // Incoming command from server
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(S2C::SendInput { text, .. }) => {
                        debug!("session {session_id}: received SendInput");
                        if let Err(e) = stdin_tx.send(text).await {
                            warn!("session {session_id}: could not forward input to stdin task: {e}");
                        }
                    }
                    Some(S2C::SendInputWithFiles { text, files, .. }) => {
                        debug!("session {session_id}: received SendInputWithFiles ({} file(s))", files.len());
                        let formatted = format_user_message_with_files(&text, &files).await;
                        if !formatted.is_empty() {
                            if let Err(e) = stdin_tx.send(formatted).await {
                                warn!("session {session_id}: could not forward files to stdin task: {e}");
                            }
                        }
                    }
                    Some(S2C::KillSession { .. }) => {
                        info!("session {session_id}: KillSession received, killing child");
                        if let Err(e) = child.kill().await {
                            warn!("session {session_id}: kill error: {e}");
                        }
                        break;
                    }
                    Some(other) => {
                        warn!("session {session_id}: unexpected command: {other:?}");
                    }
                    None => {
                        // Channel closed — connection was dropped; kill child
                        info!("session {session_id}: command channel closed, killing child");
                        let _ = child.kill().await;
                        break;
                    }
                }
            }

            // Output line from Claude
            line_result = stdout_lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(event) => {
                                update_stats(&mut stats, &event);
                                ws_send(
                                    &ws_tx,
                                    &C2S::SessionEvent {
                                        session_id: session_id.clone(),
                                        event,
                                    },
                                )
                                .await;
                            }
                            Err(e) => {
                                warn!("session {session_id}: failed to parse stdout line as JSON: {e}\nline: {line}");
                                // Forward raw as a text event so the server still sees it
                                let raw_event = serde_json::json!({
                                    "type": "raw_text",
                                    "text": line,
                                });
                                ws_send(
                                    &ws_tx,
                                    &C2S::SessionEvent {
                                        session_id: session_id.clone(),
                                        event: raw_event,
                                    },
                                )
                                .await;
                            }
                        }
                    }
                    Ok(None) => {
                        // stdout EOF — Claude has exited
                        info!("session {session_id}: stdout closed");
                        break;
                    }
                    Err(e) => {
                        warn!("session {session_id}: stdout read error: {e}");
                        break;
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // 8. Wait for the child to fully exit and collect the exit code
    // -------------------------------------------------------------------------
    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            warn!("session {session_id}: wait() error: {e}");
            -1
        }
    };

    info!("session {session_id}: exited with code {exit_code}");

    // -------------------------------------------------------------------------
    // 9. Send SessionEnded
    // -------------------------------------------------------------------------
    ws_send(
        &ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
            stats,
        },
    )
    .await;
}

// -----------------------------------------------------------------------------
// Helper: format a user message with attached files for Claude's stream-json
// stdin format. Images and PDFs are embedded as base64 content blocks;
// other file types are saved to /tmp/telegram_attachments/ and referenced
// by path so Claude can read them with its Read tool.
// -----------------------------------------------------------------------------
async fn format_user_message_with_files(text: &str, files: &[AttachedFile]) -> String {
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
fn update_stats(stats: &mut SessionStats, event: &serde_json::Value) {
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
async fn ws_send(ws_tx: &Mutex<WsSink>, msg: &C2S) {
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
