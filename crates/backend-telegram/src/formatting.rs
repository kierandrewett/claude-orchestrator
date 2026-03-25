use claude_events::TaskStateSummary;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, CodeBlockKind};

// ---------------------------------------------------------------------------
// Emoji title helpers
// ---------------------------------------------------------------------------

/// Split a title like "🤔 General chat" into ("🤔", "General chat").
///
/// The first segment (before the first space) is treated as an emoji if it
/// contains no ASCII alphanumeric characters. Returns (None, full_title) if
/// no emoji prefix is detected.
pub fn split_emoji_from_title(title: &str) -> (Option<String>, String) {
    let title = title.trim();
    if let Some(space_pos) = title.find(' ') {
        let candidate = &title[..space_pos];
        let rest = title[space_pos..].trim().to_string();
        let looks_like_emoji = !candidate.is_empty()
            && !candidate.chars().any(|c| c.is_ascii_alphanumeric());
        if looks_like_emoji {
            return (Some(candidate.to_string()), rest);
        }
    }
    (None, title.to_string())
}

// ---------------------------------------------------------------------------
// Name helpers
// ---------------------------------------------------------------------------

/// Convert a camelCase or snake_case identifier to Title Case words.
/// "ToolSearch" → "Tool Search", "rename_conversation" → "Rename Conversation"
fn to_title_case(s: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();

    for ch in s.chars() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else if ch.is_uppercase() && !current.is_empty() {
            words.push(std::mem::take(&mut current));
            current.push(ch);
        } else if current.is_empty() {
            current.push(ch.to_ascii_uppercase());
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words.join(" ")
}

/// Parse `mcp__server__tool_name` → `("Server", "Tool Name")`.
fn parse_mcp_name(tool_name: &str) -> Option<(String, String)> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    Some((to_title_case(server), to_title_case(tool)))
}

/// Human-readable display name for any tool.
fn display_name(tool_name: &str) -> String {
    if let Some((server, tool)) = parse_mcp_name(tool_name) {
        format!("{server}: {tool}")
    } else {
        to_title_case(tool_name)
    }
}

// ---------------------------------------------------------------------------
// Emoji + detail helpers
// ---------------------------------------------------------------------------

/// Return an emoji for a given tool name.
fn tool_emoji(tool_name: &str) -> &'static str {
    if parse_mcp_name(tool_name).is_some() {
        return "🔌";
    }
    match tool_name {
        "Bash" => "💻",
        "Read" => "📖",
        "Write" => "✍️",
        "Edit" => "✏️",
        "NotebookEdit" => "📓",
        "Glob" | "Grep" => "🔍",
        "WebFetch" | "WebSearch" => "🌐",
        "Agent" => "🤖",
        "TodoWrite" | "TodoRead" => "📋",
        "Task" | "TaskOutput" | "TaskStop" => "🗂",
        "ToolSearch" => "🔍",
        "AskUserQuestion" => "❓",
        _ => "🔧",
    }
}

/// Format JSON args as `Key: value` lines (used for MCP tools and fallback).
fn format_kv_args(summary: &str) -> String {
    let val: serde_json::Value = serde_json::from_str(summary).unwrap_or_default();
    if let Some(obj) = val.as_object() {
        let lines: Vec<String> = obj
            .iter()
            .map(|(k, v)| {
                let key = to_title_case(k);
                let value = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let truncated: String = value.chars().take(200).collect();
                format!("{key}: {truncated}")
            })
            .collect();
        lines.join("\n")
    } else {
        summary.chars().take(300).collect()
    }
}

/// For the Agent tool, extract display name and detail text.
fn agent_display(summary: &str) -> (String, String) {
    let val: serde_json::Value = serde_json::from_str(summary).unwrap_or_default();
    let subagent_type = val
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Agent")
        .to_string();
    let description = val
        .get("description")
        .or_else(|| val.get("prompt"))
        .and_then(|v| v.as_str())
        .unwrap_or(summary);
    let truncated: String = description.chars().take(300).collect();
    let detail = format!("<code>   ❯ {}</code>", escape_html(&truncated));
    (subagent_type, detail)
}

