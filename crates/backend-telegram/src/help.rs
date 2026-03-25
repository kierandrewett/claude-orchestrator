use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, ThreadId,
};

// ── Callback data constants ───────────────────────────────────────────────────

pub const CB_HELP_TASKS:   &str = "help:tasks";
pub const CB_HELP_MCP:     &str = "help:mcp";
pub const CB_HELP_CONFIG:  &str = "help:config";
pub const CB_HELP_TOPICS:  &str = "help:topics";
pub const CB_HELP_BACK:    &str = "help:back";

// ── Keyboards ─────────────────────────────────────────────────────────────────

pub fn main_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("📋 Task Management", CB_HELP_TASKS),
            InlineKeyboardButton::callback("🔧 MCP Servers",     CB_HELP_MCP),
        ],
        vec![
            InlineKeyboardButton::callback("⚙️ Configuration",  CB_HELP_CONFIG),
            InlineKeyboardButton::callback("💬 Topics",          CB_HELP_TOPICS),
        ],
    ])
}

fn back_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("⬅️ Back", CB_HELP_BACK),
    ]])
}

// ── Text content ──────────────────────────────────────────────────────────────

pub fn main_text() -> &'static str {
    "<b>Claude Orchestrator — Help</b>\n\nChoose a category:"
}

fn tasks_text() -> &'static str {
    "<b>📋 Task Management</b>\n\n\
     <code>/status</code> — List all tasks with state, cost, and idle countdown\n\
     <code>/cancel</code> — Interrupt the current Claude response (session stays alive)\n\
     <code>/stop</code> — Kill the current task permanently\n\
     <code>/hibernate</code> — Suspend the current task\n\
     <code>/cost</code> — Show cost for the current task\n\
     <code>/cost all</code> — Show cost breakdown for all tasks\n\n\
     <b>Examples</b>\n\
     <code>/stop</code> — stop the task in the current topic\n\
     <code>/stop abc123</code> — stop a specific task by ID\n\
     <code>/cost all</code> — see a cost breakdown across every task"
}

fn mcp_text() -> &'static str {
    "<b>🔧 MCP Servers</b>\n\n\
     <code>/mcp</code> — List all configured MCP servers\n\
     <code>/mcp add &lt;name&gt; &lt;command&gt; [args...]</code> — Add a new server\n\
     <code>/mcp remove &lt;name&gt;</code> — Remove a custom server\n\
     <code>/mcp disable &lt;name&gt;</code> — Disable a server (including built-ins)\n\
     <code>/mcp enable &lt;name&gt;</code> — Re-enable a disabled server\n\n\
     The built-in <code>orchestrator</code> server gives Claude tools like \
     <code>rename_conversation</code>.\n\n\
     <b>Examples</b>\n\
     <code>/mcp add filesystem npx @modelcontextprotocol/server-filesystem /home/user</code>\n\
     <code>/mcp add github docker run -i --rm ghcr.io/github/github-mcp-server</code>\n\
     <code>/mcp disable orchestrator</code> — turn off the built-in server"
}

fn config_text() -> &'static str {
    "<b>⚙️ Configuration</b>\n\n\
     <code>/config thinking on|off</code> — Show or hide Claude's internal thinking\n\
     <code>/reconnect &lt;task_id&gt;</code> — Link a topic to an existing task by ID \
     (use <code>/status</code> to find IDs)\n\n\
     <b>Examples</b>\n\
     <code>/config thinking on</code> — show Claude's reasoning in collapsible blocks\n\
     <code>/reconnect 550e8400-e29b-41d4-a716-446655440000</code> — reattach after bot restart"
}

fn topics_text() -> &'static str {
    "<b>💬 Topics &amp; Sessions</b>\n\n\
     <b>Scratchpad</b> — The default topic for quick back-and-forth with Claude. \
     Each response starts a fresh session.\n\n\
     <b>Task topics</b> — Long-running sessions with their own topic. \
     Claude resumes where it left off when you message a sleeping (💤) topic.\n\n\
     <code>/init</code> — Create the Scratchpad topic (first-time setup)\n\
     <code>/rename &lt;name&gt;</code> — Rename the current topic\n\n\
     <b>Topic states</b>\n\
     🟢 Active — Claude is running\n\
     💤 Sleeping — Session is suspended, resumes on next message\n\
     💀 Stopped — Task has ended permanently\n\n\
     <b>Examples</b>\n\
     Message a 💤 topic → Claude wakes up and continues where it left off\n\
     <code>/rename refactor auth</code> — give the current topic a meaningful name\n\
     <code>/hibernate</code> — manually suspend a task to free up resources"
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Send the initial /help message.
pub async fn send_help(bot: &Bot, chat_id: ChatId, thread_id: Option<ThreadId>, reply_to: Option<i32>) {
    let mut req = bot
        .send_message(chat_id, main_text())
        .parse_mode(ParseMode::Html)
        .reply_markup(main_keyboard());
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Some(id) = reply_to {
        use teloxide::types::ReplyParameters;
        req = req.reply_parameters(ReplyParameters::new(MessageId(id as i32)));
    }
    if let Err(e) = req.await {
        tracing::warn!("help: send_message failed: {e}");
    }
}

/// Handle a callback query from one of the help keyboard buttons.
pub async fn handle_callback(bot: Bot, query: CallbackQuery) {
    let data = match query.data.as_deref() {
        Some(d) if d.starts_with("help:") => d,
        _ => return,
    };

    // Always acknowledge the query to remove the loading spinner.
    let _ = bot.answer_callback_query(&query.id).await;

    let msg = match &query.message {
        Some(m) => m,
        None => return,
    };

    let (new_text, keyboard) = match data {
        CB_HELP_TASKS  => (tasks_text(),  back_keyboard()),
        CB_HELP_MCP    => (mcp_text(),    back_keyboard()),
        CB_HELP_CONFIG => (config_text(), back_keyboard()),
        CB_HELP_TOPICS => (topics_text(), back_keyboard()),
        CB_HELP_BACK   => (main_text(),   main_keyboard()),
        _              => return,
    };

    let _ = bot
        .edit_message_text(msg.chat().id, msg.id(), new_text)
        .parse_mode(ParseMode::Html)
        .reply_markup(keyboard)
        .await;
}
