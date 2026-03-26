/// Internal REST API served on the client-daemon port (client_bind).
/// This is what the dashboard Node.js server calls via ORCHESTRATOR_API.
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use claude_db::{Db, EventAction, ScheduleMode, ScheduledEvent};
use claude_events::{BackendEvent, BackendSource, MessageRef, ParsedCommand, TaskId};

use crate::task_manager::{TaskRegistry, TaskState};

#[derive(Clone)]
pub struct InternalApiState {
    pub registry: Arc<TaskRegistry>,
    pub backend_tx: mpsc::Sender<BackendEvent>,
    pub db: Db,
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
        // MCP session status
        .route("/api/mcp/session-tools", get(mcp_session_tools))
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
