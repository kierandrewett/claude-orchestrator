use std::sync::Arc;

use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_ws::Message;
use chrono::Utc;
use futures_util::StreamExt;
use tracing::{error, info, warn};

use crate::{
    protocol::*,
    state::{AppState, ClientHandle},
};

// ---------------------------------------------------------------------------
// HTTP upgrade handler
// ---------------------------------------------------------------------------

pub async fn handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, Error> {
    // --- Authentication ---
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let expected = format!("Bearer {}", state.client_token);
    if auth_header != expected {
        warn!("client_ws: rejected connection — bad token");
        return Ok(HttpResponse::Unauthorized().body("invalid token"));
    }

    let (response, ws_session, msg_stream) = actix_ws::handle(&req, stream)?;

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
    info!("client_ws: connection opened");

    // Register client with a placeholder hostname until Hello arrives.
    {
        let mut guard = state.client.write().await;
        *guard = Some(ClientHandle {
            id: uuid::Uuid::new_v4().to_string(),
            hostname: String::new(),
            session: ws_session,
        });
    }

    // Broadcast "connected" (hostname unknown yet).
    state
        .broadcast(&S2D::ClientStatus {
            connected: true,
            hostname: None,
        })
        .await;

    // Message loop.
    while let Some(msg_result) = msg_stream.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                handle_text(&state, text.as_ref()).await;
            }
            Ok(Message::Ping(bytes)) => {
                // Reply with pong — clone session to avoid holding the lock.
                let session_clone: Option<actix_ws::Session> = {
                    let guard = state.client.read().await;
                    guard.as_ref().map(|c| c.session.clone())
                };
                if let Some(mut ws) = session_clone {
                    let _ = ws.pong(&bytes).await;
                }
            }
            Ok(Message::Close(reason)) => {
                info!("client_ws: close frame received: {:?}", reason);
                break;
            }
            Ok(_) => {} // Pong / Binary / Continuation — ignore
            Err(e) => {
                error!("client_ws: stream error: {e}");
                break;
            }
        }
    }

    disconnect(&state).await;
}

// ---------------------------------------------------------------------------
// Handle a single text frame
// ---------------------------------------------------------------------------

