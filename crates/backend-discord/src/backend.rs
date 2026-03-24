use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use backend_traits::MessagingBackend;
use claude_events::{
    BackendEvent, BackendSource, MessageRef, OrchestratorEvent, TaskId, TaskStateSummary,
};
use poise::serenity_prelude::{
    AutoArchiveDuration, ChannelId, ChannelType, ClientBuilder, Context, CreateAttachment,
    CreateMessage, CreateThread, FullEvent, GatewayIntents, UserId,
};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::commands;
use crate::formatting::*;

// ── Config ───────────────────────────────────────────────────────────────────

/// Configuration for the Discord backend.
#[derive(Debug, Clone)]
pub struct DiscordConfig {
    /// Bot token from the Discord Developer Portal.
    pub bot_token: String,
    /// ID of the text channel where task threads are created.
    pub channel_id: u64,
    /// If set, commands are registered in this guild only (instant).
    /// If None, commands are registered globally (can take up to an hour).
    pub guild_id: Option<u64>,
    /// Optional whitelist of Discord user IDs that may send messages.
    /// Empty = allow everyone.
    pub allowed_user_ids: Vec<u64>,
    /// Whether to emit thinking/internal monologue messages.
    pub show_thinking: bool,
}

// ── Shared state ─────────────────────────────────────────────────────────────

/// State shared between poise commands and the serenity event handler.
pub struct Data {
    pub backend_tx: mpsc::Sender<BackendEvent>,
    /// thread ChannelId → task ID string (for routing incoming messages)
    pub thread_to_task: Arc<Mutex<HashMap<ChannelId, String>>>,
    pub allowed_users: Vec<UserId>,
    /// Filled once the `Ready` event fires; used by the orchestrator loop.
    pub ctx_holder: Arc<Mutex<Option<Context>>>,
}

// ── Serenity / poise event handler ───────────────────────────────────────────

async fn on_event(
    ctx: &Context,
    event: &FullEvent,
    _framework: poise::FrameworkContext<'_, Data, anyhow::Error>,
    data: &Data,
) -> Result<(), anyhow::Error> {
    match event {
        FullEvent::Ready { data_about_bot } => {
            info!("discord: logged in as {}", data_about_bot.user.name);
            *data.ctx_holder.lock().await = Some(ctx.clone());
        }

        FullEvent::Message { new_message } => {
            let msg = new_message;

            if msg.author.bot {
                return Ok(());
            }
            if !data.allowed_users.is_empty() && !data.allowed_users.contains(&msg.author.id) {
                return Ok(());
            }

            let text = msg.content.trim().to_string();
            if text.is_empty() {
                return Ok(());
            }

            // Ignore slash commands — poise handles those.
            if text.starts_with('/') {
                return Ok(());
            }

            let source = BackendSource::new("discord", msg.author.id.to_string());
            let msg_ref = MessageRef::new("discord", msg.id.to_string());
            let task_id_str = {
                let t2t = data.thread_to_task.lock().await;
                t2t.get(&msg.channel_id)
                    .cloned()
                    .unwrap_or_else(|| "scratchpad".to_string())
            };

            let _ = data
                .backend_tx
                .send(BackendEvent::UserMessage {
                    task_id: TaskId(task_id_str),
                    text,
                    message_ref: msg_ref,
                    source,
                })
                .await;
        }

        _ => {}
    }
    Ok(())
}

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct DiscordBackend {
    config: DiscordConfig,
}

