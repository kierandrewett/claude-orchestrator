use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, ThreadId,
};
use tokio::sync::mpsc;
use tracing::warn;

use claude_events::{BackendEvent, BackendSource, MessageRef, TaskId};

use crate::formatting::escape_html;

pub const AQ_PREFIX: &str = "aq:";

/// Parse the JSON summary from an AskUserQuestion ToolStarted event.
/// Returns (header, question, options) on success.
pub fn parse(summary: &str) -> Option<(String, String, Vec<String>)> {
    let json: serde_json::Value = serde_json::from_str(summary).ok()?;
    let header = json["header"].as_str().unwrap_or("Question").to_string();
    let question = json["question"].as_str().unwrap_or("").to_string();

    // Options may be a JSON array OR a JSON-encoded string containing an array.
    let options: Vec<String> = if let Some(arr) = json["options"].as_array() {
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    } else if let Some(s) = json["options"].as_str() {
        serde_json::from_str::<Vec<String>>(s).unwrap_or_default()
    } else {
        vec![]
    };

    Some((header, question, options))
}

/// Send an AskUserQuestion as a formatted Telegram message with inline buttons.
/// Returns the sent message ID.
pub async fn send(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    reply_to: Option<i32>,
    task_id: &str,
    header: &str,
    question: &str,
    options: &[String],
) -> Option<MessageId> {
    let text = build_text(header, question, options);
    let keyboard = build_keyboard(task_id, options);

    let mut req = bot
        .send_message(chat_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(keyboard);

    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Some(id) = reply_to {
        use teloxide::types::ReplyParameters;
        req = req.reply_parameters(ReplyParameters::new(MessageId(id)));
    }

    match req.await {
        Ok(msg) => Some(msg.id),
        Err(e) => {
            warn!("ask_question: failed to send: {e}");
            None
        }
    }
}

fn build_text(header: &str, question: &str, options: &[String]) -> String {
    let mut lines = vec![format!("❓ <b>{}</b>", escape_html(header))];
    if !question.is_empty() {
        lines.push(String::new());
        lines.push(escape_html(question));
    }
    lines.join("\n")
}

fn build_keyboard(task_id: &str, options: &[String]) -> InlineKeyboardMarkup {
    let rows: Vec<Vec<InlineKeyboardButton>> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            // Callback data: "aq:{task_id}:{index}" — stays well under 64 bytes.
            let data = format!("{AQ_PREFIX}{task_id}:{i}");
            vec![InlineKeyboardButton::callback(opt.clone(), data)]
        })
        .collect();
    InlineKeyboardMarkup::new(rows)
}

/// Handle an inline button callback for an AskUserQuestion.
/// Sends the selected option back as a user message to the task.
pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    sender: mpsc::Sender<BackendEvent>,
    task_id: &str,
    option_text: &str,
) {
    let _ = bot.answer_callback_query(&query.id).await;

    let user_id = query.from.id.0.to_string();
    let msg_ref = MessageRef::new("telegram", format!("aq_btn:{}", query.id));
    let source = BackendSource::new("telegram", user_id);

    let _ = sender
        .send(BackendEvent::UserMessage {
            task_id: TaskId(task_id.to_string()),
            text: option_text.to_string(),
            message_ref: msg_ref,
            source,
        })
        .await;
}