async fn handle_text(state: &Arc<AppState>, text: &str) {
    let msg: C2S = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("client_ws: failed to parse C2S message: {e}");
            return;
        }
    };

    match msg {
        // -------------------------------------------------------------------
        C2S::Hello {
            client_id,
            hostname,
        } => {
            info!(%client_id, %hostname, "client_ws: Hello");

            // Update the stored handle with the real hostname.
            {
                let mut guard = state.client.write().await;
                if let Some(ref mut handle) = *guard {
                    handle.hostname = hostname.clone();
                }
            }

            state
                .broadcast(&S2D::ClientStatus {
                    connected: true,
                    hostname: Some(hostname.clone()),
                })
                .await;

            // Drain pending resumes and send a StartSession for each.
            let resumes = {
                let mut pending = state.pending_resumes.write().await;
                std::mem::take(&mut *pending)
            };

            for session_info in resumes {
                let claude_session_id = session_info
                    .claude_session_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                // The session was already set to Pending during load_from_disk;
                // persist the updated status so it survives a further restart.
                {
                    let sessions = state.sessions.read().await;
                    if let Some(buf) = sessions.get(&session_info.id) {
                        state.store.save_session(&buf.info).await.ok();
                    }
                }

                let start_msg = S2C::StartSession {
                    session_id: session_info.id.clone(),
                    initial_prompt: None, // resuming — no new prompt
                    extra_args: vec![],
                    claude_session_id: claude_session_id.clone(),
                    is_resume: true,
                };

                // Send directly via the stored client session handle.
                state.send_to_client(&start_msg).await;

                // Notify dashboards so they can update the session card.
                state
                    .broadcast(&S2D::SessionUpdated {
                        session: session_info.clone(),
                    })
                    .await;

                info!(
                    "client_ws: sent resume request for session {}",
                    session_info.id
                );
            }

            // Ask the client to discover and report available slash commands.
            state.send_to_client(&S2C::QueryCommands).await;

            // Connection notifications are informational only; no ntfy call needed here.
        }

        // -------------------------------------------------------------------
        C2S::SessionStarted {
            session_id,
            pid,
            cwd,
        } => {
            info!(%session_id, %pid, %cwd, "client_ws: SessionStarted");

            let updated_info: Option<SessionInfo> = {
                let mut guard = state.sessions.write().await;
                if let Some(buf) = guard.get_mut(&session_id) {
                    buf.info.status = SessionStatus::Running;
                    buf.info.started_at = Some(Utc::now());
                    buf.info.cwd = cwd;
                    Some(buf.info.clone())
                } else {
                    warn!(%session_id, "client_ws: SessionStarted for unknown session");
                    None
                }
            };

            if let Some(info) = updated_info {
                state.store.save_session(&info).await.ok();
                state
                    .broadcast(&S2D::SessionUpdated {
                        session: info.clone(),
                    })
                    .await;
                state
                    .ntfy
                    .session_started(&session_id, info.name.as_deref(), &info.cwd)
                    .await;
            }
        }

        // -------------------------------------------------------------------
        C2S::CommandList { commands } => {
            info!(count = commands.len(), "client_ws: CommandList");

            {
                let mut cmds = state.commands.write().await;
                *cmds = commands.clone();
            }
            state.broadcast(&S2D::CommandList { commands }).await;
        }

        // -------------------------------------------------------------------
        C2S::SessionEvent { session_id, event } => {
            // Push event into buffer and update stats.
            let info_clone: Option<SessionInfo> = {
                let mut guard = state.sessions.write().await;
                if let Some(buf) = guard.get_mut(&session_id) {
                    if buf.events.len() >= state.max_buffer {
                        buf.events.pop_front();
                    }
                    AppState::update_stats(&mut buf.info.stats, &event);
                    buf.events.push_back(event.clone());
                    Some(buf.info.clone())
                } else {
                    warn!(%session_id, "client_ws: SessionEvent for unknown session");
                    None
                }
            };

            if info_clone.is_some() {
                // Persist the event to the append-only log.
                state.store.append_event(&session_id, &event).await.ok();

                state.ntfy.on_event(&session_id, &event).await;
                state
                    .broadcast(&S2D::SessionEvent { session_id, event })
                    .await;
            }
        }

        // -------------------------------------------------------------------
        C2S::VmConfig { request_id, config } => {
            let mut pending = state.vm_config_pending.write().await;
            if let Some(tx) = pending.remove(&request_id) {
                let _ = tx.send(crate::protocol::VmConfigResponse::Config(config));
            }
        }

        C2S::VmConfigAck {
            request_id,
            success,
            error,
        } => {
            let mut pending = state.vm_config_pending.write().await;
            if let Some(tx) = pending.remove(&request_id) {
                let _ = tx.send(crate::protocol::VmConfigResponse::Ack { success, error });
            }
        }

        C2S::BuildImageLog { request_id, line } => {
            let pending = state.vm_build_log_pending.read().await;
            if let Some(tx) = pending.get(&request_id) {
                let _ = tx.send(line);
            }
        }

        C2S::BuildImageResult {
            request_id,
            success,
            error,
        } => {
            // Close the log stream first so the editing task drains and stops.
            {
                let mut pending = state.vm_build_log_pending.write().await;
                pending.remove(&request_id);
            }
            let mut pending = state.vm_config_pending.write().await;
            if let Some(tx) = pending.remove(&request_id) {
                let _ = tx.send(crate::protocol::VmConfigResponse::BuildResult { success, error });
            }
        }

        // -------------------------------------------------------------------
        C2S::ImportHistory { sessions } => {
            let hostname = {
                let guard = state.client.read().await;
                guard.as_ref().map(|c| c.hostname.clone())
            };

            let mut imported = 0usize;

            for hist in sessions {
                // Skip if a session with this claude_session_id already exists.
                let exists = {
                    let guard = state.sessions.read().await;
                    guard.values().any(|buf| {
                        buf.info
                            .claude_session_id
                            .as_deref()
                            .map_or(false, |id| id == hist.claude_session_id)
                    })
                };
                if exists {
                    continue;
                }

                let session_id = uuid::Uuid::new_v4().to_string();
                let now = Utc::now();
                let info = SessionInfo {
                    id: session_id.clone(),
                    name: None,
                    cwd: hist.cwd,
                    status: SessionStatus::Completed,
                    created_at: hist.created_at.unwrap_or(now),
                    started_at: hist.created_at,
                    ended_at: hist.ended_at.or(hist.created_at).or(Some(now)),
                    stats: SessionStats::default(),
                    client_hostname: hostname.clone(),
                    claude_session_id: Some(hist.claude_session_id),
                };

                state.store.save_session(&info).await.ok();
                for event in &hist.events {
                    state.store.append_event(&session_id, event).await.ok();
                }

                let events: std::collections::VecDeque<_> = hist.events.into_iter().collect();

                {
                    let mut guard = state.sessions.write().await;
                    guard.insert(
                        session_id.clone(),
                        crate::state::SessionBuffer {
                            info: info.clone(),
                            events,
                        },
                    );
                }

                state
                    .broadcast(&S2D::SessionCreated { session: info })
                    .await;
                imported += 1;
            }

            info!("client_ws: ImportHistory: imported {imported} new sessions");
        }

        // -------------------------------------------------------------------
        C2S::SessionEnded {
            session_id,
            exit_code,
            stats,
            error,
        } => {
            info!(%session_id, %exit_code, "client_ws: SessionEnded");

            let updated_info: Option<SessionInfo> = {
                let mut sessions_guard = state.sessions.write().await;
                if let Some(buf) = sessions_guard.get_mut(&session_id) {
                    buf.info.status = if exit_code == 0 {
                        SessionStatus::Completed
                    } else {
                        SessionStatus::Failed
                    };
                    buf.info.stats = stats.clone();
                    buf.info.ended_at = Some(Utc::now());
                    Some(buf.info.clone())
                } else {
                    warn!(%session_id, "client_ws: SessionEnded for unknown session");
                    None
                }
            };

            if let Some(info) = updated_info {
                state.store.save_session(&info).await.ok();
                state
                    .broadcast(&S2D::SessionUpdated {
                        session: info.clone(),
                    })
                    .await;
                state
                    .broadcast(&S2D::SessionEnded {
                        session_id: session_id.clone(),
                        stats: stats.clone(),
                        exit_code,
                        error: error.clone(),
                    })
                    .await;

                state
                    .ntfy
                    .session_ended(&session_id, exit_code == 0, &stats)
                    .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clean up on disconnect
// ---------------------------------------------------------------------------

async fn disconnect(state: &Arc<AppState>) {
    info!("client_ws: disconnected");
    {
        let mut guard = state.client.write().await;
        *guard = None;
    }
    state
        .broadcast(&S2D::ClientStatus {
            connected: false,
            hostname: None,
        })
        .await;

    // Mark every Running/Pending session as Failed — the client lost them on
    // disconnect and they will never send SessionEnded for these sessions.
    let ended: Vec<SessionInfo> = {
        let mut guard = state.sessions.write().await;
        guard
            .values_mut()
            .filter(|buf| {
                matches!(
                    buf.info.status,
                    SessionStatus::Running | SessionStatus::Pending
                )
            })
            .map(|buf| {
                buf.info.status = SessionStatus::Failed;
                buf.info.ended_at = Some(Utc::now());
                buf.info.clone()
            })
            .collect()
    };

    for info in ended {
        state.store.save_session(&info).await.ok();
        state
            .broadcast(&S2D::SessionUpdated { session: info.clone() })
            .await;
        state
            .broadcast(&S2D::SessionEnded {
                session_id: info.id.clone(),
                stats: info.stats.clone(),
                exit_code: -1,
                error: Some("client disconnected".to_string()),
            })
            .await;
        info!("client_ws: marked session {} as Failed (client disconnected)", info.id);
    }
}