impl DiscordBackend {
    pub fn new(config: DiscordConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl MessagingBackend for DiscordBackend {
    fn name(&self) -> &str {
        "discord"
    }

    async fn run(
        &self,
        mut orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()> {
        let thread_to_task: Arc<Mutex<HashMap<ChannelId, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let task_to_thread: Arc<Mutex<HashMap<String, ChannelId>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let ctx_holder: Arc<Mutex<Option<Context>>> = Arc::new(Mutex::new(None));

        // Clones for the orchestrator loop below.
        let task_to_thread_loop = Arc::clone(&task_to_thread);
        let ctx_holder_loop = Arc::clone(&ctx_holder);

        let parent_channel = ChannelId::new(self.config.channel_id);
        let guild_id = self.config.guild_id;
        let show_thinking = self.config.show_thinking;

        let data = Data {
            backend_tx: backend_sender,
            thread_to_task,
            allowed_users: self
                .config
                .allowed_user_ids
                .iter()
                .map(|&id| UserId::new(id))
                .collect(),
            ctx_holder,
        };

        let framework = poise::Framework::builder()
            .options(poise::FrameworkOptions {
                commands: commands::all(),
                event_handler: |ctx, event, framework, data| {
                    Box::pin(on_event(ctx, event, framework, data))
                },
                ..Default::default()
            })
            .setup(move |ctx, _ready, framework| {
                Box::pin(async move {
                    if let Some(gid) = guild_id {
                        poise::builtins::register_in_guild(
                            ctx,
                            &framework.options().commands,
                            poise::serenity_prelude::GuildId::new(gid),
                        )
                        .await?;
                        info!("discord: slash commands registered in guild {gid}");
                    } else {
                        poise::builtins::register_globally(ctx, &framework.options().commands)
                            .await?;
                        info!("discord: slash commands registered globally");
                    }
                    Ok(data)
                })
            })
            .build();

        let intents = GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let client = ClientBuilder::new(&self.config.bot_token, intents)
            .framework(framework)
            .await?;

        tokio::spawn(async move {
            let mut client = client;
            if let Err(e) = client.start().await {
                error!("discord: serenity client error: {e}");
            }
        });

        info!("discord: backend started, waiting for gateway ready…");

        // Orchestrator → Discord event loop.
        loop {
            let event = match orchestrator_events.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("discord: lagged by {n} events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("discord: orchestrator channel closed, exiting");
                    return Ok(());
                }
            };

            let ctx = ctx_holder_loop.lock().await.clone();
            let Some(ctx) = ctx else {
                debug!("discord: gateway not ready, dropping {}", event_name(&event));
                continue;
            };

            handle_orch_event(
                &ctx,
                parent_channel,
                &event,
                &task_to_thread_loop,
                show_thinking,
            )
            .await;
        }
    }
}

// ── Orchestrator → Discord ────────────────────────────────────────────────────

async fn handle_orch_event(
    ctx: &Context,
    parent_channel: ChannelId,
    event: &OrchestratorEvent,
    task_to_thread: &Mutex<HashMap<String, ChannelId>>,
    show_thinking: bool,
) {
    match event {
        OrchestratorEvent::TaskCreated {
            task_id,
            name,
            profile,
            ..
        } => {
            let header = format_task_header(name, profile);
            match parent_channel
                .send_message(ctx, CreateMessage::new().content(&header))
                .await
            {
                Ok(msg) => {
                    let builder = CreateThread::new(name)
                        .auto_archive_duration(AutoArchiveDuration::OneWeek)
                        .kind(ChannelType::PublicThread);
                    match parent_channel
                        .create_thread_from_message(ctx, msg.id, builder)
                        .await
                    {
                        Ok(thread) => {
                            task_to_thread
                                .lock()
                                .await
                                .insert(task_id.0.clone(), thread.id);
                            info!("discord: created thread {} for task {task_id}", thread.id);
                        }
                        Err(e) => error!("discord: create thread for {task_id}: {e}"),
                    }
                }
                Err(e) => error!("discord: header message for {task_id}: {e}"),
            }
        }

        OrchestratorEvent::TextOutput { task_id, text, .. } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                send(ctx, tid, text).await;
            }
        }

        OrchestratorEvent::ToolStarted { task_id, tool_name, summary, .. } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                send(ctx, tid, &format_tool_started(tool_name, summary)).await;
            }
        }

        OrchestratorEvent::ToolCompleted {
            task_id, tool_name, summary, is_error, output_preview, ..
        } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                send(ctx, tid, &format_tool_completed(tool_name, summary, *is_error, output_preview.as_deref())).await;
            }
        }

        OrchestratorEvent::Thinking { task_id, text, .. } => {
            if show_thinking {
                if let Some(tid) = thread_for(task_id, task_to_thread).await {
                    send(ctx, tid, &format_thinking(text)).await;
                }
            }
        }

        OrchestratorEvent::TurnComplete { task_id, usage, duration_secs, .. } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                send(ctx, tid, &format_turn_complete(*duration_secs, usage.total_cost_usd, usage.input_tokens, usage.output_tokens)).await;
            }
        }

        OrchestratorEvent::TaskStateChanged { task_id, new_state, .. } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                let text = match new_state {
                    TaskStateSummary::Hibernated => "💤 Session hibernated.",
                    TaskStateSummary::Dead => "💀 Task stopped.",
                    TaskStateSummary::Running => "🟢 Task resumed.",
                };
                send(ctx, tid, text).await;
            }
        }

        OrchestratorEvent::Error { task_id, error, next_steps, .. } => {
            let tid = if let Some(id) = task_id {
                thread_for(id, task_to_thread).await
            } else {
                None
            };
            send(ctx, tid.unwrap_or(parent_channel), &format_error(error, next_steps)).await;
        }

        OrchestratorEvent::CommandResponse { task_id, text, .. } => {
            let tid = if let Some(id) = task_id {
                thread_for(id, task_to_thread).await
            } else {
                None
            };
            send(ctx, tid.unwrap_or(parent_channel), text).await;
        }

        OrchestratorEvent::FileOutput { task_id, filename, data, caption, .. } => {
            if let Some(tid) = thread_for(task_id, task_to_thread).await {
                let attachment = CreateAttachment::bytes(data.as_ref().clone(), filename);
                let mut msg = CreateMessage::new().add_file(attachment);
                if let Some(cap) = caption {
                    msg = msg.content(cap);
                }
                if let Err(e) = tid.send_message(ctx, msg).await {
                    warn!("discord: send file for {task_id}: {e}");
                }
            }
        }

        OrchestratorEvent::PhaseChanged { .. }
        | OrchestratorEvent::QueuedMessageDelivered { .. }
        | OrchestratorEvent::MessageQueued { .. }
        | OrchestratorEvent::ConversationRenamed { .. } => {}
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn thread_for(
    task_id: &TaskId,
    task_to_thread: &Mutex<HashMap<String, ChannelId>>,
) -> Option<ChannelId> {
    task_to_thread.lock().await.get(&task_id.0).copied()
}

