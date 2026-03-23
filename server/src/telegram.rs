//! Telegram messaging provider.
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
//! prefixed with `/` that aren't recognised bot commands are passed through to
//! Claude (e.g. `/compact`, `/help`).

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use teloxide::{
    net::Download,
    prelude::*,
    types::{MessageId, ParseMode, ReactionType, ReplyParameters},
    utils::command::BotCommands,
};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    protocol::{AttachedFile, SessionStatus, VmConfigProto, VmConfigResponse, S2C},
    provider::{self, ConversationSession, MessagingProvider, Responder, ToolEntry},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long a Telegram chat session lives before a new one is created.
const SESSION_LIFETIME: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

/// Telegram's message character limit. Reduced from 4096 to leave headroom for
/// HTML tags added by md_to_html.
const TG_MSG_LIMIT: usize = 3500;

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
    // ── VM management ──
    #[command(description = "Save auto-detected VM config to disk")]
    Vminit,
    #[command(description = "Set a VM field: /vmset image|base|datadir|network <value>")]
    Vmset(String),
    #[command(description = "Show VM config")]
    Vmconfig,
    #[command(description = "Add mount: /vmaddmount <name> <host_path> <guest_path> [size_gb]")]
    Vmaddmount(String),
    #[command(description = "Remove mount: /vmrmmount <name>")]
    Vmrmmount(String),
    #[command(description = "Manage tools: /vmtools list | add <pkg> | rm <pkg>")]
    Vmtools(String),
    #[command(description = "Enable VM mode")]
    Vmenable,
    #[command(description = "Disable VM mode")]
    Vmdisable,
    #[command(description = "Build (or rebuild) the Docker image on the client machine")]
    Vmrebuild,
}

// ---------------------------------------------------------------------------
// Per-chat state
// ---------------------------------------------------------------------------

struct ChatState {
    /// Provider-agnostic session tracking.
    inner: ConversationSession,
    /// If set, bot replies are threaded to this Telegram message (used for /task).
    task_reply_to: Option<MessageId>,
}

impl ChatState {
    fn new(session_id: String, task_reply_to: Option<MessageId>) -> Self {
        Self {
            inner: ConversationSession::new(session_id),
            task_reply_to,
        }
    }

    fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    fn is_expired(&self) -> bool {
        self.inner.is_expired(SESSION_LIFETIME)
    }

    fn collector_running(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.collector_running)
    }
}

type ChatStates = Arc<RwLock<HashMap<i64, ChatState>>>;

// ---------------------------------------------------------------------------
// Public entry point (used by main.rs)
// ---------------------------------------------------------------------------

/// Starts the Telegram bot. Called by `main` when `TELEGRAM_BOT_TOKEN` is set.
pub async fn start(app_state: Arc<AppState>, token: String) {
    TelegramProvider::new(token).start(app_state).await;
}

// ---------------------------------------------------------------------------
// TelegramProvider
// ---------------------------------------------------------------------------

/// Set of allowed Telegram user IDs. Empty means allow everyone.
type AllowedUsers = Arc<HashSet<u64>>;

/// Telegram messaging provider. Constructed by `main` when `TELEGRAM_BOT_TOKEN`
/// is set and started via the [`MessagingProvider`] trait.
pub struct TelegramProvider {
    token: String,
}

impl TelegramProvider {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl MessagingProvider for TelegramProvider {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, app_state: Arc<AppState>) {
        info!("telegram: starting bot");
        let bot = Bot::new(self.token.clone());

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

            let name = Some(format!("Telegram {}", chat_id.0));
            let session_id =
                provider::create_session(&app_state, name, Some(prompt)).await;
            let task_reply_to = msg.id;

            let collector_flag = {
                let mut guard = states.write().await;
                let state = ChatState::new(session_id.clone(), Some(task_reply_to));
                let flag = state.collector_running();
                flag.store(true, Ordering::Relaxed);
                guard.insert(chat_id.0, state);
                flag
            };

            let _ = bot
                .send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
                .await;

            set_reaction(&bot, chat_id, msg.id, "👀").await;

            spawn_collector(
                bot,
                chat_id,
                Some(task_reply_to),
                msg.id,
                session_id,
                sse_rx,
                collector_flag,
            );
        }

