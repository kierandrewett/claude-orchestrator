use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub use crate::protocol::VmConfigResponse;

use actix_ws::Session as WsSession;
use tokio::sync::{broadcast, RwLock};
use tracing::error;

use crate::protocol::*;

// ---------------------------------------------------------------------------
// ClientHandle — wraps a connected client daemon WebSocket session
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct ClientHandle {
    pub id: String,
    pub hostname: String,
    pub session: WsSession,
}

// ---------------------------------------------------------------------------
// SessionBuffer — in-memory ring buffer of events for a single session
// ---------------------------------------------------------------------------

pub struct SessionBuffer {
    pub info: SessionInfo,
    pub events: VecDeque<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    pub client_token: String,
    pub ntfy: Arc<crate::ntfy::NtfyManager>,
    pub max_buffer: usize,
    pub client: RwLock<Option<ClientHandle>>,
    pub sessions: RwLock<HashMap<String, SessionBuffer>>,
    /// Broadcast channel for pushing S2D events to all SSE subscribers.
    pub sse_tx: broadcast::Sender<String>,
    pub commands: RwLock<Vec<crate::protocol::SlashCommand>>,
    pub store: Arc<crate::persist::Store>,
    pub pending_resumes: RwLock<Vec<SessionInfo>>,
    /// Pending VM config request/response correlations.
    /// Key = request_id, value = oneshot sender to wake the waiting handler.
    pub vm_config_pending:
        RwLock<HashMap<String, tokio::sync::oneshot::Sender<VmConfigResponse>>>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        let store = Arc::new(crate::persist::Store::new(&data_dir));

        let public_url = std::env::var("PUBLIC_URL").unwrap_or_else(|_| {
            format!(
                "http://localhost:{}",
                std::env::var("PORT").unwrap_or_else(|_| "8080".to_string())
            )
        });

        let (sse_tx, _) = broadcast::channel(512);

        Arc::new(Self {
            client_token: std::env::var("CLIENT_TOKEN")
                .unwrap_or_else(|_| "client-secret".to_string()),
            ntfy: crate::ntfy::NtfyManager::new(
                std::env::var("NTFY_URL")
                    .unwrap_or_else(|_| "https://ntfy.drewett.dev/claude".to_string()),
                std::env::var("NTFY_TOKEN").ok(),
                public_url,
            ),
            max_buffer: std::env::var("MAX_BUFFER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            client: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
            sse_tx,
            commands: RwLock::new(Vec::new()),
            store,
            pending_resumes: RwLock::new(Vec::new()),
            vm_config_pending: RwLock::new(HashMap::new()),
        })
    }

    /// Load sessions persisted on disk into the in-memory state.
    /// Sessions that were Running when the server last stopped are queued for
    /// resumption in `pending_resumes`.
    pub async fn load_from_disk(&self) {
        let loaded = self.store.load_all().await;
        let mut sessions = self.sessions.write().await;
        let mut resumes = self.pending_resumes.write().await;

        for (mut info, events) in loaded {
            let needs_resume = info.status == SessionStatus::Running;
            if needs_resume {
                info.status = SessionStatus::Pending;
                resumes.push(info.clone());
            }

            let buf = SessionBuffer {
                info,
                events: events.into_iter().collect(),
            };
            sessions.insert(buf.info.id.clone(), buf);
        }

        tracing::info!(
            "Loaded {} sessions from disk ({} pending resume)",
            sessions.len(),
            resumes.len()
        );
    }

    // -----------------------------------------------------------------------
    // broadcast — send S2D JSON to every SSE subscriber
    // -----------------------------------------------------------------------
    pub async fn broadcast(&self, msg: &S2D) {
        let text = match serde_json::to_string(msg) {
            Ok(t) => t,
            Err(e) => {
                error!("broadcast: failed to serialize S2D: {e}");
                return;
            }
        };
        // A send error just means no subscribers are connected — ignore it.
        let _ = self.sse_tx.send(text);
    }

    // -----------------------------------------------------------------------
    // send_to_client — send S2C to the connected client daemon
    // -----------------------------------------------------------------------
    pub async fn send_to_client(&self, msg: &S2C) -> bool {
        let text = match serde_json::to_string(msg) {
            Ok(t) => t,
            Err(e) => {
                error!("send_to_client: failed to serialize S2C: {e}");
                return false;
            }
        };

        let session_clone: Option<WsSession> = {
            let guard = self.client.read().await;
            guard.as_ref().map(|c| c.session.clone())
        };

        match session_clone {
            None => {
                tracing::warn!("send_to_client: no client connected");
                false
            }
            Some(mut ws) => match ws.text(text).await {
                Ok(()) => true,
                Err(e) => {
                    error!("send_to_client: send failed: {e}");
                    false
                }
            },
        }
    }

    // -----------------------------------------------------------------------
    // update_stats — parse a raw Claude NDJSON event and mutate SessionStats
    // -----------------------------------------------------------------------
    pub fn update_stats(stats: &mut SessionStats, event: &serde_json::Value) {
        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // With --include-partial-messages, Anthropic streaming events are wrapped
        // in a stream_event envelope — recurse into the inner event.
        if event_type == "stream_event" {
            if let Some(inner) = event.get("event") {
                Self::update_stats(stats, inner);
            }
            return;
        }

        match event_type {
            "message_delta" => {
                if let Some(reason) =
                    event.pointer("/delta/stop_reason").and_then(|v| v.as_str())
                {
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
                if let Some(content) =
                    event.pointer("/message/content").and_then(|c| c.as_array())
                {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            if let Some(tool_name) =
                                block.get("name").and_then(|v| v.as_str())
                            {
                                *stats.tool_calls.entry(tool_name.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            "result" => {
                // Authoritative final cost — field changed to total_cost_usd in Claude Code 2.x.
                if let Some(cost) = event.get("total_cost_usd").and_then(|v| v.as_f64()) {
                    stats.cost_usd = Some(cost);
                }
                // Authoritative final token totals override accumulated per-turn counts.
                if let Some(tokens) =
                    event.pointer("/usage/input_tokens").and_then(|v| v.as_u64())
                {
                    stats.input_tokens = tokens;
                }
                if let Some(tokens) =
                    event.pointer("/usage/output_tokens").and_then(|v| v.as_u64())
                {
                    stats.output_tokens = tokens;
                }
                if let Some(turns) = event.get("num_turns").and_then(|v| v.as_u64()) {
                    stats.turns = turns as u32;
                }
            }
            _ => {}
        }
    }
}
