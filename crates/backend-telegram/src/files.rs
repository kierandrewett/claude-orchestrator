use std::sync::Arc;

use anyhow::Result;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::InputFile;

/// Download a Telegram file and return its bytes.
pub async fn download_file(bot: &Bot, file_id: &str) -> Result<Vec<u8>> {
    let file = bot
        .get_file(file_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_file failed: {e}"))?;

    let mut data = Vec::new();
    bot.download_file(&file.path, &mut data)
        .await
        .map_err(|e| anyhow::anyhow!("download_file failed: {e}"))?;
    Ok(data)
}

/// Send a file as a document.
///
/// Note: `message_thread_id` is not supported in multipart requests by teloxide 0.10.
/// Instead, pass `reply_to_message_id` to a message already in the target topic —
/// Telegram will automatically place the document in the same thread.
pub async fn send_document(
    bot: &Bot,
    chat_id: ChatId,
    reply_to_message_id: Option<i32>,
    data: Arc<Vec<u8>>,
    filename: &str,
    caption: Option<&str>,
) -> Result<()> {
    let file = InputFile::memory((*data).clone()).file_name(filename.to_string());
    let mut req = bot.send_document(chat_id, file);
    if let Some(reply_id) = reply_to_message_id {
        use teloxide::types::{MessageId, ReplyParameters};
        req = req.reply_parameters(ReplyParameters::new(MessageId(reply_id)));
    }
    if let Some(cap) = caption {
        req = req.caption(cap);
    }
    req.await
        .map_err(|e| anyhow::anyhow!("send_document failed: {e}"))?;
    Ok(())
}
