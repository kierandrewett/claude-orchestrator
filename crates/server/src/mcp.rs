use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use dashmap::DashMap;
use futures_util::{future, stream, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::debug;

use claude_db::{Db, EventAction, ScheduleMode, ScheduledEvent};
use claude_events::{EventBus, OrchestratorEvent};

use crate::task_manager::TaskRegistry;

/// One entry per active SSE connection, keyed by orchestrator session_id.
pub type McpConnections = Arc<DashMap<String, mpsc::Sender<String>>>;

#[derive(Clone)]
pub struct McpState {
    pub registry: Arc<TaskRegistry>,
    pub bus: Arc<EventBus>,
    pub connections: McpConnections,
    pub db: Db,
    /// Required Bearer token, or None to allow unauthenticated access.
    pub token: Option<String>,
}

/// GET /mcp?session_id=xxx
///
/// Opens an SSE stream for the MCP SSE transport. The first event tells the
/// client where to POST messages; subsequent events carry JSON-RPC responses.
pub async fn mcp_sse_handler(
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<McpState>,
) -> impl IntoResponse {
    if !check_token_header(&state, &headers) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    let session_id = params.get("session_id").cloned().unwrap_or_default();

    let (tx, rx) = mpsc::channel::<String>(32);
    state.connections.insert(session_id.clone(), tx);

    // Reconstruct the query string so all params (session_id, suppress, …) are
    // preserved in the POST URL the client will use.
    let qs: String = params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let post_url = format!("/mcp?{qs}");

    let endpoint_event = stream::once(future::ready(Ok::<_, Infallible>(
        Event::default().event("endpoint").data(post_url),
    )));

    let message_events = stream::unfold(rx, |mut rx| async {
        rx.recv()
            .await
            .map(|data| (Ok::<_, Infallible>(Event::default().data(data)), rx))
    });

    Sse::new(endpoint_event.chain(message_events)).keep_alive(KeepAlive::default()).into_response()
}

/// POST /mcp?session_id=xxx
///
/// Handles both SSE-transport POSTs (returns 202, sends response via SSE stream)
/// and streamable-HTTP POSTs (returns JSON directly when no SSE connection exists).
pub async fn mcp_post_handler(
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<McpState>,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    if !check_token_header(&state, &headers) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    let session_id = params.get("session_id").map(|s| s.as_str()).unwrap_or("");
    let (suppress_tools, allowed_emojis) = state.registry
        .find_by_session_id(session_id)
        .and_then(|tid| state.registry.with(&tid, |t| {
            (
                t.config.suppress_mcp_tools.iter().cloned().collect::<HashSet<String>>(),
                t.config.allowed_emojis.clone(),
            )
        }))
        .unwrap_or_default();
    let method = req["method"].as_str().unwrap_or("");

    debug!("MCP {method} session={session_id}");

    // Notifications require no response.
    if method.starts_with("notifications/") {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }

    let id = req.get("id").cloned();
    let response = dispatch(&req, id, method, session_id, &suppress_tools, &allowed_emojis, &state).await;

    // SSE transport: send via the open stream and return 202.
    if let Some(tx) = state.connections.get(session_id) {
        if let Ok(json) = serde_json::to_string(&response) {
            if tx.send(json).await.is_ok() {
                return axum::http::StatusCode::ACCEPTED.into_response();
            }
        }
        drop(tx);
        state.connections.remove(session_id);
    }

    // Streamable HTTP transport: return JSON directly.
    Json(response).into_response()
}

async fn dispatch(req: &Value, id: Option<Value>, method: &str, session_id: &str, suppress_tools: &HashSet<String>, allowed_emojis: &[String], state: &McpState) -> Value {
    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "claude-orchestrator", "version": "0.1.0"}
            }
        }),

        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),

        "tools/list" => {
            let mut resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                    {
                        "name": "rename_conversation",
                        "description": "Rename the current conversation in the chat backend (e.g. the Telegram topic name). Call this once after your first substantive response, when you have a clear sense of what this conversation is about.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "title": {
                                    "type": "string",
                                    "description": "Short, descriptive title (3–6 words, max 50 chars)"
                                }
                            },
                            "required": ["title"]
                        }
                    },
                    {
                        "name": "create_scheduled_event",
                        "description": "Create a new scheduled event that fires automatically on a cron schedule. Use this to send reminders, summaries, or prompts at specific times.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Short human-readable name for this event" },
                                "description": { "type": "string", "description": "Optional description" },
                                "schedule": { "type": "string", "description": "Standard 5-field cron expression, e.g. '0 9 * * 1-5' for weekdays at 9am" },
                                "mode": { "type": "string", "enum": ["once", "recurring"], "description": "Run once or repeat on schedule" },
                                "action_type": { "type": "string", "enum": ["send_message", "send_to_scratchpad", "prompt_session"], "description": "What action to take when the event fires" },
                                "task_id": { "type": "string", "description": "Target task ID (required for send_message and prompt_session)" },
                                "message": { "type": "string", "description": "Message text (required for send_message and send_to_scratchpad)" },
                                "prompt": { "type": "string", "description": "Prompt text (required for prompt_session)" },
                                "wake_if_hibernating": { "type": "boolean", "description": "Wake a hibernated session (prompt_session only)" },
                                "skip_if_busy": { "type": "boolean", "description": "Skip if session is already running (prompt_session only)" }
                            },
                            "required": ["name", "schedule", "mode", "action_type"]
                        }
                    },
                    {
                        "name": "list_scheduled_events",
                        "description": "List all scheduled events.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "update_scheduled_event",
                        "description": "Partially update an existing scheduled event. Only the fields you provide are changed — omit a field to leave it unchanged.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "event_id":           { "type": "string",  "description": "Event ID to update (required)" },
                                "name":               { "type": "string",  "description": "New human-readable name" },
                                "description":        { "type": "string",  "description": "New description" },
                                "schedule":           { "type": "string",  "description": "New cron expression (5-field: min hour dom month dow)" },
                                "mode":               { "type": "string",  "description": "once or recurring" },
                                "enabled":            { "type": "boolean", "description": "Enable or disable the event" },
                                "action_type":        { "type": "string",  "description": "New action type: send_message, send_to_scratchpad, or prompt_session" },
                                "task_id":            { "type": "string",  "description": "Target task ID (for send_message / prompt_session)" },
                                "message":            { "type": "string",  "description": "Message text (for send_message / send_to_scratchpad)" },
                                "prompt":             { "type": "string",  "description": "Prompt text (for prompt_session)" },
                                "wake_if_hibernating":{ "type": "boolean", "description": "Wake hibernated session before prompting" },
                                "skip_if_busy":       { "type": "boolean", "description": "Skip if session is currently processing" }
                            },
                            "required": ["event_id"]
                        }
                    },
                    {
                        "name": "delete_scheduled_event",
                        "description": "Permanently delete a scheduled event.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "event_id": { "type": "string", "description": "Event ID to delete" }
                            },
                            "required": ["event_id"]
                        }
                    },
                    {
                        "name": "enable_scheduled_event",
                        "description": "Enable a paused scheduled event.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "event_id": { "type": "string", "description": "Event ID to enable" }
                            },
                            "required": ["event_id"]
                        }
                    },
                    {
                        "name": "disable_scheduled_event",
                        "description": "Disable (pause) a scheduled event without deleting it.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "event_id": { "type": "string", "description": "Event ID to disable" }
                            },
                            "required": ["event_id"]
                        }
                    },
                    {
                        "name": "list_tasks",
                        "description": "List all tasks known to the orchestrator.",
                        "inputSchema": { "type": "object", "properties": {} }
                    },
                    {
                        "name": "search_tasks",
                        "description": "Search tasks by name.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Search query (matches task names)" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "get_task_info",
                        "description": "Get detailed information about a specific task.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "task_id": { "type": "string", "description": "Task ID to look up" }
                            },
                            "required": ["task_id"]
                        }
                    }
                ]
            }
            });
            // Remove suppressed tools from the list.
            if !suppress_tools.is_empty() {
                if let Some(tools) = resp["result"]["tools"].as_array_mut() {
                    tools.retain(|t| {
                        !suppress_tools.contains(t["name"].as_str().unwrap_or(""))
                    });
                }
            }
            resp
        }

        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            match name {
                "rename_conversation" => {
                    let title = req["params"]["arguments"]["title"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    if !allowed_emojis.is_empty() {
                        let first = title.split_whitespace().next().unwrap_or("");
                        if !allowed_emojis.iter().any(|e| e == first) {
                            return mcp_err(id, &format!(
                                "Invalid emoji. Title must begin with one of: {}\nPlease retry.",
                                allowed_emojis.join(", ")
                            ));
                        }
                    }
                    match state.registry.find_by_session_id(session_id) {
                        Some(task_id) => {
                            state.bus.emit(OrchestratorEvent::ConversationRenamed { task_id, title });
                            mcp_ok(id, "Conversation renamed successfully.")
                        }
                        None => mcp_err(id, "Error: session not found"),
                    }
                }

                "create_scheduled_event" => {
                    let args = &req["params"]["arguments"];
                    let name_str = args["name"].as_str().unwrap_or("").to_string();
                    let description = args["description"].as_str().map(|s| s.to_string());
                    let schedule = args["schedule"].as_str().unwrap_or("").to_string();
                    let mode_str = args["mode"].as_str().unwrap_or("recurring");
                    let action_type = args["action_type"].as_str().unwrap_or("");

                    if name_str.is_empty() || schedule.is_empty() {
                        return mcp_err(id, "name and schedule are required");
                    }
                    if let Err(e) = claude_scheduler::validate_cron(&schedule) {
                        return mcp_err(id, &format!("Invalid cron: {e}"));
                    }

                    let action = match action_type {
                        "send_message" => {
                            let task_id = args["task_id"].as_str().unwrap_or("").to_string();
                            let message = args["message"].as_str().unwrap_or("").to_string();
                            if task_id.is_empty() || message.is_empty() {
                                return mcp_err(id, "task_id and message required for send_message");
                            }
                            EventAction::SendMessage { task_id, message }
                        }
                        "send_to_scratchpad" => {
                            let message = args["message"].as_str().unwrap_or("").to_string();
                            if message.is_empty() {
                                return mcp_err(id, "message required for send_to_scratchpad");
                            }
                            EventAction::SendToScratchpad { message }
                        }
                        "prompt_session" => {
                            let task_id = args["task_id"].as_str().unwrap_or("").to_string();
                            let prompt = args["prompt"].as_str().unwrap_or("").to_string();
                            if task_id.is_empty() || prompt.is_empty() {
                                return mcp_err(id, "task_id and prompt required for prompt_session");
                            }
                            let wake_if_hibernating = args["wake_if_hibernating"].as_bool().unwrap_or(false);
                            let skip_if_busy = args["skip_if_busy"].as_bool().unwrap_or(false);
                            EventAction::PromptSession { task_id, prompt, wake_if_hibernating, skip_if_busy }
                        }
                        _ => return mcp_err(id, &format!("Unknown action_type: {action_type}")),
                    };

                    let mode = match mode_str {
                        "once" => ScheduleMode::Once,
                        _ => ScheduleMode::Recurring,
                    };

                    // Get the calling task (origin)
                    let (origin_task_id, origin_task_name) = state.registry
                        .find_by_session_id(session_id)
                        .map(|tid| {
                            let name = state.registry.with(&tid, |t| t.name.clone()).unwrap_or_default();
                            (tid.0, name)
                        })
                        .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

                    let next_run = claude_scheduler::calc_next_run(&schedule);
                    let event_id = claude_db::new_event_id();
                    let now = chrono::Utc::now();

                    let event = ScheduledEvent {
                        id: event_id.clone(),
                        name: name_str.clone(),
                        description,
                        schedule,
                        mode,
                        action,
                        enabled: true,
                        created_at: now,
                        last_run: None,
                        next_run,
                        origin_task_id,
                        origin_task_name,
                        consecutive_failures: 0,
                    };
                    state.db.upsert_event(&event);
                    mcp_ok(id, &format!("Scheduled event '{}' created (id: {event_id}).", name_str))
                }

                "list_scheduled_events" => {
                    let events = state.db.list_events();
                    let text = if events.is_empty() {
                        "No scheduled events.".to_string()
                    } else {
                        events.iter().map(|e| {
                            let status = if e.enabled { "enabled" } else { "disabled" };
                            let next = e.next_run.map(|t| t.to_rfc3339()).unwrap_or_else(|| "none".to_string());
                            format!("[{}] {} — {} — next: {next}", &e.id[..8], e.name, status)
                        }).collect::<Vec<_>>().join("\n")
                    };
                    mcp_ok(id, &text)
                }

                "update_scheduled_event" => {
                    let args = &req["params"]["arguments"];
                    let event_id = args["event_id"].as_str().unwrap_or("");
                    if event_id.is_empty() {
                        return mcp_err(id, "event_id required");
                    }
                    match state.db.get_event(event_id) {
                        None => mcp_err(id, &format!("Event not found: {event_id}")),
                        Some(mut event) => {
                            if let Some(v) = args["name"].as_str() { event.name = v.to_string(); }
                            if let Some(v) = args["description"].as_str() { event.description = Some(v.to_string()); }
                            if let Some(v) = args["mode"].as_str() {
                                event.mode = match v { "once" => ScheduleMode::Once, _ => ScheduleMode::Recurring };
                            }
                            if let Some(schedule) = args["schedule"].as_str() {
                                if let Err(e) = claude_scheduler::validate_cron(schedule) {
                                    return mcp_err(id, &format!("Invalid cron: {e}"));
                                }
                                event.schedule = schedule.to_string();
                                event.next_run = claude_scheduler::calc_next_run(schedule);
                            }
                            if let Some(v) = args["enabled"].as_bool() { event.enabled = v; }

                            // Partial action update: if action_type is provided, replace the whole action.
                            // Otherwise, allow updating individual action fields in-place.
                            if let Some(action_type) = args["action_type"].as_str() {
                                let new_action = match action_type {
                                    "send_message" => {
                                        let task_id = args["task_id"].as_str().unwrap_or("").to_string();
                                        let message = args["message"].as_str().unwrap_or("").to_string();
                                        if task_id.is_empty() || message.is_empty() {
                                            return mcp_err(id, "task_id and message required for send_message");
                                        }
                                        EventAction::SendMessage { task_id, message }
                                    }
                                    "send_to_scratchpad" => {
                                        let message = args["message"].as_str().unwrap_or("").to_string();
                                        if message.is_empty() {
                                            return mcp_err(id, "message required for send_to_scratchpad");
                                        }
                                        EventAction::SendToScratchpad { message }
                                    }
                                    "prompt_session" => {
                                        let task_id = args["task_id"].as_str().unwrap_or("").to_string();
                                        let prompt = args["prompt"].as_str().unwrap_or("").to_string();
                                        if task_id.is_empty() || prompt.is_empty() {
                                            return mcp_err(id, "task_id and prompt required for prompt_session");
                                        }
                                        let wake = args["wake_if_hibernating"].as_bool().unwrap_or(false);
                                        let skip = args["skip_if_busy"].as_bool().unwrap_or(false);
                                        EventAction::PromptSession { task_id, prompt, wake_if_hibernating: wake, skip_if_busy: skip }
                                    }
                                    _ => return mcp_err(id, &format!("Unknown action_type: {action_type}")),
                                };
                                event.action = new_action;
                            } else {
                                // In-place field patches on the existing action variant
                                match &mut event.action {
                                    EventAction::SendMessage { task_id, message } => {
                                        if let Some(v) = args["task_id"].as_str() { *task_id = v.to_string(); }
                                        if let Some(v) = args["message"].as_str() { *message = v.to_string(); }
                                    }
                                    EventAction::SendToScratchpad { message } => {
                                        if let Some(v) = args["message"].as_str() { *message = v.to_string(); }
                                    }
                                    EventAction::PromptSession { task_id, prompt, wake_if_hibernating, skip_if_busy } => {
                                        if let Some(v) = args["task_id"].as_str() { *task_id = v.to_string(); }
                                        if let Some(v) = args["prompt"].as_str() { *prompt = v.to_string(); }
                                        if let Some(v) = args["wake_if_hibernating"].as_bool() { *wake_if_hibernating = v; }
                                        if let Some(v) = args["skip_if_busy"].as_bool() { *skip_if_busy = v; }
                                    }
                                }
                            }

                            state.db.upsert_event(&event);
                            mcp_ok(id, &format!("Event '{}' updated.", event.name))
                        }
                    }
                }

                "delete_scheduled_event" => {
                    let event_id = req["params"]["arguments"]["event_id"].as_str().unwrap_or("");
                    if event_id.is_empty() {
                        return mcp_err(id, "event_id required");
                    }
                    match state.db.get_event(event_id) {
                        None => mcp_err(id, &format!("Event not found: {event_id}")),
                        Some(event) => {
                            state.db.delete_event(event_id);
                            mcp_ok(id, &format!("Event '{}' deleted.", event.name))
                        }
                    }
                }

                "enable_scheduled_event" => {
                    let event_id = req["params"]["arguments"]["event_id"].as_str().unwrap_or("");
                    match state.db.get_event(event_id) {
                        None => mcp_err(id, &format!("Event not found: {event_id}")),
                        Some(event) => {
                            state.db.set_event_enabled(event_id, true);
                            mcp_ok(id, &format!("Event '{}' enabled.", event.name))
                        }
                    }
                }

                "disable_scheduled_event" => {
                    let event_id = req["params"]["arguments"]["event_id"].as_str().unwrap_or("");
                    match state.db.get_event(event_id) {
                        None => mcp_err(id, &format!("Event not found: {event_id}")),
                        Some(event) => {
                            state.db.set_event_enabled(event_id, false);
                            mcp_ok(id, &format!("Event '{}' disabled.", event.name))
                        }
                    }
                }

                "list_tasks" => {
                    let tasks = state.db.list_tasks();
                    let text = if tasks.is_empty() {
                        "No tasks in database.".to_string()
                    } else {
                        tasks.iter().map(|t| {
                            format!("[{}] {} — {}", t.task_id, t.task_name, t.session_status)
                        }).collect::<Vec<_>>().join("\n")
                    };
                    mcp_ok(id, &text)
                }

                "search_tasks" => {
                    let query = req["params"]["arguments"]["query"].as_str().unwrap_or("");
                    let tasks = state.db.search_tasks(query);
                    let text = if tasks.is_empty() {
                        format!("No tasks matching '{query}'.")
                    } else {
                        tasks.iter().map(|t| {
                            format!("[{}] {} — {}", t.task_id, t.task_name, t.session_status)
                        }).collect::<Vec<_>>().join("\n")
                    };
                    mcp_ok(id, &text)
                }

                "get_task_info" => {
                    let task_id = req["params"]["arguments"]["task_id"].as_str().unwrap_or("");
                    match state.db.get_task(task_id) {
                        None => mcp_err(id, &format!("Task not found: {task_id}")),
                        Some(task) => {
                            let text = format!(
                                "Task: {} [{}]\nStatus: {}\nSession: {}\nCreated: {}\nLast activity: {}",
                                task.task_name, task.task_id, task.session_status,
                                task.session_id.as_deref().unwrap_or("none"),
                                task.created_at,
                                task.last_activity.as_deref().unwrap_or("never")
                            );
                            mcp_ok(id, &text)
                        }
                    }
                }

                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("Unknown tool: {name}")}
                }),
            }
        }

        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "Method not found"}
        }),
    }
}

fn check_token_header(state: &McpState, headers: &axum::http::HeaderMap) -> bool {
    match &state.token {
        None => true,
        Some(expected) => headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map_or(false, |t| t == expected),
    }
}

fn mcp_ok(id: Option<Value>, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}],
            "isError": false
        }
    })
}

fn mcp_err(id: Option<Value>, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}],
            "isError": true
        }
    })
}
