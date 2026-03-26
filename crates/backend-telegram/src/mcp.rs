use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, ThreadId,
};
use tokio::sync::{mpsc, Mutex};
use tracing::warn;

use claude_events::{BackendEvent, BackendSource, McpEntry, MessageRef, ParsedCommand};

const MCP_TOOL_PREFIX: &str = "mcp__orchestrator__";

/// Shared reference to the last-sent MCP list message, used to edit in place.
pub type LastMcpMsg = Arc<Mutex<Option<(ChatId, MessageId)>>>;

// ── Callback data constants ───────────────────────────────────────────────────

const CB_MCP_REFRESH: &str = "mcp:refresh";
const CB_MCP_ON_PREFIX: &str = "mcp:on:";
const CB_MCP_OFF_PREFIX: &str = "mcp:off:";

// ── UI builders ───────────────────────────────────────────────────────────────

pub fn build_text(entries: &[McpEntry], session_tools: &[String]) -> String {
    let mut lines = vec!["<b>🔧 MCP Servers</b>".to_string(), String::new()];
    for e in entries {
        let status = if e.enabled { "✅" } else { "❌" };
        let detail = if e.is_builtin {
            "built-in".to_string()
        } else if let Some(ref url) = e.url {
            format!("<code>{}</code>", crate::formatting::escape_html(url))
        } else {
            let mut parts = vec![e.command.as_deref().unwrap_or("").to_string()];
            parts.extend(e.args.iter().cloned());
            format!("<code>{}</code>", crate::formatting::escape_html(&parts.join(" ")))
        };
        lines.push(format!(
            "{status} <b>{}</b> — {}",
            crate::formatting::escape_html(&e.name),
            detail
        ));
    }

    // Show tools active in the current session if available.
    let orch_tools: Vec<&str> = session_tools
        .iter()
        .filter_map(|t| t.strip_prefix(MCP_TOOL_PREFIX))
        .collect();
    if !orch_tools.is_empty() {
        lines.push(String::new());
        lines.push("<b>Active orchestrator tools:</b>".to_string());
        for tool in &orch_tools {
            lines.push(format!("  • <code>{}</code>", crate::formatting::escape_html(tool)));
        }
    }

    lines.push(String::new());
    lines.push(
        "Press a server to toggle it. Use <code>/mcp add NAME URL [TOKEN]</code> or \
         <code>/mcp add NAME CMD [args…]</code> to add a server."
            .to_string(),
    );
    lines.join("\n")
}

fn build_keyboard(entries: &[McpEntry]) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = entries
        .iter()
        .map(|e| {
            let (label, data) = if e.enabled {
                (
                    format!("✅ {}", e.name),
                    format!("{}{}", CB_MCP_OFF_PREFIX, e.name),
                )
            } else {
                (
                    format!("❌ {}", e.name),
                    format!("{}{}", CB_MCP_ON_PREFIX, e.name),
                )
            };
            vec![InlineKeyboardButton::callback(label, data)]
        })
        .collect();
    rows.push(vec![InlineKeyboardButton::callback("🔄 Refresh", CB_MCP_REFRESH)]);
    InlineKeyboardMarkup::new(rows)
}

// ── Send / edit ───────────────────────────────────────────────────────────────

/// Send a new MCP list message and return its ID.
pub async fn send_mcp_list(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    reply_to: Option<i32>,
    entries: &[McpEntry],
    session_tools: &[String],
) -> Option<MessageId> {
    let mut req = bot
        .send_message(chat_id, build_text(entries, session_tools))
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
            warn!("mcp: send_mcp_list failed: {e}");
            None
        }
    }
}

/// Edit an existing MCP list message in place.
pub async fn edit_mcp_list(bot: &Bot, chat_id: ChatId, message_id: MessageId, entries: &[McpEntry], session_tools: &[String]) {
    let _ = bot
        .edit_message_text(chat_id, message_id, build_text(entries, session_tools))
        .parse_mode(ParseMode::Html)
        .reply_markup(build_keyboard(entries))
        .await;
}

// ── Callback handler ──────────────────────────────────────────────────────────

pub async fn handle_callback(
    bot: Bot,
    query: CallbackQuery,
    sender: mpsc::Sender<BackendEvent>,
    last_mcp_msg: LastMcpMsg,
) {
    let data = match query.data.as_deref() {
        Some(d) if d.starts_with("mcp:") => d,
        _ => return,
    };

    let _ = bot.answer_callback_query(&query.id).await;

    // Record the message that had the buttons so the event loop can edit it.
    if let Some(ref msg) = query.message {
        *last_mcp_msg.lock().await = Some((msg.chat().id, msg.id()));
    }

    let cmd = if data == CB_MCP_REFRESH {
        ParsedCommand::McpList
    } else if let Some(name) = data.strip_prefix(CB_MCP_ON_PREFIX) {
        ParsedCommand::McpEnable { name: name.to_string() }
    } else if let Some(name) = data.strip_prefix(CB_MCP_OFF_PREFIX) {
        ParsedCommand::McpDisable { name: name.to_string() }
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
