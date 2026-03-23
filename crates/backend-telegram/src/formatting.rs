use claude_events::TaskStateSummary;

/// Format a tool_started event as a compact one-liner.
pub fn format_tool_started(tool_name: &str, summary: &str) -> String {
    format!("🔧 {tool_name}: {summary}")
}

/// Format a tool_completed event.
pub fn format_tool_completed(
    tool_name: &str,
    summary: &str,
    is_error: bool,
    preview: Option<&str>,
) -> String {
    let status = if is_error { "❌" } else { "✅" };
    let preview_str = preview
        .map(|p| format!("\n`{}`", escape_markdown(p)))
        .unwrap_or_default();
    format!("🔧 {} → {} {}{}", tool_name, status, summary, preview_str)
}

/// Format a turn_complete event.
pub fn format_turn_complete(
    duration_secs: f64,
    total_cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
) -> String {
    format!(
        "✅ Done — {:.1}s, ${:.4} ({} in / {} out)",
        duration_secs, total_cost_usd, input_tokens, output_tokens
    )
}

/// Format a status message.
pub fn format_status(name: &str, state: &TaskStateSummary, profile: &str) -> String {
    let emoji = match state {
        TaskStateSummary::Running => "🟢",
        TaskStateSummary::Hibernated => "💤",
        TaskStateSummary::Dead => "💀",
    };
    format!("{emoji} {name} ({profile})")
}

/// Format an error with next steps.
pub fn format_error(error: &str, next_steps: &[String]) -> String {
    let mut msg = format!("❌ {}", error);
    if !next_steps.is_empty() {
        msg.push_str("\n\nNext steps:");
        for step in next_steps {
            msg.push_str(&format!("\n• {step}"));
        }
    }
    msg
}

/// Format thinking as a spoiler.
pub fn format_thinking(text: &str) -> String {
    format!("🤔 ||{}||", text.chars().take(500).collect::<String>())
}

/// Format a hibernated notice.
pub fn format_hibernated() -> &'static str {
    "💤 Session hibernated. Send a message to wake it."
}

/// Escape special MarkdownV2 characters.
pub fn escape_markdown(text: &str) -> String {
    let special = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut result = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if special.contains(&c) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}
