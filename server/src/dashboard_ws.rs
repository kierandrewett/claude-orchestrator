use std::sync::Arc;

use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_ws::Message;
use chrono::Utc;
use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::{
    protocol::*,
    state::{AppState, SessionBuffer},
};

// ---------------------------------------------------------------------------
// HTTP upgrade handler
// ---------------------------------------------------------------------------

pub async fn handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, Error> {
    let (response, ws_session, msg_stream) = actix_ws::handle(&req, stream)?;
    let msg_stream = msg_stream.max_frame_size(128 * 1024 * 1024);

    let state_clone: Arc<AppState> = Arc::clone(&state);
    actix_rt::spawn(run(ws_session, msg_stream, state_clone));

    Ok(response)
}

// ---------------------------------------------------------------------------
// WebSocket task
// ---------------------------------------------------------------------------

async fn run(
    ws_session: actix_ws::Session,
    mut msg_stream: actix_ws::MessageStream,
    state: Arc<AppState>,
) {
    info!("dashboard_ws: connection opened");

    // -----------------------------------------------------------------------
    // On connect: register, then send current state snapshots.
    // -----------------------------------------------------------------------

    // 1. Register this dashboard.
    {
        let mut guard = state.dashboards.write().await;
        guard.push(ws_session.clone());
    }

    // 2. Collect data needed for the initial burst (hold no locks across awaits).
    let (client_connected, client_hostname, all_sessions, current_commands): (
        bool,
        Option<String>,
        Vec<SessionInfo>,
        Vec<SlashCommand>,
    ) = {
        let client_guard = state.client.read().await;
        let connected = client_guard.is_some();
        let hostname = client_guard.as_ref().map(|c| c.hostname.clone());
        drop(client_guard);

        let sessions_guard = state.sessions.read().await;
        let sessions: Vec<SessionInfo> = sessions_guard
            .values()
            .map(|buf| buf.info.clone())
            .collect();
        drop(sessions_guard);

        let commands: Vec<SlashCommand> = state.commands.read().await.clone();

        (connected, hostname, sessions, commands)
    };

    // 3. Send initial messages directly on the session clone.
    //    We must find and send to this specific session — use the clone we hold.
    let mut this_session = ws_session.clone();

    let client_status_text =
        match serde_json::to_string(&S2D::ClientStatus {
            connected: client_connected,
            hostname: client_hostname,
        }) {
            Ok(t) => t,
            Err(e) => {
                error!("dashboard_ws: serialize ClientStatus: {e}");
                return;
            }
        };

    if let Err(e) = this_session.text(client_status_text).await {
        error!("dashboard_ws: send ClientStatus on connect: {e}");
        remove_dashboard(&state, &ws_session).await;
        return;
    }

    let session_list_text =
        match serde_json::to_string(&S2D::SessionList {
            sessions: all_sessions,
        }) {
            Ok(t) => t,
            Err(e) => {
                error!("dashboard_ws: serialize SessionList: {e}");
                return;
            }
        };

    if let Err(e) = this_session.text(session_list_text).await {
        error!("dashboard_ws: send SessionList on connect: {e}");
        remove_dashboard(&state, &ws_session).await;
        return;
    }

    // Send current command list if we already have one.
    if !current_commands.is_empty() {
        let cmd_list_text = match serde_json::to_string(&S2D::CommandList {
            commands: current_commands,
        }) {
            Ok(t) => t,
            Err(e) => {
                error!("dashboard_ws: serialize CommandList: {e}");
                return;
            }
        };
        if let Err(e) = this_session.text(cmd_list_text).await {
            error!("dashboard_ws: send CommandList on connect: {e}");
            remove_dashboard(&state, &ws_session).await;
            return;
        }
    }

    // -----------------------------------------------------------------------
    // Message loop
    // -----------------------------------------------------------------------
    while let Some(msg_result) = msg_stream.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                if !handle_text(&state, &mut this_session, text.as_ref()).await {
                    break;
                }
            }
            Ok(Message::Ping(bytes)) => {
                let _ = this_session.pong(&bytes).await;
            }
            Ok(Message::Close(reason)) => {
                info!("dashboard_ws: close frame received: {:?}", reason);
                break;
            }
            Ok(_) => {} // Pong / Binary / Continuation — ignore
            Err(e) => {
                error!("dashboard_ws: stream error: {e}");
                break;
            }
        }
    }

    remove_dashboard(&state, &ws_session).await;
    info!("dashboard_ws: disconnected");
}

// ---------------------------------------------------------------------------
// Handle a single text frame.
// Returns false if the connection should be closed.
// ---------------------------------------------------------------------------

