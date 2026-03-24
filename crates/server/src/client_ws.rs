use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use claude_events::{
    EventBus, MessageRef, OrchestratorEvent, SessionPhase, TaskId, TaskStateSummary,
};
use claude_ndjson::{ClaudeEvent, ContentBlock, UsageStats};
use claude_shared::{C2S, S2C};

use crate::client_registry::ClientRegistry;
use crate::task_manager::{TaskRegistry, TaskState};

/// Handle one client-daemon WebSocket connection.
pub async fn handle_client_ws(
    socket: WebSocket,
    registry: Arc<ClientRegistry>,
    task_registry: Arc<TaskRegistry>,
    bus: Arc<EventBus>,
) {
    let (ws_tx, mut ws_rx) = socket.split();
    let ws_tx = Arc::new(tokio::sync::Mutex::new(ws_tx));

    // Outgoing S2C channel for this connection.
    let (s2c_tx, mut s2c_rx) = mpsc::unbounded_channel::<S2C>();

    // Writer task: drain s2c_rx → ws_tx.
    let ws_tx_writer = Arc::clone(&ws_tx);
    let writer = tokio::spawn(async move {
        while let Some(msg) = s2c_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    let mut sink = ws_tx_writer.lock().await;
                    if sink.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => warn!("client_ws: serialise S2C: {e}"),
            }
        }
    });

    let mut client_id: Option<String> = None;

    // Reader loop.
    while let Some(raw) = ws_rx.next().await {
        let text = match raw {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Binary(b)) => match String::from_utf8(b.to_vec()) {
                Ok(t) => t,
                Err(_) => continue,
            },
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Ping(p)) => {
                let mut sink = ws_tx.lock().await;
                let _ = sink.send(Message::Pong(p)).await;
                continue;
            }
            _ => continue,
        };

        let c2s: C2S = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                warn!("client_ws: parse C2S: {e}");
                continue;
            }
        };

        handle_c2s(
            c2s,
            &registry,
            &task_registry,
            &bus,
            &s2c_tx,
            &mut client_id,
        )
        .await;
    }

    if let Some(ref id) = client_id {
        // Hibernate any tasks whose sessions were running on this client so
        // the next incoming message creates a fresh session rather than
        // trying to send to a now-dead session_id.
        let orphaned = registry.sessions_for_client(id);
        for session_id in &orphaned {
            registry.unregister_session(session_id);
            if let Some(task_id) = find_task_by_session(&task_registry, session_id) {
                let old = task_registry
                    .with(&task_id, |t| t.state.summary())
                    .unwrap_or(TaskStateSummary::Dead);
                task_registry.with_mut(&task_id, |t| {
                    t.state = TaskState::Hibernated;
                    t.claude_idle = true;
                    t.current_trigger = None;
                });
                bus.emit(OrchestratorEvent::TaskStateChanged {
                    task_id,
                    old_state: old,
                    new_state: TaskStateSummary::Hibernated,
                });
            }
        }
        registry.unregister_client(id);
        info!("client_ws: client {id} disconnected");
    }

    writer.abort();
}

async fn handle_c2s(
    msg: C2S,
    registry: &Arc<ClientRegistry>,
    task_registry: &Arc<TaskRegistry>,
    bus: &Arc<EventBus>,
    s2c_tx: &mpsc::UnboundedSender<S2C>,
    client_id: &mut Option<String>,
) {
    match msg {
        C2S::Hello {
            client_id: id,
            hostname,
        } => {
            info!("client_ws: Hello from {id} ({hostname})");
            registry.register_client(id.clone(), s2c_tx.clone());
            *client_id = Some(id);
        }

        C2S::SessionStarted { session_id, cwd, .. } => {
            info!("client_ws: SessionStarted {session_id} cwd={cwd}");
            if let Some(ref cid) = client_id {
                registry.register_session(session_id.clone(), cid.clone());
            }
            // Mark the owning task as Running.
            if let Some(task_id) = find_task_by_session(task_registry, &session_id) {
                task_registry.with_mut(&task_id, |t| {
                    t.state = TaskState::Running {
                        session_id: session_id.clone(),
                    };
                });
                bus.emit(OrchestratorEvent::PhaseChanged {
                    task_id,
                    phase: SessionPhase::Starting,
                    trigger_message: None,
                });
            }
        }

        C2S::SessionEvent { session_id, event } => {
            let task_id = find_task_by_session(task_registry, &session_id);
            let Some(task_id) = task_id else { return };

            let trigger_ref = task_registry
                .with(&task_id, |t| t.current_trigger.clone())
                .flatten();
            let show_thinking = task_registry
                .with(&task_id, |t| t.config.show_thinking)
                .unwrap_or(false);

            // Parse NDJSON event and emit orchestrator events.
            let orch_events =
                ndjson_to_orch_events(&task_id, &event, trigger_ref, show_thinking);

            // If this is a result event, accumulate usage stats on the task.
            if let Ok(ClaudeEvent::Result(ref result)) =
                serde_json::from_value::<ClaudeEvent>(event.clone())
            {
                task_registry.with_mut(&task_id, |t| {
                    t.usage.ingest(result);
                });
            }

            for ev in orch_events {
                bus.emit(ev);
            }
        }

        C2S::SessionEnded {
            session_id,
            exit_code,
            error,
            ..
        } => {
            info!("client_ws: SessionEnded {session_id} exit={exit_code}");
            if let Some(ref e) = error {
                warn!("client_ws: session {session_id} ended with error: {e}");
            }
            registry.unregister_session(&session_id);

            if let Some(task_id) = find_task_by_session(task_registry, &session_id) {
                let old = task_registry
                    .with(&task_id, |t| t.state.summary())
                    .unwrap_or(TaskStateSummary::Dead);
                task_registry.with_mut(&task_id, |t| {
                    t.state = TaskState::Hibernated;
                    t.claude_idle = true;
                    t.current_trigger = None;
                });
                bus.emit(OrchestratorEvent::TaskStateChanged {
                    task_id,
                    old_state: old,
                    new_state: TaskStateSummary::Hibernated,
                });
            }
        }

        C2S::ClaudeIdle { session_id } => {
            debug!("client_ws: ClaudeIdle {session_id}");
            if let Some(task_id) = find_task_by_session(task_registry, &session_id) {
                task_registry.with_mut(&task_id, |t| {
                    t.claude_idle = true;
                    t.current_trigger = None;
                });
            }
        }

    }
}