/// Build the detail block shown below the tool name line.
fn tool_detail(tool_name: &str, summary: &str) -> String {
    // MCP tools: each key-value arg on its own indented line.
    if parse_mcp_name(tool_name).is_some() {
        let args = format_kv_args(summary);
        if args.is_empty() { return String::new(); }
        let indented: String = args.lines()
            .map(|l| format!("   ❯ {}", l))
            .collect::<Vec<_>>()
            .join("\n");
        return format!("<code>{}</code>", escape_html(&indented));
    }

    let val: serde_json::Value = serde_json::from_str(summary).unwrap_or_default();
    let field = match tool_name {
        "Bash" => val.get("command"),
        "Read" | "Write" | "Edit" | "NotebookEdit" => val.get("file_path"),
        "Glob" | "Grep" => val.get("pattern"),
        "WebFetch" | "WebSearch" => val.get("url").or_else(|| val.get("query")),
        "Agent" => val.get("description").or_else(|| val.get("prompt")),
        "ToolSearch" => val.get("query"),
        _ => None,
    };
    let text = field.and_then(|v| v.as_str()).unwrap_or(summary);
    let truncated: String = text.chars().take(300).collect();
    format!("<code>   ❯ {}</code>", escape_html(&truncated))
}

// ---------------------------------------------------------------------------
// Public formatters
// ---------------------------------------------------------------------------

/// Format a tool_started event.
pub fn format_tool_started(tool_name: &str, summary: &str) -> String {
    let emoji = tool_emoji(tool_name);
    let name = if tool_name == "Agent" {
        let (subagent_type, detail) = agent_display(summary);
        return format!("{emoji} <b>{}</b>\n{}", escape_html(&subagent_type), detail);
    } else {
        display_name(tool_name)
    };
    let detail = tool_detail(tool_name, summary);
    if detail.is_empty() {
        format!("{emoji} <b>{}</b>", escape_html(&name))
    } else {
        format!("{emoji} <b>{}</b>\n{}", escape_html(&name), detail)
    }
}

/// Format a tool_completed event (replaces the started message).
pub fn format_tool_completed(
    tool_name: &str,
    summary: &str,
    is_error: bool,
    preview: Option<&str>,
) -> String {
    let emoji = tool_emoji(tool_name);
    let status = if is_error { "❌" } else { "✅" };
    let (name, detail) = if tool_name == "Agent" {
        agent_display(summary)
    } else {
        (display_name(tool_name), tool_detail(tool_name, summary))
    };
    let preview_str = preview
        .filter(|p| !p.is_empty())
        .map(|p| {
            let lines: Vec<&str> = p.lines().collect();
            let total = lines.len();
            let text = if total <= 5 {
                lines.join("\n")
            } else {
                let first2 = lines[..2].join("\n");
                let last3 = lines[lines.len() - 3..].join("\n");
                format!("{}\n…({} lines total)…\n{}", first2, total, last3)
            };
            let truncated: String = text.chars().take(600).collect();
            format!("\n<code>{}</code>", escape_html(&truncated))
        })
        .unwrap_or_default();
    if detail.is_empty() {
        format!("{status} {emoji} <b>{}</b>{}", escape_html(&name), preview_str)
    } else {
        format!("{status} {emoji} <b>{}</b>\n{}{}", escape_html(&name), detail, preview_str)
    }
}

/// Format a turn_complete event as an inline suffix (italic, appended on new line).
pub fn format_turn_complete(duration_secs: f64) -> String {
    format!("<code>❯ in {:.1}s</code>", duration_secs)
}

/// Format a status message.
pub fn format_status(name: &str, state: &TaskStateSummary, profile: &str) -> String {
    let emoji = match state {
        TaskStateSummary::Running => "🟢",
        TaskStateSummary::Hibernated => "💤",
        TaskStateSummary::Dead => "💀",
    };
    format!("{emoji} {} ({})", escape_html(name), escape_html(profile))
}

/// Format an error with next steps.
pub fn format_error(error: &str, next_steps: &[String]) -> String {
    let mut msg = format!("❌ {}", escape_html(error));
    if !next_steps.is_empty() {
        msg.push_str("\n\nNext steps:");
        for step in next_steps {
            msg.push_str(&format!("\n• {}", escape_html(step)));
        }
    }
    msg
}

