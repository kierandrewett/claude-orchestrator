use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, Mutex as TokioMutex};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::protocol::{C2S, S2C};
use crate::session_runner::{self, SessionConfig, WsSink};
use crate::tray::TrayState;

pub struct Config {
    pub server_url: String,
    pub client_token: String,
    pub client_id: String,
    pub hostname: String,
    pub claude_path: String,
    pub default_cwd: String,
}

/// Map of live session_id → channel sender for routing commands to session tasks.
type SessionMap = Arc<TokioMutex<HashMap<String, mpsc::Sender<S2C>>>>;

/// Outer reconnect loop. Runs until a clean shutdown or the shutdown receiver fires.
pub async fn run_forever(
    config: Arc<Config>,
    tray_state: Arc<Mutex<TrayState>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut backoff = 1u64;

    loop {
        tokio::select! {
            biased;

            _ = shutdown.recv() => {
                info!("shutdown signal received in run_forever");
                break;
            }

            result = connect_and_run(&config, Arc::clone(&tray_state)) => {
                // Mark disconnected whenever the connection drops
                {
                    let mut state = tray_state.lock().unwrap();
                    state.connected = false;
                    state.hostname = None;
                    state.active_sessions = 0;
                }

                match result {
                    Ok(()) => {
                        info!("connection closed cleanly, not reconnecting");
                        break;
                    }
                    Err(e) => {
                        warn!("connection error: {e:#}, reconnecting in {backoff}s");
                    }
                }
            }
        }

        // Wait backoff seconds, but still respect shutdown
        tokio::select! {
            biased;
            _ = shutdown.recv() => {
                info!("shutdown signal during backoff");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(backoff)) => {}
        }

        backoff = (backoff * 2).min(10);
    }
}

/// Connects to the server, sends Hello, then drives the read loop.
async fn connect_and_run(
    config: &Arc<Config>,
    tray_state: Arc<Mutex<TrayState>>,
) -> anyhow::Result<()> {
    info!("connecting to {}", config.server_url);

    // Build an HTTP upgrade request with the required WebSocket headers.
    // tungstenite does not inject these automatically when given a custom Request.
    let uri: tokio_tungstenite::tungstenite::http::Uri =
        config.server_url.parse().context("invalid SERVER_URL")?;
    let host = uri.host().unwrap_or("localhost").to_string();
    let host = match uri.port_u16() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    };
    let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(uri)
        .header("Host", host)
        .header("Authorization", format!("Bearer {}", config.client_token))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", key)
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .context("failed to build HTTP upgrade request")?;

    let (ws_stream, response) = tokio_tungstenite::connect_async(request)
        .await
        .context("WebSocket connect failed")?;

    info!("connected (HTTP {})", response.status());

    // Mark connected
    {
        let mut state = tray_state.lock().unwrap();
        state.connected = true;
        state.hostname = Some(config.hostname.clone());
    }

    let (write, mut read) = ws_stream.split();
    let ws_tx: Arc<TokioMutex<WsSink>> = Arc::new(TokioMutex::new(write));

    // Send Hello immediately
    send_msg(
        &ws_tx,
        &C2S::Hello {
            client_id: config.client_id.clone(),
            hostname: config.hostname.clone(),
        },
    )
    .await;

    // Import any historical Claude Code sessions in the background.
    {
        let config_clone = Arc::clone(config);
        let ws_tx_clone = Arc::clone(&ws_tx);
        tokio::spawn(async move {
            crate::history_importer::run(config_clone, ws_tx_clone).await;
        });
    }

    let session_map: SessionMap = Arc::new(TokioMutex::new(HashMap::new()));

    // Read loop
    while let Some(raw) = read.next().await {
        let msg = match raw {
            Ok(m) => m,
            Err(e) => {
                return Err(anyhow::anyhow!("WebSocket read error: {e}"));
            }
        };

        match msg {
            Message::Text(text) => {
                handle_text_message(
                    text.as_str(),
                    config,
                    &ws_tx,
                    &session_map,
                    Arc::clone(&tray_state),
                )
                .await;
            }

            Message::Binary(bin) => match std::str::from_utf8(&bin) {
                Ok(text) => {
                    handle_text_message(
                        text,
                        config,
                        &ws_tx,
                        &session_map,
                        Arc::clone(&tray_state),
                    )
                    .await;
                }
                Err(_) => {
                    warn!("received unexpected binary message, ignoring");
                }
            },

            Message::Ping(payload) => {
                debug!("received ping, sending pong");
                let mut sink = ws_tx.lock().await;
                if let Err(e) = sink.send(Message::Pong(payload)).await {
                    return Err(anyhow::anyhow!("failed to send pong: {e}"));
                }
            }

            Message::Pong(_) => {
                debug!("received pong");
            }

            Message::Close(frame) => {
                info!("server sent close frame: {:?}", frame);
                let mut sink = ws_tx.lock().await;
                let _ = sink.send(Message::Close(None)).await;
                return Ok(());
            }

            Message::Frame(_) => {
                // Raw frames are not expected at this level
            }
        }
    }

    // read stream ended
    Err(anyhow::anyhow!("WebSocket stream ended unexpectedly"))
}