        Cmd::Status => {
            let guard = states.read().await;
            if let Some(state) = guard.get(&chat_id.0).filter(|s| !s.is_expired()) {
                let sessions = app_state.sessions.read().await;
                if let Some(buf) = sessions.get(state.session_id()) {
                    let age = state.inner.created_at.elapsed();
                    let remaining = SESSION_LIFETIME.as_secs().saturating_sub(age.as_secs());
                    let status_str = format!("{:?}", buf.info.status).to_lowercase();
                    let stats = &buf.info.stats;
                    bot.send_message(
                        chat_id,
                        format!(
                            "📊 <b>Session status</b>\nID: <code>{}</code>\nStatus: {}\nAge: {}h {}m\nExpires in: {}h {}m\nTokens: {}↑ {}↓\nCost: ${:.4}",
                            &state.session_id()[..8],
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
                    .parse_mode(ParseMode::Html)
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

        // ── VM management commands ──────────────────────────────────────────

        Cmd::Vminit => {
            // Save the auto-detected defaults (returned by the client when no
            // vm.toml exists) to disk. After this, /vmconfig shows the saved
            // config and /vmset can override individual fields.
            vm_update_config(&bot, chat_id, &app_state, |_| {}).await?;
        }

        Cmd::Vmset(args) => {
            let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();
            if parts.len() < 2 {
                bot.send_message(
                    chat_id,
                    "Usage: /vmset <field> <value>\n\
                     Fields: image, base, datadir, network",
                )
                .await?;
                return Ok(());
            }
            let field = parts[0].to_lowercase();
            let value = parts[1].trim().to_string();
            match field.as_str() {
                "network" => match value.as_str() {
                    "on" | "true" | "1" | "yes" => {
                        vm_update_config(&bot, chat_id, &app_state, |c| {
                            c.network_enabled = true;
                        })
                        .await?;
                    }
                    "off" | "false" | "0" | "no" => {
                        vm_update_config(&bot, chat_id, &app_state, |c| {
                            c.network_enabled = false;
                        })
                        .await?;
                    }
                    _ => {
                        bot.send_message(chat_id, "❌ network value must be on or off")
                            .await?;
                    }
                },
                "image" => {
                    vm_update_config(&bot, chat_id, &app_state, move |c| {
                        c.image = value;
                    })
                    .await?;
                }
                "base" => {
                    vm_update_config(&bot, chat_id, &app_state, move |c| {
                        c.base_image = value;
                    })
                    .await?;
                }
                "datadir" => {
                    vm_update_config(&bot, chat_id, &app_state, move |c| {
                        c.data_dir = value;
                    })
                    .await?;
                }
                _ => {
                    bot.send_message(
                        chat_id,
                        "❌ Unknown field. Valid fields: image, base, datadir, network",
                    )
                    .await?;
                }
            }
        }

        Cmd::Vmconfig => {
            match vm_get_config(&app_state).await {
                Ok(cfg) => {
                    bot.send_message(chat_id, format_vm_config(&cfg))
                        .parse_mode(ParseMode::Html)
                        .await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ {e}")).await?;
                }
            }
        }

        Cmd::Vmenable => {
            vm_update_config(&bot, chat_id, &app_state, |c| c.enabled = true).await?;
        }

        Cmd::Vmdisable => {
            vm_update_config(&bot, chat_id, &app_state, |c| c.enabled = false).await?;
        }

        Cmd::Vmaddmount(args) => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            if parts.len() < 3 {
                bot.send_message(
                    chat_id,
                    "Usage: /vmaddmount <name> <host_path> <guest_path> [size_gb]",
                )
                .await?;
                return Ok(());
            }
            let name = parts[0].to_string();
            let host_path = parts[1].to_string();
            let guest_path = parts[2].to_string();
            let size_gb: u32 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

            vm_update_config(&bot, chat_id, &app_state, move |c| {
                // Remove existing mount with same name
                c.mounts.retain(|m| m.name != name);
                c.mounts.push(crate::protocol::VolumeMountProto {
                    name,
                    host_path,
                    guest_path,
                    size_gb,
                    excludes: vec![
                        "node_modules".to_string(),
                        "target".to_string(),
                        ".git/objects".to_string(),
                    ],
                });
            })
            .await?;
        }

        Cmd::Vmrmmount(name) => {
            let name = name.trim().to_string();
            vm_update_config(&bot, chat_id, &app_state, move |c| {
                c.mounts.retain(|m| m.name != name);
            })
            .await?;
        }

        Cmd::Vmtools(args) => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            match parts.as_slice() {
                ["list"] | [] => {
                    match vm_get_config(&app_state).await {
                        Ok(cfg) => {
                            let pkgs = if cfg.tools.extra_packages.is_empty() {
                                "(none — only base Alpine packages)".to_string()
                            } else {
                                cfg.tools.extra_packages.join(", ")
                            };
                            bot.send_message(
                                chat_id,
                                format!("🔧 <b>Extra packages:</b> {pkgs}"),
                            )
                            .parse_mode(ParseMode::Html)
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("❌ {e}")).await?;
                        }
                    }
                }
                ["add", pkg] => {
                    let pkg = pkg.to_string();
                    vm_update_config(&bot, chat_id, &app_state, move |c| {
                        if !c.tools.extra_packages.contains(&pkg) {
                            c.tools.extra_packages.push(pkg);
                        }
                    })
                    .await?;
                }
                ["rm", pkg] => {
                    let pkg = pkg.to_string();
                    vm_update_config(&bot, chat_id, &app_state, move |c| {
                        c.tools.extra_packages.retain(|p| p != &pkg);
                    })
                    .await?;
                }
                _ => {
                    bot.send_message(
                        chat_id,
                        "Usage: /vmtools list | /vmtools add <pkg> | /vmtools rm <pkg>",
                    )
                    .await?;
                }
            }
        }

        Cmd::Vmrebuild => {
            bot.send_message(chat_id, "🔨 Building Docker image on client… (this may take a while)")
                .await?;
            let request_id = Uuid::new_v4().to_string();
            match vm_send_and_await(
                &app_state,
                crate::protocol::S2C::BuildImage { request_id },
            )
            .await
            {
                Ok(VmConfigResponse::BuildResult { success: true, output }) => {
                    let tail = if output.len() > 3000 {
                        format!("…{}", &output[output.len() - 3000..])
                    } else {
                        output
                    };
                    bot.send_message(
                        chat_id,
                        format!("✅ Image built.\n<pre>{}</pre>", escape_html(&tail)),
                    )
                    .parse_mode(ParseMode::Html)
                    .await?;
                }
                Ok(VmConfigResponse::BuildResult { success: false, output }) => {
                    let tail = if output.len() > 3000 {
                        format!("…{}", &output[output.len() - 3000..])
                    } else {
                        output
                    };
                    bot.send_message(
                        chat_id,
                        format!("❌ Build failed.\n<pre>{}</pre>", escape_html(&tail)),
                    )
                    .parse_mode(ParseMode::Html)
                    .await?;
                }
                Ok(_) => {
                    bot.send_message(chat_id, "❌ Unexpected response from client.").await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ {e}")).await?;
                }
            }
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
    // Text comes from the message body (text messages) or caption (media messages).
    let text = msg
        .text()
        .map(String::from)
        .or_else(|| msg.caption().map(String::from))
        .unwrap_or_default();

    let chat_id = msg.chat.id;

    // "stop" safe word — kill immediately without forwarding to Claude.
    if text.trim().eq_ignore_ascii_case("stop") {
        return kill_current_session(&bot, chat_id, &app_state, &states).await;
    }

    // Download any attached files (photos, documents).
    let files = collect_message_files(&bot, &msg).await;

    // Nothing useful to forward.
    if text.trim().is_empty() && files.is_empty() {
        return Ok(());
    }

    // Slash commands (unrecognised bot commands forwarded to Claude, e.g. /compact)
    // are threaded back to the command message so they stay grouped.
    // Regular conversation is inline (no reply thread).
    let slash_reply = if text.trim_start().starts_with('/') {
        Some(msg.id)
    } else {
        None
    };

    // Subscribe BEFORE sending to avoid missing early streaming events.
    let sse_rx = app_state.sse_tx.subscribe();

    let (session_id, task_reply_to): (Option<String>, Option<MessageId>) = {
        let guard = states.read().await;
        guard
            .get(&chat_id.0)
            .filter(|s| !s.is_expired())
            .map(|s| (Some(s.session_id().to_string()), s.task_reply_to))
            .unwrap_or((None, None))
    };

    // reply_to: prefer the /task thread anchor, then a slash-command reply, else None.
    let reply_to = task_reply_to.or(slash_reply);

    let name = Some(format!("Telegram {}", chat_id.0));

    let (session_id, collector_flag) = if let Some(id) = session_id {
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
            let initial = if files.is_empty() { Some(text.clone()) } else { None };
            let new_id = provider::create_session(&app_state, name, initial).await;
            if !files.is_empty() {
                app_state
                    .send_to_client(&S2C::SendInputWithFiles {
                        session_id: new_id.clone(),
                        text: text.clone(),
                        files: files.clone(),
                    })
                    .await;
            }
            let flag = {
                let mut guard = states.write().await;
                let state = ChatState::new(new_id.clone(), None);
                let flag = state.collector_running();
                flag.store(true, Ordering::Relaxed);
                guard.insert(chat_id.0, state);
                flag
            };
            (new_id, flag)
        } else {
            // Session is alive — check if a collector is already running.
            let (flag, already_running) = {
                let guard = states.read().await;
                let flag = guard
                    .get(&chat_id.0)
                    .map(|s| s.collector_running())
                    .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                let already = flag.load(Ordering::Relaxed);
                (flag, already)
            };

            if files.is_empty() {
                app_state
                    .send_to_client(&S2C::SendInput {
                        session_id: id.clone(),
                        text: text.clone(),
                    })
                    .await;
            } else {
                app_state
                    .send_to_client(&S2C::SendInputWithFiles {
                        session_id: id.clone(),
                        text: text.clone(),
                        files: files.clone(),
                    })
                    .await;
            }

            if already_running {
                info!("telegram: collector already active for session {id}, skipping spawn");
                return Ok(());
            }

            flag.store(true, Ordering::Relaxed);
            (id, flag)
        }
    } else {
        // No active session — create one.
        let initial = if files.is_empty() { Some(text.clone()) } else { None };
        let id = provider::create_session(&app_state, name, initial).await;
        if !files.is_empty() {
            app_state
                .send_to_client(&S2C::SendInputWithFiles {
                    session_id: id.clone(),
                    text: text.clone(),
                    files: files.clone(),
                })
                .await;
        }
        let flag = {
            let mut guard = states.write().await;
            let state = ChatState::new(id.clone(), None);
            let flag = state.collector_running();
            flag.store(true, Ordering::Relaxed);
            guard.insert(chat_id.0, state);
            flag
        };
        (id, flag)
    };

    let _ = bot
        .send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
        .await;

    set_reaction(&bot, chat_id, msg.id, "👀").await;

    spawn_collector(bot, chat_id, reply_to, msg.id, session_id, sse_rx, collector_flag);

    Ok(())
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
            .map(|s| s.session_id().to_string())
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
// VM config helpers
// ---------------------------------------------------------------------------

/// Timeout for waiting for a VM config request/response round-trip.
const VM_CONFIG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Sends `msg` to the client and awaits the corresponding response via the
/// `vm_config_pending` oneshot map.  Returns an error string if the client is
/// not connected, the send fails, or the timeout elapses.
async fn vm_send_and_await(
    app_state: &Arc<AppState>,
    msg: S2C,
) -> anyhow::Result<VmConfigResponse> {
    // Extract request_id from the message so we can register the waiter.
    let request_id = match &msg {
        S2C::GetVmConfig { request_id } => request_id.clone(),
        S2C::SetVmConfig { request_id, .. } => request_id.clone(),
        S2C::BuildImage { request_id } => request_id.clone(),
        _ => anyhow::bail!("vm_send_and_await: unexpected message type"),
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut pending = app_state.vm_config_pending.write().await;
        pending.insert(request_id, tx);
    }

    if !app_state.send_to_client(&msg).await {
        anyhow::bail!("client not connected");
    }

    match tokio::time::timeout(VM_CONFIG_TIMEOUT, rx).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_)) => anyhow::bail!("client dropped the response channel"),
        Err(_) => anyhow::bail!("timed out waiting for VM config response"),
    }
}

