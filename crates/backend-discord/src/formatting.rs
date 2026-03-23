//! Discord message formatting (uses Discord markdown: **bold**, `code`, etc.)

/// Format a tool_started event as a compact one-liner.
pub fn format_tool_started(tool_name: &str, summary: &str) -> String {
    format!("🔧 **{tool_name}**: {summary}")
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
        .map(|p| format!("\n```\n{}\n```", p.chars().take(500).collect::<String>()))
        .unwrap_or_default();
    format!("🔧 {tool_name} → {status} {summary}{preview_str}")
}

/// Format a turn_complete event.
pub fn format_turn_complete(
    duration_secs: f64,
    total_cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
) -> String {
    format!(
        "✅ Done — {:.1}s · ${:.4} ({} in / {} out)",
        duration_secs, total_cost_usd, input_tokens, output_tokens
    )
}

/// Format an error with next steps.
pub fn format_error(error: &str, next_steps: &[String]) -> String {
    let mut msg = format!("❌ {error}");
    if !next_steps.is_empty() {
        msg.push_str("\n\n**Next steps:**");
        for step in next_steps {
            msg.push_str(&format!("\n• {step}"));
        }
    }
    msg
}

/// Format thinking text as a Discord spoiler block.
pub fn format_thinking(text: &str) -> String {
    format!("||🤔 {}||", text.chars().take(500).collect::<String>())
}

/// Header posted to the parent channel when a task is created.
/// A thread is then created from this message.
pub fn format_task_header(name: &str, profile: &str) -> String {
    format!("**📋 Task: {name}** · profile `{profile}`")
}
