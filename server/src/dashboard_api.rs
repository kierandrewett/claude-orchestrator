use std::sync::Arc;

use actix_web::{web, HttpResponse};
use bytes::Bytes;
use chrono::Utc;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{
    protocol::*,
    state::{AppState, SessionBuffer},
};

// ---------------------------------------------------------------------------
// GET /api/status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StatusResponse {
    connected: bool,
    hostname: Option<String>,
    commands: Vec<SlashCommand>,
}

pub async fn get_status(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let (connected, hostname) = {
        let guard = state.client.read().await;
        let c = guard.is_some();
        let h = guard.as_ref().map(|g| g.hostname.clone());
        (c, h)
    };
    let commands = state.commands.read().await.clone();
    HttpResponse::Ok().json(StatusResponse {
        connected,
        hostname,
        commands,
    })
}

// ---------------------------------------------------------------------------
// GET /api/sessions
// ---------------------------------------------------------------------------

pub async fn list_sessions(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let sessions: Vec<SessionInfo> = {
        let guard = state.sessions.read().await;
        guard.values().map(|b| b.info.clone()).collect()
    };
    HttpResponse::Ok().json(serde_json::json!({ "sessions": sessions }))
}

// ---------------------------------------------------------------------------
// POST /api/sessions
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateSessionBody {
    pub name: Option<String>,
    pub initial_prompt: Option<String>,
}

pub async fn create_session(
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreateSessionBody>,
) -> HttpResponse {
    let session_id = uuid::Uuid::new_v4().to_string();
    let claude_session_id = uuid::Uuid::new_v4().to_string();
    info!(%session_id, name = ?body.name, "api: CreateSession");

    let hostname = {
        let guard = state.client.read().await;
        guard.as_ref().map(|c| c.hostname.clone())
    };

    let info = SessionInfo {
        id: session_id.clone(),
        name: body.name.clone(),
        cwd: String::new(),
        status: SessionStatus::Pending,
        created_at: Utc::now(),
        started_at: None,
        ended_at: None,
        stats: SessionStats::default(),
        client_hostname: hostname,
        claude_session_id: Some(claude_session_id.clone()),
    };

    state.store.save_session(&info).await.ok();

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

    state
        .broadcast(&S2D::SessionCreated {
            session: info.clone(),
        })
        .await;

    state
        .send_to_client(&S2C::StartSession {
            session_id,
            initial_prompt: body.initial_prompt.clone(),
            extra_args: Vec::new(),
            claude_session_id,
            is_resume: false,
        })
        .await;

    HttpResponse::Ok().json(serde_json::json!({ "session": info }))
}

// ---------------------------------------------------------------------------
// GET /api/sessions/{id}/history
// ---------------------------------------------------------------------------

pub async fn get_history(state: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let session_id = path.into_inner();
    let events_opt = {
        let guard = state.sessions.read().await;
        guard
            .get(&session_id)
            .map(|b| b.events.iter().cloned().collect::<Vec<_>>())
    };

    match events_opt {
        Some(events) => HttpResponse::Ok().json(serde_json::json!({
            "session_id": session_id,
            "events": events,
        })),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("session not found: {session_id}")
        })),
    }
}

// ---------------------------------------------------------------------------
// POST /api/sessions/{id}/input
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SendInputBody {
    pub text: String,
}

pub async fn send_input(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<SendInputBody>,
) -> HttpResponse {
    let session_id = path.into_inner();
    info!(%session_id, "api: SendInput");
    state
        .send_to_client(&S2C::SendInput {
            session_id,
            text: body.text.clone(),
        })
        .await;
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// DELETE /api/sessions/{id}
// ---------------------------------------------------------------------------

pub async fn kill_session(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let session_id = path.into_inner();
    info!(%session_id, "api: KillSession");

    let updated_info = {
        let mut guard = state.sessions.write().await;
        if let Some(buf) = guard.get_mut(&session_id) {
            buf.info.status = SessionStatus::Killed;
            buf.info.ended_at = Some(Utc::now());
            Some(buf.info.clone())
        } else {
            warn!(%session_id, "api: KillSession for unknown session");
            None
        }
    };

    if let Some(info) = updated_info {
        state.store.save_session(&info).await.ok();
        state
            .broadcast(&S2D::SessionUpdated { session: info })
            .await;
        state.send_to_client(&S2C::KillSession { session_id }).await;
    }

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// GET /api/events  — SSE stream
// ---------------------------------------------------------------------------

pub async fn sse_events(state: web::Data<Arc<AppState>>) -> HttpResponse {
    // Subscribe to broadcast BEFORE reading state to avoid missing events.
    let rx = state.sse_tx.subscribe();

    // Collect current state for the initial burst.
    let initial_msgs: Vec<String> = {
        let (connected, hostname) = {
            let guard = state.client.read().await;
            let c = guard.is_some();
            let h = guard.as_ref().map(|g| g.hostname.clone());
            (c, h)
        };

        let sessions: Vec<SessionInfo> = {
            let guard = state.sessions.read().await;
            guard.values().map(|b| b.info.clone()).collect()
        };

        let commands: Vec<SlashCommand> = state.commands.read().await.clone();

        let mut msgs = Vec::new();
        if let Ok(s) = serde_json::to_string(&S2D::ClientStatus {
            connected,
            hostname,
        }) {
            msgs.push(s);
        }
        if let Ok(s) = serde_json::to_string(&S2D::SessionList { sessions }) {
            msgs.push(s);
        }
        if !commands.is_empty() {
            if let Ok(s) = serde_json::to_string(&S2D::CommandList { commands }) {
                msgs.push(s);
            }
        }
        msgs
    };

    let initial_stream = stream::iter(
        initial_msgs
            .into_iter()
            .map(|msg| Ok::<Bytes, actix_web::Error>(Bytes::from(format!("data: {msg}\n\n")))),
    );

    let live_stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let data = Bytes::from(format!("data: {msg}\n\n"));
                    return Some((Ok::<Bytes, actix_web::Error>(data), rx));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged by {n} messages");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    let combined = initial_stream.chain(live_stream);

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(combined)
}
