use std::collections::HashMap;
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

use claude_events::{EventBus, OrchestratorEvent};

use crate::task_manager::TaskRegistry;

/// One entry per active SSE connection, keyed by orchestrator session_id.
pub type McpConnections = Arc<DashMap<String, mpsc::Sender<String>>>;

#[derive(Clone)]
pub struct McpState {
    pub registry: Arc<TaskRegistry>,
    pub bus: Arc<EventBus>,
    pub connections: McpConnections,
}

/// GET /mcp?session_id=xxx
///
/// Opens an SSE stream for the MCP SSE transport. The first event tells the
/// client where to POST messages; subsequent events carry JSON-RPC responses.
pub async fn mcp_sse_handler(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<McpState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let session_id = params.get("session_id").cloned().unwrap_or_default();

    let (tx, rx) = mpsc::channel::<String>(32);
    state.connections.insert(session_id.clone(), tx);

    // The client will POST its JSON-RPC messages back to this same URL.
    let post_url = format!("/mcp?session_id={session_id}");

    let endpoint_event = stream::once(future::ready(Ok::<_, Infallible>(
        Event::default().event("endpoint").data(post_url),
    )));

    let message_events = stream::unfold(rx, |mut rx| async {
        rx.recv()
            .await
            .map(|data| (Ok::<_, Infallible>(Event::default().data(data)), rx))
    });

    Sse::new(endpoint_event.chain(message_events)).keep_alive(KeepAlive::default())
}

/// POST /mcp?session_id=xxx
///
/// Handles both SSE-transport POSTs (returns 202, sends response via SSE stream)
/// and streamable-HTTP POSTs (returns JSON directly when no SSE connection exists).
pub async fn mcp_post_handler(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<McpState>,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    let session_id = params.get("session_id").map(|s| s.as_str()).unwrap_or("");
    let method = req["method"].as_str().unwrap_or("");

    debug!("MCP {method} session={session_id}");

    // Notifications require no response.
    if method.starts_with("notifications/") {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }

    let id = req.get("id").cloned();
    let response = dispatch(&req, id, method, session_id, &state).await;

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

async fn dispatch(req: &Value, id: Option<Value>, method: &str, session_id: &str, state: &McpState) -> Value {
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

        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
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
                }]
            }
        }),

        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            match name {
                "rename_conversation" => {
                    let title = req["params"]["arguments"]["title"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    match state.registry.find_by_session_id(session_id) {
                        Some(task_id) => {
                            state.bus.emit(OrchestratorEvent::ConversationRenamed { task_id, title });
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{"type": "text", "text": "Conversation renamed successfully."}],
                                    "isError": false
                                }
                            })
                        }
                        None => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{"type": "text", "text": "Error: session not found"}],
                                "isError": true
                            }
                        }),
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
