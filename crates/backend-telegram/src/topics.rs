use anyhow::Result;
use teloxide::prelude::*;
use teloxide::types::ThreadId;

/// Create a new forum topic for a task.
pub async fn create_task_topic(bot: &Bot, group_id: ChatId, task_name: &str) -> Result<ThreadId> {
    let result = bot
        .create_forum_topic(group_id, task_name, 0x6FB9F0_u32, "")
        .await
        .map_err(|e| anyhow::anyhow!("create_forum_topic failed: {e}"))?;
    Ok(result.thread_id)
}

/// Create or find the Scratchpad topic (creates if it doesn't exist).
pub async fn create_scratchpad_topic(bot: &Bot, group_id: ChatId, name: &str) -> Result<ThreadId> {
    // In the full implementation, check if a topic with this name already exists.
    // For now, just create a new one.
    create_task_topic(bot, group_id, name).await
}
