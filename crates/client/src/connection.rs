use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
    // Ensure the URL always ends with the /ws/client path.
    let server_url = {
        let base = config.server_url.trim_end_matches('/');
        if base.ends_with("/ws/client") {
            base.to_string()
        } else {
            format!("{base}/ws/client")
        }
    };

    info!("connecting to {server_url}");

    // Build an HTTP upgrade request with the required WebSocket headers.
    // tungstenite does not inject these automatically when given a custom Request.
    let uri: tokio_tungstenite::tungstenite::http::Uri =
        server_url.parse().context("invalid SERVER_URL")?;
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
            ref system_prompt,
            ref initial_files,
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
                map.insert(sid.clone(), cmd_tx.clone());
            }

            // Pre-queue the initial prompt as a SendInput/SendInputWithFiles so it's
            // delivered through the normal select loop.
            if let Some(ref text) = initial_prompt {
                if initial_files.is_empty() {
                    let _ = cmd_tx.try_send(S2C::SendInput {
                        session_id: sid.clone(),
                        text: text.clone(),
                        message_ref_opaque_id: None,
                    });
                } else {
                    let _ = cmd_tx.try_send(S2C::SendInputWithFiles {
                        session_id: sid.clone(),
                        text: text.clone(),
                        files: initial_files.clone(),
                        message_ref_opaque_id: None,
                    });
                }
            }

            // Increment active session count
            {
                let mut state = tray_state.lock().unwrap();
                state.active_sessions += 1;
            }

            let session_cfg = SessionConfig {
                session_id: sid.clone(),
                initial_prompt: None, // delivered via cmd channel above
                initial_files: vec![],
                extra_args: extra_args.clone(),
                claude_session_id: claude_session_id.clone(),
                is_resume,
                default_cwd: config.default_cwd.clone(),
                system_prompt: system_prompt.clone(),
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
            ..
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

        S2C::InterruptSession { ref session_id } => {
            info!("InterruptSession: session_id={session_id}");
            route_to_session(session_map, session_id, msg.clone()).await;
        }

        S2C::CancelQueuedInput { ref session_id, .. } => {
            debug!("CancelQueuedInput: session_id={session_id}");
            route_to_session(session_map, session_id, msg.clone()).await;
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