async fn send(ctx: &Context, channel: ChannelId, text: &str) {
    for chunk in split_message(text, 2000) {
        if let Err(e) = channel
            .send_message(ctx, CreateMessage::new().content(chunk))
            .await
        {
            warn!("discord: send to {channel}: {e}");
        }
    }
}

fn split_message(text: &str, limit: usize) -> Vec<&str> {
    if text.len() <= limit {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut remaining = text;
    while remaining.len() > limit {
        let split_at = remaining[..limit]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(limit);
        chunks.push(&remaining[..split_at]);
        remaining = &remaining[split_at..];
    }
    if !remaining.is_empty() {
        chunks.push(remaining);
    }
    chunks
}

fn event_name(event: &OrchestratorEvent) -> &'static str {
    match event {
        OrchestratorEvent::PhaseChanged { .. } => "PhaseChanged",
        OrchestratorEvent::TextOutput { .. } => "TextOutput",
        OrchestratorEvent::ToolStarted { .. } => "ToolStarted",
        OrchestratorEvent::ToolCompleted { .. } => "ToolCompleted",
        OrchestratorEvent::Thinking { .. } => "Thinking",
        OrchestratorEvent::TurnComplete { .. } => "TurnComplete",
        OrchestratorEvent::TaskCreated { .. } => "TaskCreated",
        OrchestratorEvent::TaskStateChanged { .. } => "TaskStateChanged",
        OrchestratorEvent::Error { .. } => "Error",
        OrchestratorEvent::QueuedMessageDelivered { .. } => "QueuedMessageDelivered",
        OrchestratorEvent::MessageQueued { .. } => "MessageQueued",
        OrchestratorEvent::FileOutput { .. } => "FileOutput",
        OrchestratorEvent::CommandResponse { .. } => "CommandResponse",
        OrchestratorEvent::ConversationRenamed { .. } => "ConversationRenamed",
    }
}

#[cfg(test)]
mod tests {
    use super::split_message;

    #[test]
    fn short_text_is_single_chunk() {
        assert_eq!(split_message("hello", 2000), vec!["hello"]);
    }

    #[test]
    fn long_text_splits_on_newline() {
        let line = "a".repeat(1500);
        let text = format!("{line}\n{line}");
        let chunks = split_message(&text, 2000);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('\n'));
    }

    #[test]
    fn long_text_without_newline_splits_at_limit() {
        let text = "x".repeat(4500);
        let chunks = split_message(&text, 2000);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.len() <= 2000));
    }
}
