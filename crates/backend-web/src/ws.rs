use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use claude_events::{BackendEvent, BackendSource, MessageRef, OrchestratorEvent, TaskId};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, warn};

/// Shared list of active WebSocket senders (for broadcasting orch events to all dashboard clients).
pub type WsBroadcaster = Arc<RwLock<Vec<tokio::sync::mpsc::UnboundedSender<String>>>>;

/// Handle a single WebSocket client connection.
///
/// - Forwards `OrchestratorEvent`s (serialised as JSON) → client.
/// - Receives JSON messages from client → emits `BackendEvent`s.
pub async fn handle_ws_client(
    socket: WebSocket,
    mut orch_rx: broadcast::Receiver<OrchestratorEvent>,
    backend_tx: mpsc::Sender<BackendEvent>,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Channel to pipe orchestrator events into the ws_tx half.
    let (local_tx, mut local_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Writer task: pull from local_rx → ws_tx.
    let writer = tokio::spawn(async move {
        while let Some(text) = local_rx.recv().await {
            if ws_tx.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Orchestrator events → local_tx.
    let local_tx_clone = local_tx.clone();
    let orch_forward = tokio::spawn(async move {
        loop {
            match orch_rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if local_tx_clone.send(json).is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("web-ws: lagged by {n} orchestrator events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Reader: ws_rx → BackendEvent.
    let default_task_id = TaskId("scratchpad".to_string());
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                debug!("web-ws: read error: {e}");
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        // Parse as a simple JSON object with "text" field.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(msg_text) = val.get("text").and_then(|v| v.as_str()) {
                let task_id = val
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .map(|s| TaskId(s.to_string()))
                    .unwrap_or_else(|| default_task_id.clone());

                let msg_ref = MessageRef::new("web", format!("ws-{}", uuid_v4()));
                let source = BackendSource::new("web", "dashboard");

                if msg_text.starts_with('/') {
                    if let Ok(cmd) = claude_events::parse_command(msg_text) {
                        let _ = backend_tx
                            .send(BackendEvent::Command {
                                command: cmd,
                                task_id: Some(task_id),
                                message_ref: msg_ref,
                                source,
                            })
                            .await;
                    }
                } else {
                    let _ = backend_tx
                        .send(BackendEvent::UserMessage {
                            task_id,
                            text: msg_text.to_string(),
                            message_ref: msg_ref,
                            source,
                        })
                        .await;
                }
            }
        }
    }

    writer.abort();
    orch_forward.abort();
}

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
}