/// Sends `GetVmConfig` to the client and returns the config.
async fn vm_get_config(app_state: &Arc<AppState>) -> anyhow::Result<VmConfigProto> {
    let request_id = Uuid::new_v4().to_string();
    match vm_send_and_await(app_state, S2C::GetVmConfig { request_id }).await? {
        VmConfigResponse::Config(cfg) => Ok(cfg),
        VmConfigResponse::Ack { error: Some(e), .. } => anyhow::bail!("{e}"),
        _ => anyhow::bail!("unexpected response to GetVmConfig"),
    }
}

/// Gets the current VM config (or creates a default), applies `mutate`, sends
/// it back via `SetVmConfig`, and sends a confirmation or error message.
async fn vm_update_config<F>(
    bot: &Bot,
    chat_id: ChatId,
    app_state: &Arc<AppState>,
    mutate: F,
) -> ResponseResult<()>
where
    F: FnOnce(&mut VmConfigProto),
{
    let mut cfg = match vm_get_config(app_state).await {
        Ok(c) => c,
        Err(e) => {
            bot.send_message(chat_id, format!("❌ {e}")).await?;
            return Ok(());
        }
    };

    mutate(&mut cfg);

    let request_id = Uuid::new_v4().to_string();
    match vm_send_and_await(
        app_state,
        S2C::SetVmConfig {
            request_id,
            config: cfg.clone(),
        },
    )
    .await
    {
        Ok(VmConfigResponse::Ack { success: true, .. }) => {
            bot.send_message(chat_id, format_vm_config(&cfg))
                .parse_mode(ParseMode::Html)
                .await?;
        }
        Ok(VmConfigResponse::Ack {
            success: false,
            error,
        }) => {
            let msg = error.unwrap_or_else(|| "unknown error".to_string());
            bot.send_message(chat_id, format!("❌ {msg}")).await?;
        }
        Ok(_) => {
            bot.send_message(chat_id, "❌ Unexpected response from client.")
                .await?;
        }
        Err(e) => {
            bot.send_message(chat_id, format!("❌ {e}")).await?;
        }
    }

    Ok(())
}

