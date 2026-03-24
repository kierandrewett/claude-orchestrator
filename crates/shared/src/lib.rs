use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A custom MCP server definition passed from the server to a client session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDef {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ── Session types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: HashMap<String, u32>,
    pub turns: u32,
    pub cost_usd: Option<f64>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub cwd: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub stats: SessionStats,
    pub client_hostname: Option<String>,
    pub claude_session_id: Option<String>, // Claude's own session UUID
}

// ── Wire protocol ──────────────────────────────────────────────────────────────

/// Client daemon → Server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum C2S {
    Hello {
        client_id: String,
        hostname: String,
    },
    SessionStarted {
        session_id: String,
        pid: u32,
        cwd: String,
    },
    SessionEvent {
        session_id: String,
        event: serde_json::Value,
    },
    SessionEnded {
        session_id: String,
        exit_code: i32,
        stats: SessionStats,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Signal from the client that Claude is idle (no turn in progress) and
    /// the queue is empty.  The server uses this to reset the claude_idle flag.
    ClaudeIdle {
        session_id: String,
    },
}

/// A file attached to a message, transferred from the Telegram bot to the client
/// daemon as base64-encoded content so it can be fed to Claude.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedFile {
    /// Original filename (e.g. "photo.jpg", "report.pdf").
    pub filename: String,
    /// MIME type (e.g. "image/jpeg", "application/pdf", "text/plain").
    pub mime_type: String,
    /// Base64-encoded file content (standard alphabet, padded).
    pub data_base64: String,
}

/// Server → Client daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum S2C {
    StartSession {
        session_id: String,
        initial_prompt: Option<String>,
        extra_args: Vec<String>,
        claude_session_id: String, // pre-generated UUID for --session-id or --resume
        is_resume: bool,           // true = use --resume, false = use --session-id
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        initial_files: Vec<AttachedFile>,
        /// Additional MCP servers to inject into the session's MCP config.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mcp_servers: Vec<McpServerDef>,
        /// Names of MCP servers to disable (including built-ins like "orchestrator").
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        disabled_mcp_servers: Vec<String>,
        /// MCP tool names to suppress: hidden from tools/list and rejected at tools/call.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        suppress_mcp_tools: Vec<String>,
        /// Extra env vars injected into the MCP helper subprocess (e.g. allowed emoji list).
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        mcp_extra_env: HashMap<String, String>,
    },
    SendInput {
        session_id: String,
        text: String,
        /// Opaque backend message ID, forwarded so the client can match
        /// a CancelQueuedInput against items waiting in its queue.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_ref_opaque_id: Option<String>,
    },
    /// Like SendInput but carries attached files (images, PDFs, other) to be
    /// forwarded to Claude as multimodal content blocks.
    SendInputWithFiles {
        session_id: String,
        /// The message text / caption (may be empty if attachment-only).
        text: String,
        files: Vec<AttachedFile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_ref_opaque_id: Option<String>,
    },
    KillSession {
        session_id: String,
    },
    /// Interrupt the current Claude response (SIGINT) without ending the
    /// session.  Claude stops generating and waits for the next input.
    InterruptSession {
        session_id: String,
    },
    /// Cancel a specific queued message before it is delivered to Claude.
    /// Matches by the opaque_id of the MessageRef that was returned in
    /// OrchestratorEvent::MessageQueued.
    CancelQueuedInput {
        session_id: String,
        message_ref_opaque_id: String,
    },
}

/// Dashboard → Server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum D2S {
    CreateSession {
        name: Option<String>,
        initial_prompt: Option<String>,
    },
    SendInput {
        session_id: String,
        text: String,
    },
    KillSession {
        session_id: String,
    },
    GetHistory {
        session_id: String,
    },
}

/// Server → Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum S2D {
    SessionList {
        sessions: Vec<SessionInfo>,
    },
    SessionCreated {
        session: SessionInfo,
    },
    SessionUpdated {
        session: SessionInfo,
    },
    SessionEvent {
        session_id: String,
        event: serde_json::Value,
    },
    SessionEnded {
        session_id: String,
        stats: SessionStats,
        exit_code: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SessionHistory {
        session_id: String,
        events: Vec<serde_json::Value>,
    },
    ClientStatus {
        connected: bool,
        hostname: Option<String>,
    },
    Error {
        message: String,
    },
}
