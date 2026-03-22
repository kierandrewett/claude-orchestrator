//! Telegram bot provider.
//!
//! Starts when `TELEGRAM_BOT_TOKEN` is set. Each Telegram chat gets one Claude
//! session that lives for up to 12 hours. After that, the next message creates
//! a fresh session transparently.
//!
//! Commands:
//!   /start   — welcome message
//!   /task    — start a new focused Claude session with the given prompt, replies threaded
//!   /status  — show current session info
//!   /new     — force a new session immediately
//!
//! All other text is forwarded to the current Claude session. Slash commands
//! prefixed with `/` that aren't recognized bot commands are passed through to
//! Claude (e.g. `/compact`, `/help`).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use teloxide::{
    prelude::*,
    types::{MessageId, ReplyParameters},
    utils::command::BotCommands,
};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    protocol::{SessionInfo, SessionStats, SessionStatus, S2C, S2D},
    state::{AppState, SessionBuffer},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long a Claude session lives before a new one is created.
const SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);

/// How long to wait for Claude to respond before giving up.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);

/// Telegram's message character limit.
const TG_MSG_LIMIT: usize = 4000;

// ---------------------------------------------------------------------------
// Bot commands
// ---------------------------------------------------------------------------

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Claude Code Bot commands")]
enum Cmd {
    #[command(description = "Start chatting with Claude")]
    Start,
    #[command(description = "Start a new focused task: /task <your prompt>")]
    Task(String),
    #[command(description = "Show current session status")]
    Status,
    #[command(description = "Force a new Claude session")]
    New,
    #[command(description = "Stop Claude immediately")]
    Stop,
}

// ---------------------------------------------------------------------------
// Per-chat state
// ---------------------------------------------------------------------------

struct ChatState {
    session_id: String,
    /// If set, bot replies are threaded to this Telegram message (used for /task).
    task_reply_to: Option<MessageId>,
    created_at: Instant,
}

impl ChatState {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > SESSION_LIFETIME
    }
}

type ChatStates = Arc<RwLock<HashMap<i64, ChatState>>>;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Set of allowed Telegram user IDs. Empty means allow everyone.
type AllowedUsers = Arc<HashSet<u64>>;

pub async fn start(app_state: Arc<AppState>, token: String) {
    info!("telegram: starting bot");
    let bot = Bot::new(token);

    // Parse TELEGRAM_ALLOWED_USERS="123456789,987654321"
    let allowed: AllowedUsers = Arc::new(
        std::env::var("TELEGRAM_ALLOWED_USERS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse::<u64>().ok())
            .collect(),
    );

    if allowed.is_empty() {
        warn!("telegram: TELEGRAM_ALLOWED_USERS is not set — bot is open to everyone");
    } else {
        info!("telegram: allowlist has {} user(s)", allowed.len());
    }

    if let Err(e) = bot.set_my_commands(Cmd::bot_commands()).await {
        warn!("telegram: failed to register commands: {e}");
    }

    let states: ChatStates = Arc::new(RwLock::new(HashMap::new()));

    let handler = Update::filter_message()
        .filter(|msg: Message, allowed: AllowedUsers| {
            // Pass through if allowlist is empty OR user ID is in it
            let uid = msg.from().map(|u| u.id.0).unwrap_or(0);
            allowed.is_empty() || allowed.contains(&uid)
        })
        .branch(
            dptree::entry()
                .filter_command::<Cmd>()
                .endpoint(handle_command),
        )
        .branch(dptree::endpoint(handle_message));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![app_state, states, allowed])
        .build()
        .dispatch()
        .await;
}

// ---------------------------------------------------------------------------
// Command handler
// ---------------------------------------------------------------------------