/// Formats a `VmConfigProto` as Telegram HTML.
fn format_vm_config(cfg: &VmConfigProto) -> String {
    let status = if cfg.enabled { "✅ enabled" } else { "❌ disabled" };
    let net_status = if cfg.network_enabled { "✅ on" } else { "❌ off" };
    let mut lines = vec![
        format!("🖥 <b>VM Config</b> — {status}"),
        format!("  Network: {net_status}"),
        format!("  Base image: <code>{}</code>", escape_html(&cfg.base_image)),
        format!("  Built image: <code>{}</code>", escape_html(&cfg.image)),
    ];

    if cfg.mounts.is_empty() {
        lines.push("  <b>Mounts:</b> (none)".to_string());
    } else {
        lines.push("  <b>Mounts:</b>".to_string());
        for m in &cfg.mounts {
            lines.push(format!(
                "    • <code>{}</code>: {} → {} ({}GB)",
                escape_html(&m.name),
                escape_html(&m.host_path),
                escape_html(&m.guest_path),
                m.size_gb,
            ));
        }
    }

    if cfg.tools.extra_packages.is_empty() {
        lines.push("  <b>Extra packages:</b> (none)".to_string());
    } else {
        lines.push(format!(
            "  <b>Extra packages:</b> {}",
            cfg.tools.extra_packages.join(", ")
        ));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// TelegramResponder — implements provider::Responder for Telegram
// ---------------------------------------------------------------------------

/// Telegram-specific [`Responder`] implementation.
///
/// Holds the bot handle and per-message context needed to deliver reactions,
/// the live tool-status message, and the final response text.
struct TelegramResponder {
    bot: Bot,
    chat_id: ChatId,
    user_msg_id: MessageId,
    reply_to: Option<MessageId>,
    /// The Telegram message used to show live tool status (created lazily).
    status_msg_id: tokio::sync::Mutex<Option<MessageId>>,
}

impl TelegramResponder {
    fn new(
        bot: Bot,
        chat_id: ChatId,
        user_msg_id: MessageId,
        reply_to: Option<MessageId>,
    ) -> Self {
        Self {
            bot,
            chat_id,
            user_msg_id,
            reply_to,
            status_msg_id: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl Responder for TelegramResponder {
    async fn on_working(&self) {
        set_reaction(&self.bot, self.chat_id, self.user_msg_id, "🏗").await;
    }

    async fn update_tool_status(
        &self,
        tools: &[ToolEntry],
        current: Option<&ToolEntry>,
        session_start: Instant,
        done: bool,
    ) {
        let mut guard = self.status_msg_id.lock().await;
        *guard = send_or_edit_tool_status(
            &self.bot,
            self.chat_id,
            self.reply_to,
            tools,
            current,
            session_start,
            *guard,
            done,
        )
        .await;
    }

    async fn send_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        for chunk in split_message(text) {
            let html = md_to_html(&chunk);
            let mut req = self
                .bot
                .send_message(self.chat_id, &html)
                .parse_mode(ParseMode::Html);
            if let Some(rid) = self.reply_to {
                req = req.reply_parameters(ReplyParameters::new(rid));
            }
            if let Err(e) = req.await {
                warn!("telegram: failed to send message: {e}");
            }
        }
    }

    async fn on_done(&self) {
        set_reaction(&self.bot, self.chat_id, self.user_msg_id, "✅").await;
    }
}

// ---------------------------------------------------------------------------
// Collector spawn helper
// ---------------------------------------------------------------------------

/// Spawns a task that runs [`provider::collect_and_respond`] with a
/// [`TelegramResponder`], then clears `collector_flag` when done.
fn spawn_collector(
    bot: Bot,
    chat_id: ChatId,
    reply_to: Option<MessageId>,
    user_msg_id: MessageId,
    session_id: String,
    sse_rx: tokio::sync::broadcast::Receiver<String>,
    collector_flag: Arc<AtomicBool>,
) {
    let responder = Arc::new(TelegramResponder::new(bot, chat_id, user_msg_id, reply_to));
    tokio::spawn(async move {
        provider::collect_and_respond(session_id, sse_rx, &*responder).await;
        collector_flag.store(false, Ordering::Relaxed);
    });
}

// ---------------------------------------------------------------------------
// Tool-status Telegram message helpers
// ---------------------------------------------------------------------------

/// Sends a new tool-status message or edits the existing one.
/// Returns the `MessageId` of the (possibly new) message.
async fn send_or_edit_tool_status(
    bot: &Bot,
    chat_id: ChatId,
    reply_to: Option<MessageId>,
    tools: &[ToolEntry],
    current: Option<&ToolEntry>,
    session_start: Instant,
    existing_id: Option<MessageId>,
    done: bool,
) -> Option<MessageId> {
    let body = format_tool_status(tools, current, session_start, done);

    if let Some(mid) = existing_id {
        match bot
            .edit_message_text(chat_id, mid, &body)
            .parse_mode(ParseMode::Html)
            .await
        {
            Ok(_) => return Some(mid),
            Err(e) => {
                warn!("telegram: failed to edit tool status message: {e}");
                return Some(mid);
            }
        }
    }

    let mut req = bot
        .send_message(chat_id, &body)
        .parse_mode(ParseMode::Html);
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

/// Formats the list of completed (and optionally in-progress) tool calls as HTML.
///
/// Each completed tool shows elapsed duration plus a spoiler with full input.
/// The current in-progress tool (if any) is shown at the bottom with a spinner.
fn format_tool_status(
    tools: &[ToolEntry],
    current: Option<&ToolEntry>,
    session_start: Instant,
    done: bool,
) -> String {
    let total_secs = session_start.elapsed().as_secs();
    let icon = if done { "✅" } else { "⚙️" };
    let mut lines = vec![format!(
        "{icon} <b>Tool calls</b> • {}",
        fmt_duration(total_secs)
    )];

    for entry in tools {
        let secs = entry.elapsed_secs();
        let summary = summarise_tool_input(&entry.input);
        let full_html = full_tool_input_html(&entry.input);
        let time = fmt_duration(secs);

        let line = if summary.is_empty() {
            format!("• <b>{}</b> ({})", escape_html(&entry.name), time)
        } else if full_html.len() > escape_html(&summary).len() + 10 {
            format!(
                "• <b>{}</b> — <code>{}</code> ({time}) <tg-spoiler>{full_html}</tg-spoiler>",
                escape_html(&entry.name),
                escape_html(&summary),
            )
        } else {
            format!(
                "• <b>{}</b> — <code>{}</code> ({})",
                escape_html(&entry.name),
                escape_html(&summary),
                time
            )
        };
        lines.push(line);
    }

    if let Some(entry) = current {
        let secs = entry.elapsed_secs();
        let summary = summarise_tool_input(&entry.input);
        let time = fmt_duration(secs);
        let line = if summary.is_empty() {
            format!("• ⏳ <b>{}</b> ({}…)", escape_html(&entry.name), time)
        } else {
            format!(
                "• ⏳ <b>{}</b> — <code>{}</code> ({}…)",
                escape_html(&entry.name),
                escape_html(&summary),
                time
            )
        };
        lines.push(line);
    }

    lines.join("\n")
}

fn fmt_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

/// Returns a short human-readable summary of a (possibly partial) JSON input string.
fn summarise_tool_input(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(input) {
        if let Some((k, v)) = map.iter().next() {
            let val = match v {
                serde_json::Value::String(s) => truncate_chars(s, 80),
                other => truncate_chars(&other.to_string(), 80),
            };
            return format!("{k}={val}");
        }
    }
    truncate_chars(input.trim(), 80)
}

/// Returns all key=value pairs from the tool input as HTML, suitable for use
/// inside a `<tg-spoiler>` block.
fn full_tool_input_html(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(input) {
        let parts: Vec<String> = map
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => escape_html(s),
                    other => escape_html(&other.to_string()),
                };
                format!("<b>{}</b>={val}", escape_html(k))
            })
            .collect();
        return parts.join(" | ");
    }
    escape_html(input.trim())
}

// ---------------------------------------------------------------------------
// Attachment helpers
// ---------------------------------------------------------------------------

/// Downloads all attachments from a Telegram message and returns them as
/// base64-encoded `AttachedFile` records ready to send to the client daemon.
///
/// Supports photos and documents. Other media types (video, voice, sticker)
/// are silently skipped.
async fn collect_message_files(bot: &Bot, msg: &Message) -> Vec<AttachedFile> {
    let mut files = Vec::new();

    // Photos — take the highest-resolution version (last in the array).
    if let Some(photos) = msg.photo() {
        if let Some(photo) = photos.last() {
            let name = format!("photo_{}.jpg", &photo.file.id[..8.min(photo.file.id.len())]);
            if let Some(data) = download_tg_file(bot, &photo.file.id).await {
                files.push(AttachedFile {
                    filename: name,
                    mime_type: "image/jpeg".to_string(),
                    data_base64: STANDARD.encode(&data),
                });
            }
        }
    }

    // Documents (any file the user sends as a file, not compressed).
    if let Some(doc) = msg.document() {
        let mime = doc
            .mime_type
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let name = doc
            .file_name
            .clone()
            .unwrap_or_else(|| format!("file_{}", &doc.file.id[..8.min(doc.file.id.len())]));
        if let Some(data) = download_tg_file(bot, &doc.file.id).await {
            files.push(AttachedFile {
                filename: name,
                mime_type: mime,
                data_base64: STANDARD.encode(&data),
            });
        }
    }

    files
}

/// Downloads a Telegram file by its `file_id` into memory.
/// Returns `None` and logs a warning on any error.
async fn download_tg_file(bot: &Bot, file_id: &str) -> Option<Vec<u8>> {
    let file = match bot.get_file(file_id).await {
        Ok(f) => f,
        Err(e) => {
            warn!("telegram: get_file {file_id}: {e}");
            return None;
        }
    };

    // Download into a temp file then read back — tokio::fs::File implements AsyncWrite.
    let tmp_path = std::env::temp_dir().join(format!("tg_dl_{}", Uuid::new_v4()));
    let mut dst = match tokio::fs::File::create(&tmp_path).await {
        Ok(f) => f,
        Err(e) => {
            warn!("telegram: failed to create tmp file for download: {e}");
            return None;
        }
    };

    if let Err(e) = bot.download_file(&file.path, &mut dst).await {
        warn!("telegram: download_file {file_id}: {e}");
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return None;
    }
    drop(dst);

    let data = match tokio::fs::read(&tmp_path).await {
        Ok(b) => b,
        Err(e) => {
            warn!("telegram: failed to read downloaded file: {e}");
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return None;
        }
    };
    let _ = tokio::fs::remove_file(&tmp_path).await;

    info!("telegram: downloaded {} bytes for file {file_id}", data.len());
    Some(data)
}

// ---------------------------------------------------------------------------
// Reaction helper
// ---------------------------------------------------------------------------

/// Sets a reaction emoji on a message. Logs and swallows errors (e.g. if the
/// emoji isn't in Telegram's allowed reaction set).
async fn set_reaction(bot: &Bot, chat_id: ChatId, msg_id: MessageId, emoji: &str) {
    let reaction = vec![ReactionType::Emoji {
        emoji: emoji.to_string(),
    }];
    if let Err(e) = bot
        .set_message_reaction(chat_id, msg_id)
        .reaction(reaction)
        .await
    {
        warn!("telegram: failed to set reaction {emoji}: {e}");
    }
}

// ---------------------------------------------------------------------------
// Markdown → Telegram HTML converter
// ---------------------------------------------------------------------------

/// Converts Claude's CommonMark output to Telegram HTML.
///
/// Supported: fenced code blocks, inline code, **bold**, *italic*, _italic_,
/// __bold__, headers (→ bold), [links](url). Everything else is HTML-escaped.
pub fn md_to_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 256);
    let mut lines = md.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();

        // Fenced code block (``` or ~~~)
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence_marker = if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            };
            // Everything after the fence marker on the opening line is the language hint.
            // (Telegram doesn't render it but we include it for completeness.)
            let mut code_lines: Vec<&str> = Vec::new();

            for inner in lines.by_ref() {
                if inner.trim_start().starts_with(fence_marker) {
                    break;
                }
                code_lines.push(inner);
            }

            let code = code_lines.join("\n");
            out.push_str("<pre><code>");
            out.push_str(&escape_html(&code));
            out.push_str("</code></pre>\n");
            continue;
        }

        // Normal line — process inline formatting.
        out.push_str(&process_inline_md(line));
        out.push('\n');
    }

    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Handles Markdown headers and delegates everything else to `process_inline_spans`.