/// Find a task whose Running session_id matches.
fn find_task_by_session(registry: &TaskRegistry, session_id: &str) -> Option<TaskId> {
    registry.all_ids().into_iter().find(|id| {
        registry
            .with(id, |t| {
                matches!(&t.state, TaskState::Running { session_id: sid } if sid == session_id)
            })
            .unwrap_or(false)
    })
}

/// Convert a raw NDJSON event value into zero or more OrchestratorEvents.
fn ndjson_to_orch_events(
    task_id: &TaskId,
    event: &serde_json::Value,
    trigger_ref: Option<MessageRef>,
    show_thinking: bool,
) -> Vec<OrchestratorEvent> {
    let mut out = Vec::new();

    let claude_event = match serde_json::from_value::<ClaudeEvent>(event.clone()) {
        Ok(e) => e,
        Err(_) => return out,
    };

    match claude_event {
        ClaudeEvent::Assistant(msg) => {
            let content = msg
                .message
                .as_ref()
                .map(|m| m.content.as_slice())
                .unwrap_or(&[]);

            // Track whether we've already emitted a TextOutput for this message.
            let mut text_started = false;
            let mut last_tool_name: Option<String> = None;

            for block in content {
                match block {
                    ContentBlock::Text { text } => {
                        out.push(OrchestratorEvent::TextOutput {
                            task_id: task_id.clone(),
                            text: text.clone(),
                            is_continuation: text_started,
                            trigger_ref: trigger_ref.clone(),
                        });
                        text_started = true;
                    }
                    ContentBlock::Thinking { thinking } => {
                        if show_thinking {
                            out.push(OrchestratorEvent::Thinking {
                                task_id: task_id.clone(),
                                text: thinking.clone(),
                                trigger_ref: trigger_ref.clone(),
                            });
                        }
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let summary = serde_json::to_string(input)
                            .unwrap_or_default()
                            .chars()
                            .take(400)
                            .collect();
                        last_tool_name = Some(name.clone());
                        out.push(OrchestratorEvent::ToolStarted {
                            task_id: task_id.clone(),
                            tool_name: name.clone(),
                            summary,
                            trigger_ref: trigger_ref.clone(),
                        });
                    }
                    ContentBlock::Unknown => {}
                }
            }
            let _ = last_tool_name;
        }

        ClaudeEvent::ToolResult(result) => {
            // Extract a text preview from the result content.
            let preview = result
                .content
                .as_ref()
                .and_then(|v| {
                    if let Some(s) = v.as_str() {
                        return Some(s.chars().take(8000).collect::<String>());
                    }
                    v.as_array()
                        .and_then(|a| a.first())
                        .and_then(|el| el.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.chars().take(8000).collect())
                });

            out.push(OrchestratorEvent::ToolCompleted {
                task_id: task_id.clone(),
                tool_name: String::new(), // name not available in tool_result
                summary: String::new(),
                is_error: result.is_error.unwrap_or(false),
                output_preview: preview,
                trigger_ref: trigger_ref.clone(),
            });
        }

        ClaudeEvent::Result(result) => {
            let mut usage = UsageStats::default();
            usage.ingest(&result);
            let duration_secs = result
                .duration_ms
                .map(|ms| ms as f64 / 1000.0)
                .unwrap_or(0.0);
            out.push(OrchestratorEvent::TurnComplete {
                task_id: task_id.clone(),
                usage,
                duration_secs,
                trigger_ref,
            });
        }

        ClaudeEvent::System(_) => {
            // System event means Claude started up and is ready.
            out.push(OrchestratorEvent::PhaseChanged {
                task_id: task_id.clone(),
                phase: SessionPhase::Responding,
                trigger_message: trigger_ref,
            });
        }

        _ => {}
    }

    out
}
