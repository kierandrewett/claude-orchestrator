use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use backend_traits::MessagingBackend;
use claude_events::{
    BackendEvent, BackendSource, MessageRef, OrchestratorEvent, SessionPhase, TaskId,
    TaskStateSummary,
};
use teloxide::prelude::*;
use teloxide::types::{ChatId, MessageId, MessageReactionUpdated, ReactionType, ThreadId};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::formatting::{
    format_error, format_hibernated, format_thinking, format_tool_completed, format_tool_started,
    format_turn_complete, md_to_telegram_html,
};
use crate::reactions::{apply_reaction, clear_reaction, ReactionTracker};
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
    pub state_dir: std::path::PathBuf,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TelegramState {
    scratchpad_thread_id: Option<i32>,
}

impl TelegramState {
    fn load(state_dir: &std::path::Path) -> Self {
        let path = state_dir.join("telegram_state.json");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, state_dir: &std::path::Path) {
        let path = state_dir.join("telegram_state.json");
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Per-task Telegram state (which topic, streaming state, reactions).
struct TaskTopicState {
    thread_id: Option<ThreadId>,
    streaming: StreamingState,
    #[allow(dead_code)]
    reactions: ReactionTracker,
    /// Messages queued while Claude was busy: (Telegram message_id, MessageRef)
    queued_messages: Vec<(i32, MessageRef)>,
    /// Telegram message_id of the message Claude is currently processing.
    processing_msg_id: Option<i32>,
    /// Tool name/summary saved from ToolStarted (ToolCompleted has empty fields).
    pending_tool_name: String,
    pending_tool_summary: String,
    /// Accumulated HTML for the current tool group message.
    tool_group_text: String,
    /// Byte offset in `tool_group_text` where the last pending tool entry starts.
    /// On ToolCompleted we truncate here and append the completed version.
    last_tool_start_offset: usize,
    /// The most recent bot message ID sent in this task's topic.
    /// Used as fallback reply target when trigger_ref is cleared (e.g. second turn).
    last_bot_message_id: Option<i32>,
}

impl TaskTopicState {
    fn new(thread_id: Option<ThreadId>) -> Self {
        Self {
            thread_id,
            streaming: StreamingState::default(),
            reactions: ReactionTracker::new(),
            queued_messages: Vec::new(),
            processing_msg_id: None,
            pending_tool_name: String::new(),
            pending_tool_summary: String::new(),
            tool_group_text: String::new(),
            last_tool_start_offset: 0,
            last_bot_message_id: None,
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
        let state_dir = self.config.state_dir.clone();

        // Load persisted state.
        let persisted = TelegramState::load(&state_dir);

        // Map of task_id (String) → topic state.
        let task_states: Arc<Mutex<HashMap<String, TaskTopicState>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Map of thread_id (i32) → task_id (String) for reverse lookup.
        let thread_to_task: Arc<Mutex<HashMap<i32, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Restore scratchpad from persisted state, verifying the topic still exists.
        let needs_init = if let Some(tid) = persisted.scratchpad_thread_id {
            use teloxide::prelude::Requester;
            use teloxide::types::MessageId;
            // Probe by sending a silent test message; delete it immediately if it lands.
            let thread_id = ThreadId(MessageId(tid));
            let probe = bot
                .send_message(group_id, ".")
                .message_thread_id(thread_id)
                .disable_notification(true)
                .await;
            match probe {
                Ok(m) => {
                    let _ = bot.delete_message(group_id, m.id).await;
                    task_states.lock().await.insert("scratchpad".to_string(), TaskTopicState::new(Some(thread_id)));
                    thread_to_task.lock().await.insert(tid, "scratchpad".to_string());
                    info!("telegram: restored scratchpad topic thread_id={tid}");
                    false
                }
                Err(e) => {
                    warn!("telegram: scratchpad topic {tid} probe failed ({e}) — invalidating persisted state, /init required");
                    TelegramState::default().save(&state_dir);
                    true
                }
            }
        } else {
            true
        };

        info!("telegram: backend started for group {group_id}");

        // --- Startup welcome message (only shown when /init hasn't been run yet) ---
        if needs_init {
            let bot_clone = bot.clone();
            tokio::spawn(async move {
                use teloxide::prelude::Requester;
                let text = "👋 <b>Claude Orchestrator</b> is online.\n\nRun /init to create the Scratchpad topic and register channels.";
                if let Err(e) = bot_clone
                    .send_message(group_id, text)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await
                {
                    warn!("telegram: failed to send welcome message: {e}");
                }
            });
        }

        // --- Incoming message + reaction handler ---
        {
            let bot_clone = bot.clone();
            let sender = backend_sender.clone();
            let allowed = self.config.allowed_users.clone();
            let t2t = Arc::clone(&thread_to_task);
            let task_states_clone = Arc::clone(&task_states);
            let scratchpad_name = self.config.scratchpad_topic_name.clone();
            let state_dir_clone = state_dir.clone();

            tokio::spawn(async move {
                let message_handler = {
                    let bot_msg = bot_clone.clone();
                    let sender = sender.clone();
                    let allowed = allowed.clone();
                    let t2t = Arc::clone(&t2t);
                    let ts = Arc::clone(&task_states_clone);
                    let sp_name = scratchpad_name.clone();
                    let sd = state_dir_clone.clone();
                    move |msg: Message| {
                        let bot_msg = bot_msg.clone();
                        let sender = sender.clone();
                        let allowed = allowed.clone();
                        let t2t = Arc::clone(&t2t);
                        let ts = Arc::clone(&ts);
                        let sp_name = sp_name.clone();
                        let sd = sd.clone();
                        async move {
                            handle_incoming(msg, bot_msg, group_id, sender, &allowed, &t2t, &ts, &sp_name, &sd).await;
                            Ok::<_, anyhow::Error>(())
                        }
                    }
                };

                let reaction_handler = {
                    let sender = sender.clone();
                    let allowed = allowed.clone();
                    let task_states_react = Arc::clone(&task_states_clone);
                    move |reaction: MessageReactionUpdated| {
                        let sender = sender.clone();
                        let allowed = allowed.clone();
                        let task_states_react = Arc::clone(&task_states_react);
                        async move {
                            handle_reaction(reaction, sender, &allowed, &task_states_react).await;
                            Ok::<_, anyhow::Error>(())
                        }
                    }
                };

                let handler = dptree::entry()
                    .branch(
                        Update::filter_message()
                            .branch(
                                dptree::filter(|msg: Message| msg.from.is_some())
                                    .endpoint(message_handler),
                            ),
                    )
                    .branch(
                        Update::filter_message_reaction_updated().endpoint(reaction_handler),
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
            task_id,
            phase,
            trigger_message,
        } => {
            if let Some(msg_ref) = trigger_message {
                if msg_ref.backend == "telegram" {
                    if let Ok(msg_id) = msg_ref.opaque_id.parse::<i32>() {
                        // Track which message is currently being processed.
                        if *phase == SessionPhase::Responding {
                            let state = states
                                .entry(task_id.0.clone())
                                .or_insert_with(|| TaskTopicState::new(None));
                            state.processing_msg_id = Some(msg_id);
                        }
                        // Apply emoji reaction for the current phase.
                        apply_reaction(bot, group_id, MessageId(msg_id), phase.emoji()).await;
                    }
                }
            }
        }

        OrchestratorEvent::MessageQueued {
            task_id,
            message_ref,
        } => {
            if message_ref.backend == "telegram" {
                if let Ok(msg_id) = message_ref.opaque_id.parse::<i32>() {
                    let state = states
                        .entry(task_id.0.clone())
                        .or_insert_with(|| TaskTopicState::new(None));
                    // Track the queued message so we can handle ❌ reactions later.
                    state
                        .queued_messages
                        .push((msg_id, message_ref.clone()));
                    // Apply 🤔 reaction to let the user know their message is queued.
                    apply_reaction(bot, group_id, MessageId(msg_id), "🤔").await;
                    debug!("telegram: applied 🤔 reaction to queued message {msg_id}");
                }
            }
        }

        OrchestratorEvent::TextOutput {
            task_id,
            text,
            is_continuation,
            trigger_ref,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            // Reply to the triggering user message on the first chunk of a new response,
            // falling back to the last bot message when trigger_ref has been cleared.
            let reply_to = if !is_continuation {
                telegram_msg_id(trigger_ref).or(state.last_bot_message_id)
            } else {
                None
            };
            // New response block — reset tool group so the next tools start a fresh message.
            if !is_continuation {
                state.tool_group_text.clear();
                state.streaming.current_tool_message_id = None;
            }
            if state.streaming.should_start_new_message(text.len()) {
                // First chunk (or overflow) — send a new message.
                let html = md_to_telegram_html(text);
                if html.len() > 4000 {
                    // Too long for a Telegram message — send as a markdown file.
                    let data = std::sync::Arc::new(text.as_bytes().to_vec());
                    let _ = crate::files::send_document(
                        bot, group_id, reply_to, data, "response.md", None,
                    ).await;
                    // Don't track in streaming; TurnComplete will send stats as a reply.
                } else {
                    let msg_id = send_text_reply(bot, group_id, thread_id, &html, reply_to, false).await;
                    if let Some(id) = msg_id {
                        state.streaming.new_message(id, text); // store raw markdown
                        state.last_bot_message_id = Some(id);
                    }
                }
            } else {
                // Subsequent chunk — accumulate raw markdown then edit the message.
                state.streaming.append(text);
                let full_html = md_to_telegram_html(&state.streaming.current_text);
                if let Some(msg_id) = state.streaming.current_message_id {
                    use teloxide::prelude::Requester;
                    let _ = bot
                        .edit_message_text(group_id, MessageId(msg_id), &full_html)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await;
                }
            }
        }

        OrchestratorEvent::ToolStarted {
            task_id,
            tool_name,
            summary,
            trigger_ref,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            // Save for use in ToolCompleted (which has empty name/summary).
            state.pending_tool_name = tool_name.clone();
            state.pending_tool_summary = summary.clone();
            // Force next TextOutput to start a fresh message instead of appending.
            state.streaming.reset_text();

            let entry = format_tool_started(tool_name, summary);

            // Append to the tool group, recording where this entry starts so
            // ToolCompleted can replace just this entry with the completed version.
            if !state.tool_group_text.is_empty() {
                state.tool_group_text.push('\n');
            }
            state.last_tool_start_offset = state.tool_group_text.len();
            state.tool_group_text.push_str(&entry);

            let reply_to = telegram_msg_id(trigger_ref).or(state.last_bot_message_id);
            // Apply 👨‍💻 reaction to the triggering user message.
            if let Some(mid) = telegram_msg_id(trigger_ref) {
                apply_reaction(bot, group_id, MessageId(mid), "👨‍💻").await;
            }

            if let Some(msg_id) = state.streaming.current_tool_message_id {
                // Edit existing group message to append the new entry.
                use teloxide::prelude::Requester;
                let group_text = state.tool_group_text.clone();
                if let Err(e) = bot
                    .edit_message_text(group_id, MessageId(msg_id), &group_text)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await
                {
                    warn!("telegram: tool group edit failed (msg {msg_id}): {e}");
                }
            } else {
                // Send new tool group message.
                let group_text = state.tool_group_text.clone();
                let msg_id = send_text_reply(bot, group_id, thread_id, &group_text, reply_to, true).await;
                if let Some(id) = msg_id {
                    state.streaming.current_tool_message_id = Some(id);
                    state.last_bot_message_id = Some(id);
                }
            }
        }

        OrchestratorEvent::ToolCompleted {
            task_id,
            is_error,
            output_preview,
            ..
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let name = std::mem::take(&mut state.pending_tool_name);
            let summary = std::mem::take(&mut state.pending_tool_summary);
            let completed_entry =
                format_tool_completed(&name, &summary, *is_error, output_preview.as_deref());

            // Replace the pending entry (from last_tool_start_offset onward) with the
            // completed version — this handles multi-tool groups correctly.
            state.tool_group_text.truncate(state.last_tool_start_offset);
            state.tool_group_text.push_str(&completed_entry);
            let group_text = state.tool_group_text.clone();

            if let Some(msg_id) = state.streaming.current_tool_message_id {
                use teloxide::prelude::Requester;
                if let Err(e) = bot
                    .edit_message_text(group_id, MessageId(msg_id), &group_text)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await
                {
                    warn!("telegram: tool edit failed (msg {msg_id}): {e}");
                    send_text_reply(bot, group_id, thread_id, &completed_entry, None, true).await;
                }
            } else {
                warn!("telegram: ToolCompleted has no tool_message_id for task {}", task_id.0);
                send_text_reply(bot, group_id, thread_id, &completed_entry, None, true).await;
            }
        }

        OrchestratorEvent::Thinking { task_id, text, .. } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;
            let msg = format_thinking(text);
            send_text_reply(bot, group_id, thread_id, &msg, None, true).await;
        }

        OrchestratorEvent::TurnComplete {
            task_id,
            usage,
            duration_secs,
            trigger_ref,
        } => {
            let state = states
                .entry(task_id.0.clone())
                .or_insert_with(|| TaskTopicState::new(None));
            let thread_id = state.thread_id;

            let stats = format_turn_complete(*duration_secs);

            // Apply 👍 to the message being processed (processing_msg_id is more reliable
            // than trigger_ref which may have been cleared by ClaudeIdle already).
            let reaction_target = state.processing_msg_id
                .or_else(|| telegram_msg_id(trigger_ref));
            if let Some(mid) = reaction_target {
                apply_reaction(bot, group_id, MessageId(mid), "👍").await;
            }
            state.processing_msg_id = None;

            // Append duration to the last response message.
            let last_msg_id = state.streaming.current_message_id;
            let last_raw = state.streaming.current_text.clone();
            state.streaming.reset();

            if let Some(msg_id) = last_msg_id {
                use teloxide::prelude::Requester;
                let last_html = md_to_telegram_html(&last_raw);
                let edited = format!("{last_html}\n{stats}");
                if edited.len() > 4096 {
                    // Too long to edit — append stats as a separate reply.
                    let reply_to = Some(msg_id);
                    send_text_reply(bot, group_id, thread_id, &stats, reply_to, false).await;
                } else if let Err(_) = bot.edit_message_text(group_id, MessageId(msg_id), &edited)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await
                {
                    let reply_to = telegram_msg_id(trigger_ref);
                    send_text_reply(bot, group_id, thread_id, &stats, reply_to, false).await;
                }
            } else {
                let reply_to = telegram_msg_id(trigger_ref).or(state.last_bot_message_id);
                send_text_reply(bot, group_id, thread_id, &stats, reply_to, false).await;
            }
        }

        OrchestratorEvent::QueuedMessageDelivered {
            task_id,
            original_ref,
        } => {
            // The queued message was delivered to Claude — remove ⏰ reaction.
            if original_ref.backend == "telegram" {
                if let Ok(msg_id) = original_ref.opaque_id.parse::<i32>() {
                    let state = states
                        .entry(task_id.0.clone())
                        .or_insert_with(|| TaskTopicState::new(None));
                    state.queued_messages.retain(|(id, _)| *id != msg_id);
                    clear_reaction(bot, group_id, MessageId(msg_id)).await;
                    debug!("telegram: cleared ⏰ reaction from delivered message {msg_id}");
                }
            }
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
            send_text_reply(bot, group_id, thread_id, &text, None, false).await;
        }

        OrchestratorEvent::Error {
            task_id,
            error,
            next_steps,
            trigger_ref,
        } => {
            let thread_id = task_id
                .as_ref()
                .and_then(|id| states.get(&id.0))
                .and_then(|s| s.thread_id);
            let reply_to = telegram_msg_id(trigger_ref);
            // Apply 🤬 reaction to the message that caused the error.
            if let Some(mid) = reply_to {
                apply_reaction(bot, group_id, MessageId(mid), "🤬").await;
            }
            let text = format_error(error, next_steps);
            send_text_reply(bot, group_id, thread_id, &text, reply_to, false).await;
        }

        OrchestratorEvent::FileOutput {
            task_id,
            filename,
            data,
            mime_type: _,
            caption,
        } => {
            // Use the last streaming message as reply target to thread the document correctly.
            let reply_to = states.get(&task_id.0).and_then(|s| {
                s.streaming.current_message_id
                    .or(s.processing_msg_id)
            });
            let _ = crate::files::send_document(
                bot,
                group_id,
                reply_to,
                Arc::clone(data),
                filename,
                caption.as_deref(),
            )
            .await;
        }

        OrchestratorEvent::CommandResponse {
            task_id,
            text,
            trigger_ref,
        } => {
            let thread_id = task_id
                .as_ref()
                .and_then(|id| states.get(&id.0))
                .and_then(|s| s.thread_id);
            let reply_to = telegram_msg_id(trigger_ref);
            send_text_reply(bot, group_id, thread_id, text, reply_to, false).await;
        }
    }
}

/// Send a message, optionally replying to a specific message ID.
/// Pass `silent = true` to suppress the notification sound/banner.
async fn send_text_reply(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    text: &str,
    reply_to_message_id: Option<i32>,
    silent: bool,
) -> Option<i32> {
    let mut req = bot
        .send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .disable_notification(silent);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Some(reply_id) = reply_to_message_id {
        use teloxide::types::ReplyParameters;
        req = req.reply_parameters(ReplyParameters::new(MessageId(reply_id)));
    }
    match req.await {
        Ok(msg) => Some(msg.id.0),
        Err(e) => {
            warn!("telegram: send_message failed: {e}");
            None
        }
    }
}

/// Extract a Telegram message ID from a MessageRef, if it belongs to the telegram backend.
fn telegram_msg_id(msg_ref: &Option<MessageRef>) -> Option<i32> {
    msg_ref.as_ref().and_then(|r| {
        if r.backend == "telegram" {
            r.opaque_id.parse::<i32>().ok()
        } else {
            None
        }
    })
}

async fn handle_incoming(
    msg: Message,
    bot: Bot,
    group_id: ChatId,
    sender: mpsc::Sender<BackendEvent>,
    allowed_users: &[i64],
    thread_to_task: &Arc<Mutex<HashMap<i32, String>>>,
    task_states: &Arc<Mutex<HashMap<String, TaskTopicState>>>,
    scratchpad_topic_name: &str,
    state_dir: &std::path::Path,
) {
    let from = match &msg.from {
        Some(u) => u,
        None => return,
    };

    if !allowed_users.is_empty() && !allowed_users.contains(&(from.id.0 as i64)) {
        return;
    }

    let user_id = from.id.0.to_string();
    let source = BackendSource::new("telegram", &user_id);
    let msg_id = msg.id.0.to_string();
    let msg_ref = MessageRef::new("telegram", &msg_id);

    // Handle /init before anything else — it's backend-local, not routed to the orchestrator.
    if msg.text() == Some("/init") {
        let already_init = task_states.lock().await.contains_key("scratchpad");
        if already_init {
            send_text_reply(&bot, group_id, msg.thread_id, "ℹ️ Already initialised — Scratchpad topic is registered and ready.", None, false).await;
        } else {
            match crate::topics::create_scratchpad_topic(&bot, group_id, scratchpad_topic_name).await {
                Ok(thread_id) => {
                    let tid_i32 = thread_id.0.0;
                    thread_to_task.lock().await.insert(tid_i32, "scratchpad".to_string());
                    task_states.lock().await.insert(
                        "scratchpad".to_string(),
                        TaskTopicState::new(Some(thread_id)),
                    );
                    TelegramState { scratchpad_thread_id: Some(tid_i32) }.save(state_dir);
                    {
                        use teloxide::prelude::Requester;
                        use teloxide::types::ParseMode;
                        let text = format!(
                            "✅ <b>{}</b> topic created. Send messages here to chat with Claude.\n\n<i>Tip: long-press the topic and tap Pin to keep it at the top.</i>",
                            scratchpad_topic_name
                        );
                        if let Err(e) = bot.send_message(group_id, text)
                            .parse_mode(ParseMode::Html)
                            .message_thread_id(thread_id)
                            .await
                        {
                            warn!("telegram: failed to send /init confirmation: {e}");
                        }
                    }
                    info!("telegram: scratchpad topic created, thread_id={tid_i32}");
                }
                Err(e) => {
                    error!("telegram: /init failed to create scratchpad topic: {e}");
                    send_text_reply(&bot, group_id, None,
                        &format!("❌ Failed to create Scratchpad topic: {e}. Make sure the bot is an admin with topic management permissions."),
                        None, false,
                    ).await;
                }
            }
        }
        return;
    }

    // Only handle messages in known threads (scratchpad or task topics).
    // Ignore messages in the main group chat or unregistered topics.
    let task_id_str = match msg.thread_id {
        Some(tid) => {
            let t2t = thread_to_task.lock().await;
            match t2t.get(&tid.0 .0).cloned() {
                Some(id) => id,
                None => return, // unknown topic — ignore
            }
        }
        None => return, // main group chat — ignore
    };

    let task_id = TaskId(task_id_str);

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
                    // Unknown slash command — don't forward to Claude.
                    debug!("telegram: ignoring unknown command: {text}");
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

/// Handles a reaction update — detects ❌ reactions added by the user and
/// either cancels a queued message or interrupts the active Claude turn.
async fn handle_reaction(
    reaction: MessageReactionUpdated,
    sender: mpsc::Sender<BackendEvent>,
    allowed_users: &[i64],
    task_states: &Arc<Mutex<HashMap<String, TaskTopicState>>>,
) {
    // Only act on reactions from allowed users.
    if let Some(user) = &reaction.user {
        if !allowed_users.is_empty() && !allowed_users.contains(&(user.id.0 as i64)) {
            return;
        }
    }

    // Check if ❌ was newly added (present in new_reaction but not old_reaction).
    let had_cancel = is_cancel_reaction(&reaction.old_reaction);
    let has_cancel = is_cancel_reaction(&reaction.new_reaction);
    if !has_cancel || had_cancel {
        return; // ❌ was not newly added
    }

    let msg_id = reaction.message_id.0;
    let user_id = reaction
        .user
        .as_ref()
        .map(|u| u.id.0.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let source = BackendSource::new("telegram", &user_id);

    // Search all task states to find which task this message belongs to.
    let states = task_states.lock().await;
    for (task_id_str, state) in states.iter() {
        // Check if it's a queued message.
        if let Some((_, msg_ref)) = state
            .queued_messages
            .iter()
            .find(|(id, _)| *id == msg_id)
        {
            let task_id = TaskId(task_id_str.clone());
            let msg_ref = msg_ref.clone();
            drop(states);
            info!("telegram: user ❌ on queued message {msg_id} — cancelling queue entry");
            let _ = sender
                .send(BackendEvent::CancelQueuedMessage {
                    task_id,
                    message_ref: msg_ref,
                    source,
                })
                .await;
            return;
        }

        // Check if it's the currently-processing message.
        if state.processing_msg_id == Some(msg_id) {
            let task_id = TaskId(task_id_str.clone());
            drop(states);
            info!("telegram: user ❌ on processing message {msg_id} — interrupting Claude");
            let _ = sender
                .send(BackendEvent::InterruptTask { task_id, source })
                .await;
            return;
        }
    }
}

/// Returns true if the reaction list contains the ❌ cancel emoji.
fn is_cancel_reaction(reactions: &[ReactionType]) -> bool {
    reactions.iter().any(|r| match r {
        ReactionType::Emoji { emoji } => emoji == "❌",
        _ => false,
    })
}