/// Parses a text message as S2C and dispatches it.
async fn handle_text_message(
    text: &str,
    config: &Arc<Config>,
    ws_tx: &Arc<TokioMutex<WsSink>>,
    session_map: &SessionMap,
    tray_state: Arc<Mutex<TrayState>>,
) {
    let msg: S2C = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("failed to parse S2C message: {e}\nraw: {text}");
            return;
        }
    };

    match msg {
        S2C::StartSession {
            ref session_id,
            ref initial_prompt,
            ref extra_args,
            ref claude_session_id,
            is_resume,
        } => {
            let sid = session_id.clone();
            info!("StartSession: session_id={sid}, is_resume={is_resume}");

            let (cmd_tx, cmd_rx) = mpsc::channel::<S2C>(32);

            {
                let mut map = session_map.lock().await;
                if map.contains_key(&sid) {
                    warn!("session {sid} already exists, ignoring StartSession");
                    return;
                }
                map.insert(sid.clone(), cmd_tx);
            }

            // Increment active session count
            {
                let mut state = tray_state.lock().unwrap();
                state.active_sessions += 1;
            }

            let session_cfg = SessionConfig {
                session_id: sid.clone(),
                initial_prompt: initial_prompt.clone(),
                extra_args: extra_args.clone(),
                claude_path: config.claude_path.clone(),
                claude_session_id: claude_session_id.clone(),
                is_resume,
                default_cwd: config.default_cwd.clone(),
            };

            let ws_tx_clone = Arc::clone(ws_tx);
            let session_map_clone = Arc::clone(session_map);
            let tray_state_clone = Arc::clone(&tray_state);

            tokio::spawn(async move {
                session_runner::run_session(session_cfg, ws_tx_clone, cmd_rx).await;

                // Clean up session from map when the task ends
                {
                    let mut map = session_map_clone.lock().await;
                    map.remove(&sid);
                }

                // Decrement active session count
                {
                    let mut state = tray_state_clone.lock().unwrap();
                    state.active_sessions = state.active_sessions.saturating_sub(1);
                }

                info!("session {sid} removed from session map");
            });
        }

        S2C::SendInput {
            ref session_id,
            ref text,
        } => {
            debug!("SendInput: session_id={session_id}");
            route_to_session(session_map, session_id, msg.clone()).await;
            let _ = text;
        }

        S2C::SendInputWithFiles {
            ref session_id,
            ref files,
            ..
        } => {
            debug!("SendInputWithFiles: session_id={session_id}, files={}", files.len());
            route_to_session(session_map, session_id, msg.clone()).await;
        }

        S2C::KillSession { ref session_id } => {
            info!("KillSession: session_id={session_id}");
            route_to_session(session_map, session_id, msg.clone()).await;
        }

        S2C::GetVmConfig { request_id } => {
            let ws_tx_clone = Arc::clone(ws_tx);
            tokio::spawn(async move {
                let cfg = match crate::vm::config::VmConfig::load() {
                    Ok(Some(cfg)) => cfg,
                    // No file yet — return auto-detected defaults so the server
                    // can show them to the user before they've run /vminit.
                    Ok(None) => crate::vm::config::VmConfig::detect_defaults(),
                    Err(e) => {
                        ws_send(
                            &ws_tx_clone,
                            &C2S::VmConfigAck {
                                request_id,
                                success: false,
                                error: Some(e.to_string()),
                            },
                        )
                        .await;
                        return;
                    }
                };
                ws_send(
                    &ws_tx_clone,
                    &C2S::VmConfig {
                        request_id,
                        config: cfg.into(),
                    },
                )
                .await;
            });
        }

        S2C::SetVmConfig { request_id, config } => {
            let ws_tx_clone = Arc::clone(ws_tx);
            tokio::spawn(async move {
                let vm_cfg: crate::vm::config::VmConfig = config.into();
                let result = vm_cfg.save();
                ws_send(
                    &ws_tx_clone,
                    &C2S::VmConfigAck {
                        request_id,
                        success: result.is_ok(),
                        error: result.err().map(|e| e.to_string()),
                    },
                )
                .await;
            });
        }

        S2C::BuildImage { request_id } => {
            let ws_tx_clone = Arc::clone(ws_tx);
            tokio::spawn(async move {
                let cfg = match crate::vm::config::VmConfig::load() {
                    Ok(Some(c)) => c,
                    Ok(None) => crate::vm::config::VmConfig::detect_defaults(),
                    Err(e) => {
                        ws_send(
                            &ws_tx_clone,
                            &C2S::BuildImageResult {
                                request_id,
                                success: false,
                                error: Some(format!("failed to load vm config: {e}")),
                            },
                        )
                        .await;
                        return;
                    }
                };

                let dockerfile = crate::vm::config::VmConfig::dockerfile_path();

                // Write (or overwrite) the Dockerfile from the current config.
                if let Err(e) = crate::vm::rootfs::write_dockerfile(
                    &cfg.base_image,
                    &cfg.tools.extra_packages,
                    &dockerfile,
                )
                .await
                {
                    ws_send(
                        &ws_tx_clone,
                        &C2S::BuildImageResult {
                            request_id,
                            success: false,
                            error: Some(format!("failed to write Dockerfile: {e}")),
                        },
                    )
                    .await;
                    return;
                }

                // Stream log lines back to the server as they arrive.
                let (log_tx, mut log_rx) =
                    tokio::sync::mpsc::unbounded_channel::<String>();
                let ws_for_logs = Arc::clone(&ws_tx_clone);
                let req_id_for_logs = request_id.clone();
                tokio::spawn(async move {
                    while let Some(line) = log_rx.recv().await {
                        ws_send(
                            &ws_for_logs,
                            &C2S::BuildImageLog {
                                request_id: req_id_for_logs.clone(),
                                line,
                            },
                        )
                        .await;
                    }
                });

                let (success, error) =
                    match crate::vm::rootfs::build(&cfg.image, &dockerfile, log_tx).await {
                        Ok(()) => (true, None),
                        Err(e) => (false, Some(e.to_string())),
                    };

                ws_send(
                    &ws_tx_clone,
                    &C2S::BuildImageResult {
                        request_id,
                        success,
                        error,
                    },
                )
                .await;
            });
        }

        S2C::QueryCommands => {
            info!("QueryCommands: discovering slash commands");
            let claude_path = config.claude_path.clone();
            let ws_tx_clone = Arc::clone(ws_tx);
            tokio::spawn(async move {
                let commands = discover_commands(&claude_path).await;
                ws_send(&ws_tx_clone, &C2S::CommandList { commands }).await;
            });
        }
    }
}

