use serde::{Deserialize, Serialize};

// ── ClaudeEvent — every top-level line Claude Code emits on stdout ─────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeEvent {
    System(SystemInfo),
    Assistant(AssistantMessage),
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseRequest),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultEvent),
    Result(FinalResult),
    /// Catch-all for unknown event types so we never crash on forward compat.
    #[serde(other)]
    Unknown,
}

// ── System ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SystemInfo {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tools: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Assistant ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AssistantMessage {
    #[serde(default)]
    pub message: Option<MessageBody>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MessageBody {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub usage: Option<TokenUsage>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
    },
    #[serde(other)]
    Unknown,
}

// ── ToolUse ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolUseRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── ToolResult ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolResultEvent {
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Result (final turn summary) ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FinalResult {
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub usage: Option<UsageSummary>,
    #[serde(default)]
    pub num_turns: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UsageSummary {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── UserInput — what we write to Claude Code's stdin ──────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct UserInput {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: UserMessage,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserMessage {
    pub role: String,
    pub content: String,
}

impl UserInput {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            kind: "user".to_string(),
            message: UserMessage {
                role: "user".to_string(),
                content: text.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_system_event() {
        let json = r#"{"type":"system","session_id":"abc123","tools":[]}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::System(_)));
    }

    #[test]
    fn parse_assistant_text() {
        let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]}}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::Assistant(_)));
    }

    #[test]
    fn parse_tool_use() {
        let json = r#"{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/x"}}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::ToolUse(_)));
    }

    #[test]
    fn parse_tool_result() {
        let json = r#"{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::ToolResult(_)));
    }

    #[test]
    fn parse_result() {
        let json = r#"{"type":"result","subtype":"success","total_cost_usd":0.0042,"num_turns":1}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::Result(_)));
    }

    #[test]
    fn unknown_fields_dont_break_deserialisation() {
        let json = r#"{"type":"result","totally_new_field":true,"another_future_field":{"nested":42},"total_cost_usd":0.001}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::Result(_)));
    }

    #[test]
    fn unknown_event_type_becomes_unknown_variant() {
        let json = r#"{"type":"future_event_type","data":"something"}"#;
        let ev: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ev, ClaudeEvent::Unknown));
    }
}
