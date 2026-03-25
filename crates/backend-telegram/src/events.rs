use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, ThreadId,
};
use tokio::sync::{mpsc, Mutex};
use tracing::warn;

use claude_events::{BackendEvent, BackendSource, EventListEntry, MessageRef, ParsedCommand};

/// Shared reference to the last-sent events list message, used to edit in place.
pub type LastEventsMsg = Arc<Mutex<Option<(ChatId, MessageId)>>>;

// ── Callback data constants ───────────────────────────────────────────────────

const CB_EVT_REFRESH: &str = "evt:refresh";
const CB_EVT_ON_PREFIX: &str = "evt:on:";
const CB_EVT_OFF_PREFIX: &str = "evt:off:";
const CB_EVT_DEL_PREFIX: &str = "evt:del:";

// ── UI builders ───────────────────────────────────────────────────────────────

pub fn build_text(entries: &[EventListEntry]) -> String {
    let mut lines = vec!["<b>📅 Scheduled Events</b>".to_string(), String::new()];

    if entries.is_empty() {
        lines.push("No scheduled events.".to_string());
    } else {
        for e in entries {
            let status = if e.enabled { "✅" } else { "⏸️" };
            let mode = if e.mode == "once" { "🔂" } else { "🔁" };
            let next = e
                .next_run
                .as_deref()
                .unwrap_or(if e.enabled { "not scheduled" } else { "paused" });
            lines.push(format!(
                "{status} {mode} <b>{}</b>",
                crate::formatting::escape_html(&e.name),
            ));
            lines.push(format!(
                "   <code>{}</code> — next: {}",
                crate::formatting::escape_html(&e.schedule),
                crate::formatting::escape_html(next),
            ));
        }
    }

    lines.push(String::new());
    lines.push(
        "Toggle with the buttons below. Use <code>/events info &lt;id&gt;</code> for details."
            .to_string(),
    );
    lines.join("\n")
}

fn build_keyboard(entries: &[EventListEntry]) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = entries
        .iter()
        .map(|e| {
            let id_short = &e.id[..8.min(e.id.len())];
            if e.enabled {
                vec![
                    InlineKeyboardButton::callback(
                        format!("⏸️ {}", e.name),
                        format!("{}{}", CB_EVT_OFF_PREFIX, id_short),
                    ),
                    InlineKeyboardButton::callback(
                        "🗑️",
                        format!("{}{}", CB_EVT_DEL_PREFIX, id_short),
                    ),
                ]
            } else {
                vec![
                    InlineKeyboardButton::callback(
                        format!("▶️ {}", e.name),
                        format!("{}{}", CB_EVT_ON_PREFIX, id_short),
                    ),
                    InlineKeyboardButton::callback(
                        "🗑️",
                        format!("{}{}", CB_EVT_DEL_PREFIX, id_short),
                    ),
                ]
            }
        })
        .collect();
    rows.push(vec![InlineKeyboardButton::callback("🔄 Refresh", CB_EVT_REFRESH)]);
    InlineKeyboardMarkup::new(rows)
}

// ── Send / edit ───────────────────────────────────────────────────────────────

/// Send a new events list message and return its ID.
pub async fn send_events_list(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    reply_to: Option<i32>,
    entries: &[EventListEntry],
) -> Option<MessageId> {
    let mut req = bot
        .send_message(chat_id, build_text(entries))
        .parse_mode(ParseMode::Html)
        .reply_markup(build_keyboard(entries));
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Some(id) = reply_to {
        use teloxide::types::ReplyParameters;
        req = req.reply_parameters(ReplyParameters::new(MessageId(id as i32)));
    }
    match req.await {
        Ok(msg) => Some(msg.id),
        Err(e) => {
            warn!("events: send_events_list failed: {e}");
            None
        }
    }
}

/// Edit an existing events list message in place.
pub async fn edit_events_list(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    entries: &[EventListEntry],
) {
    let _ = bot
        .edit_message_text(chat_id, message_id, build_text(entries))
        .parse_mode(ParseMode::Html)
        .reply_markup(build_keyboard(entries))
        .await;
}

// ── Callback handler ──────────────────────────────────────────────────────────

pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    sender: mpsc::Sender<BackendEvent>,
    last_events_msg: LastEventsMsg,
) {
    let data = match query.data.as_deref() {
        Some(d) if d.starts_with("evt:") => d,
        _ => return,
    };

    let _ = bot.answer_callback_query(&query.id).await;

    // Record the message that had the buttons so the event loop can edit it.
    if let Some(ref msg) = query.message {
        *last_events_msg.lock().await = Some((msg.chat().id, msg.id()));
    }

    let cmd = if data == CB_EVT_REFRESH {
        ParsedCommand::EventsList
    } else if let Some(id) = data.strip_prefix(CB_EVT_ON_PREFIX) {
        ParsedCommand::EventsEnable { id: id.to_string() }
    } else if let Some(id) = data.strip_prefix(CB_EVT_OFF_PREFIX) {
        ParsedCommand::EventsDisable { id: id.to_string() }
    } else if let Some(id) = data.strip_prefix(CB_EVT_DEL_PREFIX) {
        ParsedCommand::EventsDelete { id: id.to_string() }
    } else {
        return;
    };

    let user_id = query.from.id.0.to_string();
    let _ = sender
        .send(BackendEvent::Command {
            command: cmd,
            task_id: None,
            message_ref: MessageRef::new("telegram", format!("btn:{}", query.id)),
            source: BackendSource::new("telegram", user_id),
        })
        .await;
}
