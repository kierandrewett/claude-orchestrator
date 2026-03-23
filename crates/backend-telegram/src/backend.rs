use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use backend_traits::MessagingBackend;
use claude_events::{
    BackendEvent, BackendSource, MessageRef, OrchestratorEvent, TaskId, TaskStateSummary,
};
use teloxide::prelude::*;
use teloxide::types::{ChatId, MessageId, ThreadId};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::formatting::{
    format_error, format_hibernated, format_thinking, format_tool_completed, format_tool_started,
    format_turn_complete,
};
use crate::reactions::ReactionTracker;
use crate::streaming::StreamingState;

/// Configuration for the Telegram backend.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub supergroup_id: i64,
    pub scratchpad_topic_name: String,
    pub allowed_users: Vec<i64>,
    pub voice_stt_api_key: Option<String>,
    pub show_thinking: bool,
}

/// Per-task Telegram state (which topic, streaming state, reactions).
struct TaskTopicState {
    thread_id: Option<ThreadId>,
    streaming: StreamingState,
    #[allow(dead_code)]
    reactions: ReactionTracker,
}

impl TaskTopicState {
    fn new(thread_id: Option<ThreadId>) -> Self {
        Self {
            thread_id,
            streaming: StreamingState::default(),
            reactions: ReactionTracker::new(),
        }
    }
}

pub struct TelegramBackend {
    config: TelegramConfig,
}

