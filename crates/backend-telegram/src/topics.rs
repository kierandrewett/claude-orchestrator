use anyhow::Result;
use teloxide::prelude::*;
use teloxide::types::ThreadId;
use tracing::warn;

/// Create a new forum topic for a task.
pub async fn create_task_topic(bot: &Bot, group_id: ChatId, task_name: &str) -> Result<ThreadId> {
    let result = bot
        .create_forum_topic(group_id, task_name, 0x6FB9F0_u32, "")
        .await
        .map_err(|e| anyhow::anyhow!("create_forum_topic failed: {e}"))?;
    Ok(result.thread_id)
}

/// Create the Scratchpad topic with a pencil-and-paper emoji icon.
pub async fn create_scratchpad_topic(bot: &Bot, group_id: ChatId, name: &str) -> Result<ThreadId> {
    let emoji_id = find_icon_emoji_id(bot, "📝").await.unwrap_or_default();
    let result = bot
        .create_forum_topic(group_id, name, 0xFFD67E_u32, emoji_id)
        .await
        .map_err(|e| anyhow::anyhow!("create_forum_topic failed: {e}"))?;
    Ok(result.thread_id)
}

/// Look up the custom emoji sticker ID for a given emoji character from the
/// set of allowed forum topic icon stickers.
async fn find_icon_emoji_id(bot: &Bot, emoji: &str) -> Option<String> {
    match bot.get_forum_topic_icon_stickers().await {
        Ok(stickers) => stickers
            .into_iter()
            .find(|s| s.emoji.as_deref().map_or(false, |e| e.contains(emoji)))
            .and_then(|s| s.custom_emoji_id().map(|id| id.to_owned())),
        Err(e) => {
            warn!("telegram: failed to fetch topic icon stickers: {e}");
            None
        }
    }
}