/// Format thinking as a spoiler.
pub fn format_thinking(text: &str) -> String {
    let snippet: String = text.chars().take(500).collect();
    format!("🤔 <tg-spoiler>{}</tg-spoiler>", escape_html(&snippet))
}

/// Format a hibernated notice.
pub fn format_hibernated() -> &'static str {
    "💤 Session hibernated. Send a message to wake it."
}

/// Escape HTML special characters for Telegram HTML parse mode.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Convert a markdown string to Telegram-compatible HTML.
///
/// Telegram HTML supports: <b>, <i>, <u>, <s>, <code>, <pre>, <a>, <tg-spoiler>.
/// Tables are rendered as fixed-width code blocks since Telegram has no table support.
pub fn md_to_telegram_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(markdown, opts);
    let mut html = String::with_capacity(markdown.len());
    let mut in_code_block = false;

    // Table state.
    let mut in_table = false;
    let mut in_table_head = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();

    for event in parser {
        // --- Table accumulation ---
        if in_table {
            match event {
                Event::End(TagEnd::Table) => {
                    in_table = false;
                    html.push_str(&render_table(&table_rows));
                    table_rows.clear();
                }
                Event::Start(Tag::TableHead) => { in_table_head = true; }
                Event::End(TagEnd::TableHead) => {
                    in_table_head = false;
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                Event::Start(Tag::TableRow) => {}
                Event::End(TagEnd::TableRow) => {
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                Event::Start(Tag::TableCell) => { current_cell.clear(); }
                Event::End(TagEnd::TableCell) => {
                    current_row.push(std::mem::take(&mut current_cell));
                }
                Event::Text(t) | Event::Code(t) => { current_cell.push_str(&t); }
                Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => {}
                Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => {}
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table_rows.clear();
                current_row.clear();
                current_cell.clear();
            }

            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => html.push_str("\n\n"),

            Event::Start(Tag::Heading { .. }) => html.push_str("<b>"),
            Event::End(TagEnd::Heading(_)) => html.push_str("</b>\n\n"),

            Event::Start(Tag::Strong) => html.push_str("<b>"),
            Event::End(TagEnd::Strong) => html.push_str("</b>"),

            Event::Start(Tag::Emphasis) => html.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => html.push_str("</i>"),

            Event::Start(Tag::Strikethrough) => html.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => html.push_str("</s>"),

            Event::Start(Tag::Link { dest_url, .. }) => {
                html.push_str(&format!("<a href=\"{}\">", escape_html(&dest_url)));
            }
            Event::End(TagEnd::Link) => html.push_str("</a>"),

            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                        html.push_str(&format!(
                            "<pre><code class=\"language-{}\">",
                            escape_html(&lang)
                        ));
                    }
                    _ => html.push_str("<pre><code>"),
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                html.push_str("</code></pre>\n");
            }

            Event::Start(Tag::Item) => html.push_str("• "),
            Event::End(TagEnd::Item) => html.push('\n'),

            Event::Start(Tag::List(_)) | Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::BlockQuote(_)) | Event::End(TagEnd::BlockQuote(_)) => {}

            Event::Code(text) => {
                html.push_str(&format!("<code>{}</code>", escape_html(&text)));
            }

            Event::Text(text) => {
                html.push_str(&escape_html(&text));
            }

            Event::SoftBreak => html.push('\n'),
            Event::HardBreak => html.push_str("\n\n"),
            Event::Rule => html.push_str("\n─────\n"),

            Event::Html(raw) | Event::InlineHtml(raw) => html.push_str(&raw),

            _ => {}
        }
    }

    html.trim_end().to_string()
}

/// Render a collected table as a fixed-width `<pre><code>` block.
fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    // Compute max width per column.
    let mut widths = vec![0usize; col_count];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::from("<pre><code>");
    for (ri, row) in rows.iter().enumerate() {
        let line: Vec<String> = (0..col_count)
            .map(|i| {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                format!("{:<width$}", cell, width = widths[i])
            })
            .collect();
        out.push_str(&escape_html(&line.join("  ")));
        out.push('\n');
        // Separator after header row.
        if ri == 0 {
            let sep: Vec<String> = widths.iter().map(|&w| "─".repeat(w)).collect();
            out.push_str(&escape_html(&sep.join("  ")));
            out.push('\n');
        }
    }
    out.push_str("</code></pre>\n");
    out
}