impl TelegramBackend {
    pub fn new(config: TelegramConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl MessagingBackend for TelegramBackend {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn run(
        &self,
        mut orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()> {
        let bot = Bot::new(&self.config.bot_token);
        let group_id = ChatId(self.config.supergroup_id);

        // Map of task_id (String) → topic state.
        let task_states: Arc<Mutex<HashMap<String, TaskTopicState>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Map of thread_id (i32) → task_id (String) for reverse lookup.
        let thread_to_task: Arc<Mutex<HashMap<i32, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        info!("telegram: backend started for group {group_id}");

        // --- Incoming message handler ---
        {
            let bot_clone = bot.clone();
            let sender = backend_sender.clone();
            let allowed = self.config.allowed_users.clone();
            let t2t = Arc::clone(&thread_to_task);

            tokio::spawn(async move {
                let handler = teloxide::dptree::entry().branch(
                    teloxide::dptree::filter(|msg: Message| msg.from.is_some()).endpoint(
                        move |msg: Message| {
                            let sender = sender.clone();
                            let allowed = allowed.clone();
                            let t2t = Arc::clone(&t2t);
                            async move {
                                handle_incoming(msg, sender, &allowed, &t2t).await;
                                Ok::<_, anyhow::Error>(())
                            }
                        },
                    ),
                );

                teloxide::dispatching::Dispatcher::builder(bot_clone, handler)
                    .build()
                    .dispatch()
                    .await;
            });
        }

        // --- Orchestrator event loop ---
        loop {
            let event = match orchestrator_events.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("telegram: lagged by {n} events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("telegram: orchestrator channel closed");
                    return Ok(());
                }
            };

            let mut states = task_states.lock().await;
            let mut t2t = thread_to_task.lock().await;
            handle_orch_event(&bot, group_id, &event, &mut states, &mut t2t).await;
        }
    }
}

async fn handle_orch_event(
    bot: &Bot,
    group_id: ChatId,
    event: &OrchestratorEvent,
    states: &mut HashMap<String, TaskTopicState>,
    thread_to_task: &mut HashMap<i32, String>,
) {
    match event {
        OrchestratorEvent::TaskCreated { task_id, name, .. } => {
            match crate::topics::create_task_topic(bot, group_id, name).await {
                Ok(thread_id) => {
                    // ThreadId wraps MessageId which wraps i32
                    let tid_i32 = thread_id.0 .0;
                    states.insert(task_id.0.clone(), TaskTopicState::new(Some(thread_id)));
                    thread_to_task.insert(tid_i32, task_id.0.clone());
                    info!("telegram: created topic {tid_i32} for task {task_id}");
                }
                Err(e) => {
                    error!("telegram: failed to create topic for {task_id}: {e}");
                    states.insert(task_id.0.clone(), TaskTopicState::new(None));
                }
            }
        }

        OrchestratorEvent::PhaseChanged {
            task_id: _,
            phase,
            trigger_message,
        } => {
            if let Some(msg_ref) = trigger_message {
                let emoji = phase.emoji();
                debug!(
                    "telegram: phase {phase:?} ({emoji}) for trigger {}",
                    msg_ref.opaque_id
                );
                // setMessageReaction — teloxide 0.13 doesn't expose this yet; skip.
            }
        }

        OrchestratorEvent::TextOutput {
            task_id,
            text,
            is_continuation,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;

            if *is_continuation {
                if state.streaming.current_message_id.is_some() {
                    // For simplicity, just send a new message rather than editing.
                    let msg_id = send_text(bot, group_id, thread_id, text, false).await;
                    if let Some(id) = msg_id {
                        state.streaming.new_message(id);
                    }
                } else {
                    let msg_id = send_text(bot, group_id, thread_id, text, false).await;
                    if let Some(id) = msg_id {
                        state.streaming.new_message(id);
                    }
                }
            } else {
                let msg_id = send_text(bot, group_id, thread_id, text, false).await;
                if let Some(id) = msg_id {
                    state.streaming.new_message(id);
                }
            }
            state.streaming.append(text.len());
        }

        OrchestratorEvent::ToolStarted {
            task_id,
            tool_name,
            summary,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let text = format_tool_started(tool_name, summary);
            let msg_id = send_text(bot, group_id, thread_id, &text, false).await;
            if let Some(id) = msg_id {
                state.streaming.current_tool_message_id = Some(id);
            }
        }

        OrchestratorEvent::ToolCompleted {
            task_id,
            tool_name,
            summary,
            is_error,
            output_preview,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let text =
                format_tool_completed(tool_name, summary, *is_error, output_preview.as_deref());
            if let Some(msg_id) = state.streaming.current_tool_message_id {
                let _ = bot
                    .edit_message_text(group_id, MessageId(msg_id), &text)
                    .await;
            } else {
                send_text(bot, group_id, thread_id, &text, false).await;
            }
        }

        OrchestratorEvent::Thinking { task_id, text } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let msg = format_thinking(text);
            send_text(bot, group_id, thread_id, &msg, true).await;
        }

        OrchestratorEvent::TurnComplete {
            task_id,
            usage,
            duration_secs,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            state.streaming.reset();
            let text = format_turn_complete(
                *duration_secs,
                usage.total_cost_usd,
                usage.input_tokens,
                usage.output_tokens,
            );
            send_text(bot, group_id, thread_id, &text, false).await;
        }

        OrchestratorEvent::TaskStateChanged {
            task_id, new_state, ..
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let text = match new_state {
                TaskStateSummary::Hibernated => format_hibernated().to_string(),
                TaskStateSummary::Dead => "💀 Task stopped.".to_string(),
                TaskStateSummary::Running => "🟢 Task resumed.".to_string(),
            };
            send_text(bot, group_id, thread_id, &text, false).await;
        }

        OrchestratorEvent::Error {
            task_id,
            error,
            next_steps,
        } => {
            let thread_id = task_id
                .as_ref()
                .and_then(|id| states.get(&id.0))
                .and_then(|s| s.thread_id);
            let text = format_error(error, next_steps);
            send_text(bot, group_id, thread_id, &text, false).await;
        }

        OrchestratorEvent::FileOutput {
            task_id,
            filename,
            data,
            mime_type: _,
            caption,
        } => {
            let thread_id = states.get(&task_id.0).and_then(|s| s.thread_id);
            let _ = crate::files::send_document(
                bot,
                group_id,
                thread_id,
                Arc::clone(data),
                filename,
                caption.as_deref(),
            )
            .await;
        }

        OrchestratorEvent::CommandResponse { task_id, text } => {
            let thread_id = task_id
                .as_ref()
                .and_then(|id| states.get(&id.0))
                .and_then(|s| s.thread_id);
            send_text(bot, group_id, thread_id, text, false).await;
        }

        OrchestratorEvent::QueuedMessageDelivered {
            task_id,
            original_ref: _,
        } => {
            debug!("telegram: queued message delivered for {}", task_id.0);
        }
    }
}

async fn send_text(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    text: &str,
    _markdown: bool,
) -> Option<i32> {
    let mut req = bot.send_message(chat_id, text);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    match req.await {
        Ok(msg) => Some(msg.id.0),
        Err(e) => {
            warn!("telegram: send_message failed: {e}");
            None
        }
    }
}

async fn handle_incoming(
    msg: Message,
    sender: mpsc::Sender<BackendEvent>,
    allowed_users: &[i64],
    thread_to_task: &Arc<Mutex<HashMap<i32, String>>>,
) {
    let from = match &msg.from {
        Some(u) => u,
        None => return,
    };

    // Check user is allowed.
    if !allowed_users.is_empty() && !allowed_users.contains(&(from.id.0 as i64)) {
        return;
    }

    let user_id = from.id.0.to_string();
    let source = BackendSource::new("telegram", &user_id);
    let msg_id = msg.id.0.to_string();
    let msg_ref = MessageRef::new("telegram", &msg_id);

    // Determine which task this message belongs to via thread_id.
    let task_id_str = if let Some(tid) = msg.thread_id {
        let t2t = thread_to_task.lock().await;
        t2t.get(&tid.0 .0)
            .cloned()
            .unwrap_or_else(|| "scratchpad".to_string())
    } else {
        "scratchpad".to_string()
    };

    let task_id = TaskId(task_id_str);

    // Check for commands.
    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            match claude_events::parse_command(text) {
                Ok(cmd) => {
                    let _ = sender
                        .send(BackendEvent::Command {
                            command: cmd,
                            task_id: Some(task_id),
                            message_ref: msg_ref,
                            source,
                        })
                        .await;
                }
                Err(_) => {
                    // Unknown slash command — treat as text.
                    let _ = sender
                        .send(BackendEvent::UserMessage {
                            task_id,
                            text: text.to_string(),
                            message_ref: msg_ref,
                            source,
                        })
                        .await;
                }
            }
        } else {
            let _ = sender
                .send(BackendEvent::UserMessage {
                    task_id,
                    text: text.to_string(),
                    message_ref: msg_ref,
                    source,
                })
                .await;
        }
    }
}