/// Forwards a command to the session task that owns the given session_id.
async fn route_to_session(session_map: &SessionMap, session_id: &str, msg: S2C) {
    let map = session_map.lock().await;
    match map.get(session_id) {
        Some(tx) => {
            if let Err(e) = tx.send(msg).await {
                warn!("failed to forward message to session {session_id}: {e}");
            }
        }
        None => {
            warn!("no active session {session_id} for incoming command");
        }
    }
}

/// Serialises and sends a C2S message over the WebSocket, logging failures.
pub async fn send_msg(ws_tx: &TokioMutex<WsSink>, msg: &C2S) {
    match serde_json::to_string(msg) {
        Ok(json) => {
            let mut sink = ws_tx.lock().await;
            if let Err(e) = sink.send(Message::Text(json)).await {
                warn!("ws_send error: {e}");
            }
        }
        Err(e) => {
            warn!("failed to serialise C2S message: {e}");
        }
    }
}

/// Same as send_msg but takes an Arc<TokioMutex<WsSink>> for use in spawned tasks.
async fn ws_send(ws_tx: &Arc<TokioMutex<WsSink>>, msg: &C2S) {
    send_msg(ws_tx, msg).await;
}

/// Discovers available slash commands by spawning Claude Code with stream-json
/// and sending `/help`, then parsing the text response for slash command names
/// and descriptions.
async fn discover_commands(claude_path: &str) -> Vec<claude_shared::SlashCommand> {
    let mut cmd = tokio::process::Command::new(claude_path);
    cmd.args([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--dangerously-skip-permissions",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("discover_commands: failed to spawn claude: {e}");
            return vec![];
        }
    };

    let mut stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

    let help_msg = concat!(
        r#"{"type":"user","message":{"role":"user","content":"/help"}}"#,
        "\n"
    );
    if let Err(e) = stdin.write_all(help_msg.as_bytes()).await {
        warn!("discover_commands: stdin write error: {e}");
        return vec![];
    }
    drop(stdin); // signal EOF so claude knows there's no more input

    let mut lines = BufReader::new(stdout).lines();
    let mut full_text = String::new();

    let read_fut = async {
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match event.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "content_block_delta" => {
                    if let Some(text) = event.pointer("/delta/text").and_then(|t| t.as_str()) {
                        full_text.push_str(text);
                    }
                }
                "result" => {
                    if let Some(text) = event.get("result").and_then(|t| t.as_str()) {
                        if full_text.is_empty() {
                            full_text = text.to_string();
                        }
                    }
                    break;
                }
                _ => {}
            }
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(30), read_fut).await {
        Ok(()) => {}
        Err(_) => {
            warn!("discover_commands: timed out waiting for /help response");
            let _ = child.kill().await;
        }
    }
    let _ = child.wait().await;

    if full_text.is_empty() {
        return vec![];
    }

    parse_slash_commands(&full_text)
}

fn parse_slash_commands(text: &str) -> Vec<claude_shared::SlashCommand> {
    let mut commands = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('/') {
            continue;
        }

        // Try "name - desc" or "name: desc" patterns
        if let Some((name, desc)) = line.split_once(" - ").or_else(|| line.split_once(": ")) {
            let name = name.trim().to_string();
            let desc = desc.trim().to_string();
            if name.starts_with('/') && !name.contains(' ') {
                commands.push(claude_shared::SlashCommand {
                    name,
                    description: desc,
                });
            }
        } else {
            // Just the command name, no description
            let name = line.split_whitespace().next().unwrap_or("").to_string();
            if name.starts_with('/') {
                commands.push(claude_shared::SlashCommand {
                    name,
                    description: String::new(),
                });
            }
        }
    }
    commands
}
