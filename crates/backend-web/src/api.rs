use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};

use claude_events::{
    BackendEvent, BackendSource, MessageRef, OrchestratorEvent, ParsedCommand, TaskId,
};

/// Shared app state passed to all route handlers.
#[derive(Clone)]
pub struct ApiState {
    pub backend_tx: mpsc::Sender<BackendEvent>,
    pub orch_tx: broadcast::Sender<OrchestratorEvent>,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/:id", get(get_task))
        .route("/api/tasks/:id", delete(stop_task))
        .route("/api/tasks/:id/message", post(send_message))
        .route("/api/tasks/:id/hibernate", post(hibernate_task))
        .route("/api/profiles", get(list_profiles))
        .route("/api/config", get(get_config))
        .with_state(state)
}

// ── Route handlers ─────────────────────────────────────────────────────────────

async fn list_tasks(State(_state): State<ApiState>) -> Json<Value> {
    // In the full implementation, query the TaskRegistry.
    // For now return an empty list — the dashboard will receive real-time events via WebSocket.
    Json(json!({ "tasks": [] }))
}

#[derive(Deserialize)]
struct CreateTaskRequest {
    profile: Option<String>,
    prompt: Option<String>,
}

async fn create_task(
    State(state): State<ApiState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<Json<Value>, StatusCode> {
    let profile = body.profile.unwrap_or_else(|| "base".to_string());
    let prompt = body.prompt.unwrap_or_default();

    let msg_ref = MessageRef::new("web", "api-create");
    let source = BackendSource::new("web", "dashboard");

    state
        .backend_tx
        .send(BackendEvent::Command {
            command: ParsedCommand::New { profile, prompt },
            task_id: None,
            message_ref: msg_ref,
            source,
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    Ok(Json(json!({ "status": "creating" })))
}

async fn get_task(
    Path(id): Path<String>,
    State(_state): State<ApiState>,
) -> Json<Value> {
    Json(json!({ "id": id, "status": "unknown" }))
}

async fn stop_task(
    Path(id): Path<String>,
    State(state): State<ApiState>,
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
    State(state): State<ApiState>,
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
    State(state): State<ApiState>,
) -> Result<Json<Value>, StatusCode> {
    emit_command(&state, ParsedCommand::Hibernate, Some(TaskId(id))).await
}

async fn list_profiles(State(_state): State<ApiState>) -> Json<Value> {
    let profiles = claude_containers::load_profiles(
        &std::path::PathBuf::from("docker/profiles"),
    )
    .unwrap_or_default();
    let names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    Json(json!({ "profiles": names }))
}

async fn get_config(State(_state): State<ApiState>) -> Json<Value> {
    Json(json!({ "show_thinking": false, "stream_coalesce_ms": 500 }))
}

// ── Helper ─────────────────────────────────────────────────────────────────────

async fn emit_command(
    state: &ApiState,
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
