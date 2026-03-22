use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use actix_ws::Session as WsSession;
use tokio::sync::RwLock;
use tracing::{error, warn};

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
    pub dashboard_token: String,
    pub ntfy: Arc<crate::ntfy::NtfyManager>,
    pub max_buffer: usize,
    pub client: RwLock<Option<ClientHandle>>,
    pub sessions: RwLock<HashMap<String, SessionBuffer>>,
    pub dashboards: RwLock<Vec<WsSession>>,
    pub commands: RwLock<Vec<crate::protocol::SlashCommand>>,  // current slash command list from client
    pub http: reqwest::Client,
    pub store: Arc<crate::persist::Store>,
    pub pending_resumes: RwLock<Vec<SessionInfo>>,
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

        Arc::new(Self {
            client_token: std::env::var("CLIENT_TOKEN")
                .unwrap_or_else(|_| "client-secret".to_string()),
            dashboard_token: std::env::var("DASHBOARD_TOKEN")
                .unwrap_or_else(|_| "dashboard-secret".to_string()),
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
            dashboards: RwLock::new(Vec::new()),
            commands: RwLock::new(Vec::new()),
            http: reqwest::Client::new(),
            store,
            pending_resumes: RwLock::new(Vec::new()),
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
                // Mark as pending; will be resumed when the client daemon connects.
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
    // broadcast — send S2D to every connected dashboard, pruning dead sessions
    // -----------------------------------------------------------------------
    pub async fn broadcast(&self, msg: &S2D) {
        let text = match serde_json::to_string(msg) {
            Ok(t) => t,
            Err(e) => {
                error!("broadcast: failed to serialize S2D: {e}");
                return;
            }
        };

        // Collect sessions; we need to release the read lock before awaiting.
        let sessions: Vec<WsSession> = {
            let guard = self.dashboards.read().await;
            guard.clone()
        };

        let mut live: Vec<WsSession> = Vec::with_capacity(sessions.len());
        for mut ws in sessions {
            match ws.text(text.clone()).await {
                Ok(()) => live.push(ws),
                Err(e) => {
                    warn!("broadcast: dashboard send failed (removing): {e}");
                    // session is already consumed / closed — drop it
                }
            }
        }

        // Write back only the live sessions.
        let mut guard = self.dashboards.write().await;
        *guard = live;
    }

    // -----------------------------------------------------------------------
    // send_to_client — send S2C to the connected client daemon
    // Returns true if the message was delivered.
    // -----------------------------------------------------------------------
    pub async fn send_to_client(&self, msg: &S2C) -> bool {
        let text = match serde_json::to_string(msg) {
            Ok(t) => t,
            Err(e) => {
                error!("send_to_client: failed to serialize S2C: {e}");
                return false;
            }
        };

        // Clone the session handle so we can release the lock before awaiting.
        let session_clone: Option<WsSession> = {
            let guard = self.client.read().await;
            guard.as_ref().map(|c| c.session.clone())
        };

        match session_clone {
            None => {
                warn!("send_to_client: no client connected");
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
        // message_start → input token count
        if event.get("type").and_then(|t| t.as_str()) == Some("message_start") {
            if let Some(tokens) = event
                .pointer("/message/usage/input_tokens")
                .and_then(|v| v.as_u64())
            {
                stats.input_tokens = stats.input_tokens.saturating_add(tokens);
            }
        }

        // message_delta → output tokens and stop_reason
        if event.get("type").and_then(|t| t.as_str()) == Some("message_delta") {
            if let Some(tokens) = event
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64())
            {
                stats.output_tokens = stats.output_tokens.saturating_add(tokens);
            }
            if let Some(reason) = event
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str())
            {
                stats.stop_reason = Some(reason.to_string());
            }
        }

        // content_block_start with type=tool_use → count tool calls
        if event.get("type").and_then(|t| t.as_str()) == Some("content_block_start") {
            if event
                .pointer("/content_block/type")
                .and_then(|v| v.as_str())
                == Some("tool_use")
            {
                if let Some(tool_name) = event
                    .pointer("/content_block/name")
                    .and_then(|v| v.as_str())
                {
                    *stats.tool_calls.entry(tool_name.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Turn-complete format: assistant message with tool_use content blocks
        if event.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(content) = event.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(tool_name) = block.get("name").and_then(|v| v.as_str()) {
                            *stats.tool_calls.entry(tool_name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        // result event → cost_usd, num_turns
        if event.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(cost) = event.get("cost_usd").and_then(|v| v.as_f64()) {
                stats.cost_usd = Some(stats.cost_usd.unwrap_or(0.0) + cost);
            }
            if let Some(turns) = event.get("num_turns").and_then(|v| v.as_u64()) {
                stats.turns = turns as u32;
            }
        }
    }
}
