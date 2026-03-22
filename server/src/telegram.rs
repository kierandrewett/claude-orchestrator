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
use teloxide::{prelude::*, types::{MessageId, ReplyParameters}, utils::command::BotCommands};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    protocol::{S2C, S2D, SessionInfo, SessionStats, SessionStatus},
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
                    let remaining =
                        SESSION_LIFETIME.as_secs().saturating_sub(age.as_secs());
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
        app_state
            .send_to_client(&S2C::SendInput {
                session_id: id.clone(),
                text,
            })
            .await;
        id
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
// Response collector
// ---------------------------------------------------------------------------

/// Subscribes to the broadcast channel, accumulates Claude's text response for
/// `session_id`, then sends it to Telegram (optionally as a reply).
async fn collect_and_send(
    bot: Bot,
    chat_id: ChatId,
    reply_to: Option<MessageId>,
    session_id: String,
    mut sse_rx: broadcast::Receiver<String>,
) {
    let deadline = tokio::time::Instant::now() + RESPONSE_TIMEOUT;
    let mut text = String::new();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
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
                        let evt_type =
                            event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        match evt_type {
                            "content_block_delta" => {
                                if let Some(delta) =
                                    event.pointer("/delta/text").and_then(|t| t.as_str())
                                {
                                    text.push_str(delta);
                                }
                            }
                            "message_stop" | "result" => break,
                            // Turn-complete format: extract text from content array
                            "assistant" => {
                                if let Some(content) =
                                    event.get("content").and_then(|c| c.as_array())
                                {
                                    for block in content {
                                        if block
                                            .get("type")
                                            .and_then(|t| t.as_str())
                                            == Some("text")
                                        {
                                            if let Some(t) =
                                                block.get("text").and_then(|t| t.as_str())
                                            {
                                                text.push_str(t);
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                    S2D::SessionEnded {
                        session_id: sid, ..
                    } if sid == session_id => break,
                    _ => {}
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }

    if text.is_empty() {
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