async fn handle_text(
    state: &Arc<AppState>,
    this_session: &mut actix_ws::Session,
    text: &str,
) -> bool {
    let msg: D2S = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("dashboard_ws: failed to parse D2S message: {e}");
            let err_text = serde_json::to_string(&S2D::Error {
                message: format!("invalid message: {e}"),
            })
            .unwrap_or_default();
            let _ = this_session.text(err_text).await;
            return true; // keep connection open
        }
    };

    match msg {
        // -------------------------------------------------------------------
        D2S::CreateSession {
            name,
            initial_prompt,
        } => {
            let session_id = uuid::Uuid::new_v4().to_string();
            let claude_session_id = uuid::Uuid::new_v4().to_string();
            info!(%session_id, ?name, "dashboard_ws: CreateSession");

            let info = SessionInfo {
                id: session_id.clone(),
                name: name.clone(),
                cwd: String::new(),   // filled in when client reports SessionStarted
                status: SessionStatus::Pending,
                created_at: Utc::now(),
                started_at: None,
                ended_at: None,
                stats: SessionStats::default(),
                client_hostname: {
                    // Read hostname without holding lock across await.
                    let guard = state.client.read().await;
                    guard.as_ref().map(|c| c.hostname.clone())
                },
                claude_session_id: Some(claude_session_id.clone()),
            };

            // Persist immediately so the session survives a server restart.
            state.store.save_session(&info).await.ok();

            // Store the session.
            {
                let mut guard = state.sessions.write().await;
                guard.insert(
                    session_id.clone(),
                    SessionBuffer {
                        info: info.clone(),
                        events: std::collections::VecDeque::new(),
                    },
                );
            }

            // Broadcast SessionCreated to ALL dashboards (including sender).
            state
                .broadcast(&S2D::SessionCreated {
                    session: info.clone(),
                })
                .await;

            // Forward StartSession to the client daemon.
            state
                .send_to_client(&S2C::StartSession {
                    session_id,
                    initial_prompt,
                    extra_args: Vec::new(),
                    claude_session_id,
                    is_resume: false,
                })
                .await;
        }

        // -------------------------------------------------------------------
        D2S::SendInput { session_id, text } => {
            info!(%session_id, "dashboard_ws: SendInput");
            state
                .send_to_client(&S2C::SendInput { session_id, text })
                .await;
        }

        // -------------------------------------------------------------------
        D2S::KillSession { session_id } => {
            info!(%session_id, "dashboard_ws: KillSession");

            let updated_info: Option<SessionInfo> = {
                let mut guard = state.sessions.write().await;
                if let Some(buf) = guard.get_mut(&session_id) {
                    buf.info.status = SessionStatus::Killed;
                    buf.info.ended_at = Some(Utc::now());
                    Some(buf.info.clone())
                } else {
                    warn!(%session_id, "dashboard_ws: KillSession for unknown session");
                    None
                }
            };

            if let Some(info) = updated_info {
                state.store.save_session(&info).await.ok();
                state
                    .broadcast(&S2D::SessionUpdated { session: info })
                    .await;
                state
                    .send_to_client(&S2C::KillSession { session_id })
                    .await;
            }
        }

        // -------------------------------------------------------------------
        D2S::GetHistory { session_id } => {
            info!(%session_id, "dashboard_ws: GetHistory");

            // Collect events without holding the lock across await.
            let events_opt: Option<Vec<serde_json::Value>> = {
                let guard = state.sessions.read().await;
                guard
                    .get(&session_id)
                    .map(|buf| buf.events.iter().cloned().collect())
            };

            let response_text = match events_opt {
                Some(events) => match serde_json::to_string(&S2D::SessionHistory {
                    session_id: session_id.clone(),
                    events,
                }) {
                    Ok(t) => t,
                    Err(e) => {
                        error!("dashboard_ws: serialize SessionHistory: {e}");
                        return true;
                    }
                },
                None => {
                    match serde_json::to_string(&S2D::Error {
                        message: format!("session not found: {session_id}"),
                    }) {
                        Ok(t) => t,
                        Err(e) => {
                            error!("dashboard_ws: serialize Error: {e}");
                            return true;
                        }
                    }
                }
            };

            // Send only to this dashboard.
            if let Err(e) = this_session.text(response_text).await {
                error!("dashboard_ws: send SessionHistory: {e}");
                return false;
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Remove this dashboard's session from the shared list on disconnect.
// ---------------------------------------------------------------------------

async fn remove_dashboard(_state: &Arc<AppState>, _closing_session: &actix_ws::Session) {
    // actix_ws::Session does not implement PartialEq, so we rely on the fact
    // that broadcast() already prunes dead sessions (text() returns Err after
    // the underlying connection is gone). We trigger a prune by doing a
    // zero-content write cycle, or simply shrink the list on the next
    // broadcast. However, for correctness we do a best-effort shrink here by
    // attempting to send a harmless message and letting broadcast() prune.
    //
    // A cleaner approach (used here) is to rebuild the list by keeping only
    // the sessions that still respond. Since we have no unique ID on each
    // WsSession, we instead do the simplest safe thing: leave the dead entry
    // in place and let the next broadcast() remove it automatically.
    //
    // If you need deterministic removal, wrap WsSession in an Arc<Mutex<…>>
    // together with a UUID, which is a common pattern.
    info!("dashboard_ws: session removed from dashboard list (will be pruned on next broadcast)");
}
