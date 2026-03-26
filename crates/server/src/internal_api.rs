/// Internal REST API served on the client-daemon port (client_bind).
/// This is what the dashboard Node.js server calls via ORCHESTRATOR_API.
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use claude_events::{BackendEvent, BackendSource, MessageRef, ParsedCommand, TaskId};

use crate::task_manager::{TaskRegistry, TaskState};

#[derive(Clone)]
pub struct InternalApiState {
    pub registry: Arc<TaskRegistry>,
    pub backend_tx: mpsc::Sender<BackendEvent>,
}

pub fn router(state: InternalApiState) -> Router {
    Router::new()
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/:id", get(get_task))
        .route("/api/tasks/:id", delete(stop_task))
        .route("/api/tasks/:id/message", post(send_message))
        .route("/api/tasks/:id/hibernate", post(hibernate_task))
        .route("/api/tasks/:id/wake", post(wake_task))
        .with_state(state)
}

// ── Handlers ───────────────────────────────────────────────────────────────────

async fn list_tasks(State(state): State<InternalApiState>) -> Json<Value> {
    let mut tasks = Vec::new();
    for id in state.registry.all_ids() {
        if let Some(t) = state.registry.with(&id, task_to_json) {
            tasks.push(t);
        }
    }
    // Sort by created_at descending
    tasks.sort_by(|a, b| {
        b["created_at"].as_str().cmp(&a["created_at"].as_str())
    });
    Json(json!({ "tasks": tasks }))
}

async fn get_task(
    Path(id): Path<String>,
    State(state): State<InternalApiState>,
) -> Json<Value> {
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

// ── Helpers ────────────────────────────────────────────────────────────────────

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
