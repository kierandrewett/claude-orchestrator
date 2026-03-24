use claude_events::{BackendEvent, BackendSource, MessageRef, ParsedCommand, TaskId};

use crate::backend::Data;

pub type Error = anyhow::Error;
pub type Context<'a> = poise::Context<'a, Data, Error>;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Infer the task ID from the channel the command was invoked in.
async fn infer_task_id(ctx: &Context<'_>) -> Option<TaskId> {
    let t2t = ctx.data().thread_to_task.lock().await;
    t2t.get(&ctx.channel_id()).map(|s| TaskId(s.clone()))
}

async fn dispatch(ctx: &Context<'_>, cmd: ParsedCommand) -> Result<(), Error> {
    let task_id = infer_task_id(ctx).await;
    let msg_ref = MessageRef::new("discord", ctx.id().to_string());
    let source = BackendSource::new("discord", ctx.author().id.to_string());
    ctx.data()
        .backend_tx
        .send(BackendEvent::Command {
            command: cmd,
            task_id,
            message_ref: msg_ref,
            source,
        })
        .await?;
    Ok(())
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Create a new Claude task.
#[poise::command(slash_command, guild_only)]
pub async fn new(
    ctx: Context<'_>,
    #[description = "Profile name (e.g. rust, python)"] profile: String,
    #[description = "Initial prompt for Claude"] prompt: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(&ctx, ParsedCommand::New { profile, prompt }).await?;
    ctx.say("Creating task…").await?;
    Ok(())
}

/// Stop a task (defaults to the task for the current thread).
#[poise::command(slash_command, guild_only)]
pub async fn stop(
    ctx: Context<'_>,
    #[description = "Task ID to stop (leave blank to stop the task for this thread)"]
    task_id: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(
        &ctx,
        ParsedCommand::Stop {
            task_id: task_id.map(TaskId),
        },
    )
    .await?;
    ctx.say("Stopping task…").await?;
    Ok(())
}

/// Show status of all active tasks.
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(&ctx, ParsedCommand::Status).await?;
    ctx.say("Fetching status…").await?;
    Ok(())
}

/// Show cost for the current task or all tasks.
#[poise::command(slash_command, guild_only)]
pub async fn cost(
    ctx: Context<'_>,
    #[description = "Show cost for all tasks"] all: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(&ctx, ParsedCommand::Cost { all: all.unwrap_or(false) }).await?;
    ctx.say("Fetching cost…").await?;
    Ok(())
}

/// Hibernate the task for the current thread.
#[poise::command(slash_command, guild_only)]
pub async fn hibernate(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(&ctx, ParsedCommand::Hibernate).await?;
    ctx.say("Hibernating task…").await?;
    Ok(())
}

/// Set a config option for the current task (e.g. /config thinking on).
#[poise::command(slash_command, guild_only)]
pub async fn config(
    ctx: Context<'_>,
    #[description = "Config key (e.g. thinking)"] key: String,
    #[description = "Config value (e.g. on / off)"] value: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    dispatch(&ctx, ParsedCommand::Config { key, value }).await?;
    ctx.say("Config updated.").await?;
    Ok(())
}

pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![new(), stop(), status(), cost(), hibernate(), config()]
}
