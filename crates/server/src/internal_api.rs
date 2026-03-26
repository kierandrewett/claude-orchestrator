/// Internal REST API served on the client-daemon port (client_bind).
/// This is what the dashboard Node.js server calls via ORCHESTRATOR_API.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, Json},
    routing::{delete, get, post, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use claude_db::{Db, EventAction, ScheduleMode, ScheduledEvent};
use claude_events::{BackendEvent, BackendSource, MessageRef, ParsedCommand, TaskId};

use crate::mcp_oauth::{self, PendingOAuth};
use crate::mcp_registry::McpServerRegistry;
use crate::task_manager::{TaskRegistry, TaskState};

#[derive(Clone)]
pub struct InternalApiState {
    pub registry: Arc<TaskRegistry>,
    pub backend_tx: mpsc::Sender<BackendEvent>,
    pub db: Db,
    pub mcp_registry: Arc<McpServerRegistry>,
    /// Pending OAuth flows keyed by the `state` parameter.
    pub pending_oauth: Arc<Mutex<HashMap<String, PendingOAuth>>>,
}

pub fn router(state: InternalApiState) -> Router {
    Router::new()
        // Tasks
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/:id", get(get_task))
        .route("/api/tasks/:id", delete(stop_task))
        .route("/api/tasks/:id/message", post(send_message))
        .route("/api/tasks/:id/hibernate", post(hibernate_task))
        .route("/api/tasks/:id/wake", post(wake_task))
        // MCP session status + OAuth
        .route("/api/mcp/session-tools", get(mcp_session_tools))
        .route("/api/mcp/oauth-start/:server_name", get(mcp_oauth_start))
        .route("/api/mcp/oauth-callback", get(mcp_oauth_callback))
        // Scheduled events
        .route("/api/scheduled-events", get(list_events))
        .route("/api/scheduled-events", post(create_event))
        .route("/api/scheduled-events/:id", put(update_event))
        .route("/api/scheduled-events/:id", delete(delete_event))
        .route("/api/scheduled-events/:id/enable", post(enable_event))
        .route("/api/scheduled-events/:id/disable", post(disable_event))
        .with_state(state)
}

// ── Task handlers ──────────────────────────────────────────────────────────────

async fn list_tasks(State(state): State<InternalApiState>) -> Json<Value> {
    let mut tasks = Vec::new();
    for id in state.registry.all_ids() {
        if let Some(t) = state.registry.with(&id, task_to_json) {
            tasks.push(t);
        }
    }
    tasks.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    Json(json!({ "tasks": tasks }))
}

async fn get_task(Path(id): Path<String>, State(state): State<InternalApiState>) -> Json<Value> {
    match state.registry.with(&TaskId(id.clone()), task_to_json) {
        Some(t) => Json(t),
        None => Json(json!({ "id": id, "status": "unknown" })),
    }
}

#[derive(Deserialize)]
struct CreateTaskRequest {
    profile: Option<String>,
    prompt: Option<String>,
}

async fn create_task(
    State(state): State<InternalApiState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<Json<Value>, StatusCode> {
    let profile = body.profile.unwrap_or_else(|| "base".to_string());
    let prompt = body.prompt.unwrap_or_default();
    emit_command(&state, ParsedCommand::New { profile, prompt }, None).await
}

async fn stop_task(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Result<Json<Value>, StatusCode> {
    emit_command(
        &state,
        ParsedCommand::Stop { task_id: Some(TaskId(id.clone())) },
        Some(TaskId(id)),
    )
    .await
}

#[derive(Deserialize)]
struct SendMessageRequest {
    text: String,
}

async fn send_message(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<Value>, StatusCode> {
    let msg_ref = MessageRef::new("web", format!("api-msg-{id}"));
    let source = BackendSource::new("web", "dashboard");
    state
        .backend_tx
        .send(BackendEvent::UserMessage {
            task_id: TaskId(id),
            text: body.text,
            message_ref: msg_ref,
            source,
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "status": "sent" })))
}

async fn hibernate_task(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Result<Json<Value>, StatusCode> {
    emit_command(&state, ParsedCommand::Hibernate, Some(TaskId(id))).await
}

async fn wake_task(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Result<Json<Value>, StatusCode> {
    emit_command(&state, ParsedCommand::Wake, Some(TaskId(id))).await
}

// ── MCP session status ─────────────────────────────────────────────────────────

async fn mcp_session_tools(State(state): State<InternalApiState>) -> Json<Value> {
    // Collect the union of available tools across all running tasks.
    let mut tools: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut has_running = false;
    for id in state.registry.all_ids() {
        state.registry.with(&id, |t| {
            if matches!(t.state, TaskState::Running { .. }) {
                has_running = true;
                for tool in &t.config.available_tools {
                    tools.insert(tool.clone());
                }
            }
        });
    }
    Json(json!({
        "tools": tools.into_iter().collect::<Vec<_>>(),
        "has_running_session": has_running,
    }))
}

// ── Scheduled event handlers ───────────────────────────────────────────────────

async fn list_events(State(state): State<InternalApiState>) -> Json<Value> {
    let events: Vec<Value> = state.db.list_events().iter().map(event_to_json).collect();
    Json(json!({ "events": events }))
}

#[derive(Deserialize)]
struct CreateEventRequest {
    name: String,
    cron: String,
    #[serde(default = "default_true")]
    enabled: bool,
    prompt: Option<String>,
}

fn default_true() -> bool { true }

async fn create_event(
    State(state): State<InternalApiState>,
    Json(body): Json<CreateEventRequest>,
) -> Result<Json<Value>, StatusCode> {
    let next_run = claude_scheduler::calc_next_run(&body.cron);
    let event = ScheduledEvent {
        id: claude_db::new_event_id(),
        name: body.name,
        description: None,
        schedule: body.cron,
        mode: ScheduleMode::Recurring,
        action: EventAction::SendToScratchpad {
            message: body.prompt.unwrap_or_default(),
        },
        enabled: body.enabled,
        created_at: chrono::Utc::now(),
        last_run: None,
        next_run,
        origin_task_id: "dashboard".to_string(),
        origin_task_name: "Dashboard".to_string(),
        consecutive_failures: 0,
    };
    state.db.upsert_event(&event);
    Ok(Json(event_to_json(&event)))
}

#[derive(Deserialize)]
struct UpdateEventRequest {
    name: Option<String>,
    cron: Option<String>,
    enabled: Option<bool>,
    prompt: Option<String>,
}

async fn update_event(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<Json<Value>, StatusCode> {
    let mut event = state
        .db
        .get_event(&id)
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(name) = body.name { event.name = name; }
    if let Some(cron) = body.cron {
        event.next_run = claude_scheduler::calc_next_run(&cron);
        event.schedule = cron;
    }
    if let Some(enabled) = body.enabled { event.enabled = enabled; }
    if let Some(prompt) = body.prompt {
        event.action = EventAction::SendToScratchpad { message: prompt };
    }

    state.db.upsert_event(&event);
    Ok(Json(event_to_json(&event)))
}

async fn delete_event(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Json<Value> {
    state.db.delete_event(&id);
    Json(json!({ "ok": true }))
}

async fn enable_event(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Json<Value> {
    state.db.set_event_enabled(&id, true);
    Json(json!({ "ok": true }))
}

async fn disable_event(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Json<Value> {
    state.db.set_event_enabled(&id, false);
    Json(json!({ "ok": true }))
}

// ── MCP OAuth handlers ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OAuthStartQuery {
    redirect_uri: String,
}

/// Start an OAuth flow for a URL-based MCP server.
/// Returns `{ auth_url }` — the caller opens this in the browser.
async fn mcp_oauth_start(
    Path(server_name): Path<String>,
    Query(q): Query<OAuthStartQuery>,
    State(state): State<InternalApiState>,
) -> Result<Json<Value>, StatusCode> {
    // Find the server URL.
    let url = state
        .mcp_registry
        .custom_servers()
        .into_iter()
        .find(|s| s.name == server_name)
        .and_then(|s| s.url)
        .ok_or(StatusCode::NOT_FOUND)?;

    // Kick off discovery + registration.
    let (auth_url, pending) = mcp_oauth::start_flow(&server_name, &url, &q.redirect_uri)
        .await
        .map_err(|e| {
            tracing::error!("OAuth start failed for '{server_name}': {e}");
            StatusCode::BAD_GATEWAY
        })?;

    // Extract `state` param from the auth URL so we can key the pending entry.
    let oauth_state = url::Url::parse(&auth_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.into_owned())
        })
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    state
        .pending_oauth
        .lock()
        .unwrap()
        .insert(oauth_state, pending);

    Ok(Json(json!({ "auth_url": auth_url })))
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// OAuth callback endpoint — the auth server redirects here after user authorises.
async fn mcp_oauth_callback(
    Query(q): Query<OAuthCallbackQuery>,
    State(state): State<InternalApiState>,
) -> Html<String> {
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        return Html(oauth_page("Authorization failed", &format!("{err}: {desc}"), false));
    }

    let (code, oauth_state) = match (q.code, q.state) {
        (Some(c), Some(s)) => (c, s),
        _ => return Html(oauth_page("Authorization failed", "Missing code or state parameter.", false)),
    };

    let pending = match state.pending_oauth.lock().unwrap().remove(&oauth_state) {
        Some(p) => p,
        None => return Html(oauth_page(
            "Authorization failed",
            "Unknown or expired state. Please try authorizing again.",
            false,
        )),
    };

    let server_name = pending.server_name.clone();

    match mcp_oauth::exchange_code(&pending, &code).await {
        Ok(tokens) => {
            let expires_at = tokens.expires_in.map(|secs| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    + secs
            });
            if let Err(e) = state.mcp_registry.set_oauth_token(
                &server_name,
                tokens.access_token,
                tokens.refresh_token,
                expires_at,
                Some(pending.token_endpoint),
                Some(pending.client_id),
            ) {
                tracing::error!("Failed to store OAuth token for '{server_name}': {e}");
                return Html(oauth_page("Authorization failed", &e, false));
            }
            tracing::info!("OAuth token stored for MCP server '{server_name}'");
            Html(oauth_page(&format!("\"{}\" authorized", server_name), "You can close this tab.", true))
        }
        Err(e) => {
            tracing::error!("OAuth code exchange failed for '{server_name}': {e}");
            Html(oauth_page("Authorization failed", &e.to_string(), false))
        }
    }
}

fn oauth_page(title: &str, message: &str, success: bool) -> String {
    let color = if success { "#10b981" } else { "#ef4444" };
    let icon = if success { "✓" } else { "✗" };
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>{title}</title>
<style>body{{font-family:system-ui,sans-serif;background:#0a0a0a;color:#e4e4e7;
display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}
.card{{background:#18181b;border:1px solid #27272a;border-radius:12px;padding:32px 40px;
text-align:center;max-width:400px}}.icon{{font-size:48px;color:{color};margin-bottom:16px}}
h1{{font-size:18px;font-weight:600;margin:0 0 8px;color:#f4f4f5}}
p{{font-size:14px;color:#71717a;margin:0}}</style></head>
<body><div class="card"><div class="icon">{icon}</div>
<h1>{title}</h1><p>{message}</p></div></body></html>"#
    )
}

// ── Serialisation helpers ──────────────────────────────────────────────────────

fn event_to_json(e: &ScheduledEvent) -> Value {
    let prompt = match &e.action {
        EventAction::SendToScratchpad { message } => Some(message.as_str()),
        EventAction::PromptSession { prompt, .. } => Some(prompt.as_str()),
        EventAction::SendMessage { message, .. } => Some(message.as_str()),
    };
    json!({
        "id": e.id,
        "name": e.name,
        "cron": e.schedule,
        "enabled": e.enabled,
        "mode": e.mode.as_str(),
        "prompt": prompt,
        "next_run": e.next_run.map(|t| t.to_rfc3339()),
        "last_run": e.last_run.map(|t| t.to_rfc3339()),
        "origin_task_name": e.origin_task_name,
    })
}

fn task_to_json(t: &crate::task_manager::Task) -> Value {
    let state_str = match &t.state {
        TaskState::Running { .. } => "Running",
        TaskState::Hibernated => "Hibernated",
        TaskState::Dead => "Dead",
    };
    json!({
        "id": t.id.0,
        "name": t.name,
        "profile": t.profile,
        "state": state_str,
        "created_at": t.created_at.to_rfc3339(),
        "last_activity": t.last_activity.to_rfc3339(),
        "input_tokens": t.usage.input_tokens,
        "output_tokens": t.usage.output_tokens,
        "cost_usd": t.usage.total_cost_usd,
        "turns": t.usage.turns,
    })
}

async fn emit_command(
    state: &InternalApiState,
    command: ParsedCommand,
    task_id: Option<TaskId>,
) -> Result<Json<Value>, StatusCode> {
    let msg_ref = MessageRef::new("web", "api");
    let source = BackendSource::new("web", "dashboard");
    state
        .backend_tx
        .send(BackendEvent::Command {
            command,
            task_id,
            message_ref: msg_ref,
            source,
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "status": "ok" })))
}