async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Cmd,
    app_state: Arc<AppState>,
    states: ChatStates,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;

    match cmd {
        Cmd::Start => {
            bot.send_message(
                chat_id,
                "👋 Hi! I'm Claude Code.\n\nSend me any message to chat, or use /task <prompt> to kick off a focused coding task.\n\nYour session lasts up to 12 hours.",
            )
            .await?;
        }

        Cmd::Task(prompt) => {
            if prompt.trim().is_empty() {
                bot.send_message(
                    chat_id,
                    "Usage: /task <your prompt>\nExample: /task write a Python script that parses logs",
                )
                .await?;
                return Ok(());
            }

            // Subscribe before creating the session so we don't miss events.
            let sse_rx = app_state.sse_tx.subscribe();

            let session_id = create_session(&app_state, chat_id.0, Some(prompt)).await;
            let task_reply_to = msg.id;

            {
                let mut guard = states.write().await;
                guard.insert(
                    chat_id.0,
                    ChatState {
                        session_id: session_id.clone(),
                        task_reply_to: Some(task_reply_to),
                        created_at: Instant::now(),
                    },
                );
            }

            let _ = bot
                .send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
                .await;

            tokio::spawn(collect_and_send(
                bot,
                chat_id,
                Some(task_reply_to),
                session_id,
                sse_rx,
            ));
        }

        Cmd::Status => {
            let guard = states.read().await;
            if let Some(state) = guard.get(&chat_id.0).filter(|s| !s.is_expired()) {
                let sessions = app_state.sessions.read().await;
                if let Some(buf) = sessions.get(&state.session_id) {
                    let age = state.created_at.elapsed();
                    let remaining = SESSION_LIFETIME.as_secs().saturating_sub(age.as_secs());
                    let status_str = format!("{:?}", buf.info.status).to_lowercase();
                    let stats = &buf.info.stats;
                    bot.send_message(
                        chat_id,
                        format!(
                            "📊 *Session status*\nID: `{}`\nStatus: {}\nAge: {}h {}m\nExpires in: {}h {}m\nTokens: {}↑ {}↓\nCost: ${:.4}",
                            &state.session_id[..8],
                            status_str,
                            age.as_secs() / 3600,
                            (age.as_secs() % 3600) / 60,
                            remaining / 3600,
                            (remaining % 3600) / 60,
                            stats.input_tokens,
                            stats.output_tokens,
                            stats.cost_usd.unwrap_or(0.0),
                        ),
                    )
                    .await?;
                } else {
                    bot.send_message(chat_id, "No active session. Send a message to start one.")
                        .await?;
                }
            } else {
                bot.send_message(chat_id, "No active session. Send a message to start one.")
                    .await?;
            }
        }

        Cmd::New => {
            {
                let mut guard = states.write().await;
                guard.remove(&chat_id.0);
            }
            bot.send_message(
                chat_id,
                "✅ Session cleared. Your next message will start a fresh Claude session.",
            )
            .await?;
        }

        Cmd::Stop => {
            kill_current_session(&bot, chat_id, &app_state, &states).await?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Plain-message handler
// ---------------------------------------------------------------------------

async fn handle_message(
    bot: Bot,
    msg: Message,
    app_state: Arc<AppState>,
    states: ChatStates,
) -> ResponseResult<()> {
    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let chat_id = msg.chat.id;

    // "stop" safe word — kill immediately without forwarding to Claude.
    if text.trim().eq_ignore_ascii_case("stop") {
        return kill_current_session(&bot, chat_id, &app_state, &states).await;
    }

    // Subscribe BEFORE sending to avoid missing early streaming events.
    let sse_rx = app_state.sse_tx.subscribe();

    let (session_id, task_reply_to): (Option<String>, Option<MessageId>) = {
        let guard = states.read().await;
        guard
            .get(&chat_id.0)
            .filter(|s| !s.is_expired())
            .map(|s| (Some(s.session_id.clone()), s.task_reply_to))
            .unwrap_or((None, None))
    };

    let session_id = if let Some(id) = session_id {
        // If the session never started or has ended (killed/failed/completed),
        // discard it and create a fresh one.
        let is_dead = {
            let sessions = app_state.sessions.read().await;
            sessions
                .get(&id)
                .map(|b| {
                    matches!(
                        b.info.status,
                        SessionStatus::Pending
                            | SessionStatus::Completed
                            | SessionStatus::Failed
                            | SessionStatus::Killed
                    )
                })
                .unwrap_or(true)
        };

        if is_dead {
            warn!("telegram: session {id} is dead, creating fresh session");
            states.write().await.remove(&chat_id.0);
            let new_id = create_session(&app_state, chat_id.0, Some(text)).await;
            states.write().await.insert(
                chat_id.0,
                ChatState {
                    session_id: new_id.clone(),
                    task_reply_to: None,
                    created_at: Instant::now(),
                },
            );
            new_id
        } else {
            app_state
                .send_to_client(&S2C::SendInput {
                    session_id: id.clone(),
                    text,
                })
                .await;
            id
        }
    } else {
        // No active session — create one with the first message as initial prompt.
        let id = create_session(&app_state, chat_id.0, Some(text)).await;
        {
            let mut guard = states.write().await;
            guard.insert(
                chat_id.0,
                ChatState {
                    session_id: id.clone(),
                    task_reply_to: None,
                    created_at: Instant::now(),
                },
            );
        }
        id
    };

    let _ = bot
        .send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
        .await;

    tokio::spawn(collect_and_send(
        bot,
        chat_id,
        task_reply_to,
        session_id,
        sse_rx,
    ));

    Ok(())
}

// ---------------------------------------------------------------------------
// Session creation helper
// ---------------------------------------------------------------------------

async fn create_session(
    app_state: &Arc<AppState>,
    chat_id: i64,
    initial_prompt: Option<String>,
) -> String {
    let session_id = Uuid::new_v4().to_string();
    let claude_session_id = Uuid::new_v4().to_string();

    let hostname = {
        let guard = app_state.client.read().await;
        guard.as_ref().map(|c| c.hostname.clone())
    };

    let info = SessionInfo {
        id: session_id.clone(),
        name: Some(format!("Telegram {chat_id}")),
        cwd: String::new(),
        status: SessionStatus::Pending,
        created_at: Utc::now(),
        started_at: None,
        ended_at: None,
        stats: SessionStats::default(),
        client_hostname: hostname,
        claude_session_id: Some(claude_session_id.clone()),
    };

    app_state.store.save_session(&info).await.ok();

    {
        let mut guard = app_state.sessions.write().await;
        guard.insert(
            session_id.clone(),
            SessionBuffer {
                info: info.clone(),
                events: std::collections::VecDeque::new(),
            },
        );
    }

    app_state
        .broadcast(&S2D::SessionCreated { session: info })
        .await;

    app_state
        .send_to_client(&S2C::StartSession {
            session_id: session_id.clone(),
            initial_prompt,
            extra_args: Vec::new(),
            claude_session_id,
            is_resume: false,
        })
        .await;

    session_id
}

// ---------------------------------------------------------------------------
// Stop / kill helper
// ---------------------------------------------------------------------------

/// Kills the active session for this chat (if any) and clears the chat state.
async fn kill_current_session(
    bot: &Bot,
    chat_id: ChatId,
    app_state: &Arc<AppState>,
    states: &ChatStates,
) -> ResponseResult<()> {
    let session_id = {
        let guard = states.read().await;
        guard
            .get(&chat_id.0)
            .filter(|s| !s.is_expired())
            .map(|s| s.session_id.clone())
    };

    if let Some(id) = session_id {
        // Only kill sessions that are actually running.
        let is_running = {
            let sessions = app_state.sessions.read().await;
            sessions
                .get(&id)
                .map(|b| matches!(b.info.status, crate::protocol::SessionStatus::Running))
                .unwrap_or(false)
        };

        if is_running {
            info!("telegram: killing session {id} on user request");
            app_state
                .send_to_client(&S2C::KillSession {
                    session_id: id.clone(),
                })
                .await;
            states.write().await.remove(&chat_id.0);
            bot.send_message(chat_id, "🛑 Stopped.").await?;
        } else {
            bot.send_message(chat_id, "Nothing is running right now.")
                .await?;
        }
    } else {
        bot.send_message(chat_id, "Nothing is running right now.")
            .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Response collector
// ---------------------------------------------------------------------------

/// Subscribes to the broadcast channel, accumulates Claude's text response for
/// `session_id`, then sends it to Telegram (optionally as a reply).
///
/// While Claude is working, tool calls are displayed as a live-edited status
/// message. When the final text response arrives it is sent as a new message.
async fn collect_and_send(
    bot: Bot,
    chat_id: ChatId,
    reply_to: Option<MessageId>,
    session_id: String,
    mut sse_rx: broadcast::Receiver<String>,
) {
    info!("telegram: collect_and_send waiting for response on session {session_id}");
    let deadline = tokio::time::Instant::now() + RESPONSE_TIMEOUT;

    // Accumulated final text response.
    let mut text = String::new();

    // Live tool-call tracking.
    // Each entry is (tool_name, partial_input_json).
    let mut completed_tools: Vec<(String, String)> = Vec::new();
    // The tool currently being streamed.
    let mut current_tool: Option<(String, String)> = None;
    // The Telegram message used to show live tool status (created on first tool call).
    let mut status_msg_id: Option<MessageId> = None;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            warn!("telegram: response timeout for session {session_id}");
            break;
        }

        match tokio::time::timeout(remaining, sse_rx.recv()).await {
            Ok(Ok(raw)) => {
                let Ok(s2d) = serde_json::from_str::<S2D>(&raw) else {
                    continue;
                };
                match s2d {
                    S2D::SessionEvent {
                        session_id: sid,
                        event,
                    } if sid == session_id => {
                        let evt_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        match evt_type {
                            // Streaming events are wrapped in stream_event — unwrap and
                            // extract text deltas and tool call info from inner events.
                            "stream_event" => {
                                if let Some(inner) = event.get("event") {
                                    let inner_type =
                                        inner.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                    match inner_type {
                                        "content_block_start" => {
                                            // Detect start of a tool_use block.
                                            if inner
                                                .pointer("/content_block/type")
                                                .and_then(|t| t.as_str())
                                                == Some("tool_use")
                                            {
                                                let name = inner
                                                    .pointer("/content_block/name")
                                                    .and_then(|n| n.as_str())
                                                    .unwrap_or("unknown")
                                                    .to_string();
                                                current_tool = Some((name, String::new()));
                                            }
                                        }
                                        "content_block_delta" => {
                                            // Text delta → accumulate response text.
                                            if let Some(t) = inner
                                                .pointer("/delta/text")
                                                .and_then(|t| t.as_str())
                                            {
                                                text.push_str(t);
                                            }
                                            // Input JSON delta → accumulate tool input.
                                            if let Some(partial) = inner
                                                .pointer("/delta/partial_json")
                                                .and_then(|t| t.as_str())
                                            {
                                                if let Some((_, ref mut input)) = current_tool {
                                                    input.push_str(partial);
                                                }
                                            }
                                        }
                                        "content_block_stop" => {
                                            // Finalise current tool call and update status.
                                            if let Some(tool) = current_tool.take() {
                                                completed_tools.push(tool);
                                                status_msg_id = update_tool_status(
                                                    &bot,
                                                    chat_id,
                                                    reply_to,
                                                    &completed_tools,
                                                    status_msg_id,
                                                    false,
                                                )
                                                .await;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            // result marks the end of the entire Claude turn (incl. all tool use).
                            "result" => {
                                info!(
                                    "telegram: session {session_id} result, sending {} chars",
                                    text.len()
                                );
                                // Mark the tool status message as done.
                                if status_msg_id.is_some() && !completed_tools.is_empty() {
                                    update_tool_status(
                                        &bot,
                                        chat_id,
                                        reply_to,
                                        &completed_tools,
                                        status_msg_id,
                                        true,
                                    )
                                    .await;
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                    S2D::SessionEnded {
                        session_id: sid, ..
                    } if sid == session_id => {
                        info!(
                            "telegram: session {session_id} ended, sending {} chars",
                            text.len()
                        );
                        break;
                    }
                    _ => {}
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                warn!("telegram: collect_and_send lagged by {n} messages for session {session_id}");
                continue;
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                warn!("telegram: broadcast channel closed for session {session_id}");
                break;
            }
            Err(_) => {
                warn!("telegram: timeout waiting for session {session_id}");
                break;
            }
        }
    }

    if text.is_empty() {
        info!("telegram: no text to send for session {session_id}");
        return;
    }

    for chunk in split_message(&text) {
        let mut req = bot.send_message(chat_id, &chunk);
        if let Some(rid) = reply_to {
            req = req.reply_parameters(ReplyParameters::new(rid));
        }
        if let Err(e) = req.await {
            warn!("telegram: failed to send message: {e}");
        }
    }
}

/// Sends or edits the live tool-status message.
///
/// Returns the `MessageId` of the status message so the caller can pass it
/// back on the next call for editing.
async fn update_tool_status(
    bot: &Bot,
    chat_id: ChatId,
    reply_to: Option<MessageId>,
    tools: &[(String, String)],
    existing_id: Option<MessageId>,
    done: bool,
) -> Option<MessageId> {
    let body = format_tool_status(tools, done);

    if let Some(mid) = existing_id {
        match bot.edit_message_text(chat_id, mid, &body).await {
            Ok(_) => return Some(mid),
            Err(e) => {
                warn!("telegram: failed to edit tool status message: {e}");
                return Some(mid);
            }
        }
    }

    // No existing message — send a new one.
    let mut req = bot.send_message(chat_id, &body);
    if let Some(rid) = reply_to {
        req = req.reply_parameters(ReplyParameters::new(rid));
    }
    match req.await {
        Ok(msg) => Some(msg.id),
        Err(e) => {
            warn!("telegram: failed to send tool status message: {e}");
            None
        }
    }
}

/// Formats the list of completed tool calls into a compact status string.
fn format_tool_status(tools: &[(String, String)], done: bool) -> String {
    let icon = if done { "✅" } else { "⚙️" };
    let mut lines = vec![format!("{icon} Tool calls")];
    for (name, input) in tools {
        let summary = summarise_tool_input(input);
        if summary.is_empty() {
            lines.push(format!("• {name}"));
        } else {
            lines.push(format!("• {name} — {summary}"));
        }
    }
    lines.join("\n")
}

/// Returns a short human-readable summary of a (possibly partial) JSON input string.
fn summarise_tool_input(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    // Try to parse as a JSON object and list top-level keys / first value.
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(input) {
        // Show the first key=value pair as a hint (truncated).
        if let Some((k, v)) = map.iter().next() {
            let val = match v {
                serde_json::Value::String(s) => {
                    if s.len() > 50 {
                        format!("{}…", &s[..50])
                    } else {
                        s.clone()
                    }
                }
                other => {
                    let s = other.to_string();
                    if s.len() > 50 {
                        format!("{}…", &s[..50])
                    } else {
                        s
                    }
                }
            };
            return format!("{k}={val}");
        }
    }
    // Fall back to a truncated raw string.
    let trimmed = input.trim();
    if trimmed.len() <= 60 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..60])
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn split_message(text: &str) -> Vec<String> {
    if text.len() <= TG_MSG_LIMIT {
        return vec![text.to_string()];
    }
    // Split on newline boundaries where possible
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in text.split('\n') {
        if current.len() + line.len() + 1 > TG_MSG_LIMIT && !current.is_empty() {
            chunks.push(current.trim_end().to_string());
            current = String::new();
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}