fn process_inline_md(line: &str) -> String {
    // ATX-style headers: strip leading #s and wrap in <b>
    let content = if let Some(rest) = line
        .strip_prefix("#### ")
        .or_else(|| line.strip_prefix("### "))
        .or_else(|| line.strip_prefix("## "))
        .or_else(|| line.strip_prefix("# "))
    {
        return format!("<b>{}</b>", process_inline_spans(rest));
    } else {
        line
    };

    process_inline_spans(content)
}

/// Converts inline Markdown spans (bold, italic, code, links) to HTML.
/// Also HTML-escapes `&`, `<`, `>` in plain text regions.
fn process_inline_spans(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 64);
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            // HTML special chars in plain text
            '&' => {
                out.push_str("&amp;");
                i += 1;
            }
            '<' => {
                out.push_str("&lt;");
                i += 1;
            }
            '>' => {
                out.push_str("&gt;");
                i += 1;
            }

            // Inline code: `...`
            '`' => {
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && chars[end] != '`' {
                    end += 1;
                }
                if end < chars.len() {
                    let code: String = chars[start..end].iter().collect();
                    out.push_str("<code>");
                    out.push_str(&escape_html(&code));
                    out.push_str("</code>");
                    i = end + 1;
                } else {
                    out.push('`');
                    i += 1;
                }
            }

            // Bold ** or italic *
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    // Potential bold **text**
                    let start = i + 2;
                    let mut j = start;
                    while j + 1 < chars.len() {
                        if chars[j] == '*' && chars[j + 1] == '*' {
                            break;
                        }
                        j += 1;
                    }
                    if j + 1 < chars.len() && chars[j] == '*' && chars[j + 1] == '*' {
                        let inner: String = chars[start..j].iter().collect();
                        out.push_str("<b>");
                        out.push_str(&process_inline_spans(&inner));
                        out.push_str("</b>");
                        i = j + 2;
                    } else {
                        // No closing ** — output literally
                        out.push('*');
                        i += 1;
                    }
                } else if i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
                    // Potential italic *text*
                    let start = i + 1;
                    let mut j = start;
                    while j < chars.len() && chars[j] != '*' {
                        j += 1;
                    }
                    if j < chars.len() {
                        let inner: String = chars[start..j].iter().collect();
                        out.push_str("<i>");
                        out.push_str(&process_inline_spans(&inner));
                        out.push_str("</i>");
                        i = j + 1;
                    } else {
                        out.push('*');
                        i += 1;
                    }
                } else {
                    // Bullet list marker or lonely asterisk — output literally
                    out.push('*');
                    i += 1;
                }
            }

            // Bold __ or italic _
            '_' => {
                if i + 1 < chars.len() && chars[i + 1] == '_' {
                    // Potential bold __text__
                    let start = i + 2;
                    let mut j = start;
                    while j + 1 < chars.len() {
                        if chars[j] == '_' && chars[j + 1] == '_' {
                            break;
                        }
                        j += 1;
                    }
                    if j + 1 < chars.len() && chars[j] == '_' && chars[j + 1] == '_' {
                        let inner: String = chars[start..j].iter().collect();
                        out.push_str("<b>");
                        out.push_str(&process_inline_spans(&inner));
                        out.push_str("</b>");
                        i = j + 2;
                    } else {
                        out.push('_');
                        i += 1;
                    }
                } else if i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
                    // Potential italic _text_
                    let start = i + 1;
                    let mut j = start;
                    while j < chars.len() && chars[j] != '_' {
                        j += 1;
                    }
                    if j < chars.len() {
                        let inner: String = chars[start..j].iter().collect();
                        out.push_str("<i>");
                        out.push_str(&process_inline_spans(&inner));
                        out.push_str("</i>");
                        i = j + 1;
                    } else {
                        out.push('_');
                        i += 1;
                    }
                } else {
                    out.push('_');
                    i += 1;
                }
            }

            // Links: [text](url)
            '[' => {
                // Find closing ]
                let mut j = i + 1;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j < chars.len() && j + 1 < chars.len() && chars[j + 1] == '(' {
                    let link_text: String = chars[i + 1..j].iter().collect();
                    let mut k = j + 2;
                    while k < chars.len() && chars[k] != ')' {
                        k += 1;
                    }
                    if k < chars.len() {
                        let url: String = chars[j + 2..k].iter().collect();
                        out.push_str(&format!(
                            "<a href=\"{}\">{}</a>",
                            escape_html(&url),
                            escape_html(&link_text)
                        ));
                        i = k + 1;
                        continue;
                    }
                }
                // Not a valid link — output [ literally
                out.push('[');
                i += 1;
            }

            c => {
                out.push(c);
                i += 1;
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Escapes HTML special characters (`&`, `<`, `>`).
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Truncates a string to at most `max_chars` Unicode characters, appending `…`
/// if truncation occurs.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let mut result = String::with_capacity(max_chars + 4);
    let mut count = 0;
    for c in chars.by_ref() {
        if count >= max_chars {
            result.push('…');
            return result;
        }
        result.push(c);
        count += 1;
    }
    result
}

fn split_message(text: &str) -> Vec<String> {
    if text.len() <= TG_MSG_LIMIT {
        return vec![text.to_string()];
    }
    // Split on newline boundaries where possible.
    // Note: splitting mid-code-block will produce slightly malformed HTML in
    // the second chunk, but this is an acceptable trade-off for very long responses.
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
