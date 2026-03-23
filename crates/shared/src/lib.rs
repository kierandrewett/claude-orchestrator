use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

// ── Slash command discovery ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

// ── Wire protocol ──────────────────────────────────────────────────────────────

/// One historical Claude Code conversation, imported from ~/.claude/projects/.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalSession {
    pub claude_session_id: String,
    pub cwd: String,
    pub events: Vec<serde_json::Value>, // user + assistant lines only
    pub created_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

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
    CommandList {
        commands: Vec<SlashCommand>,
    },
    ImportHistory {
        sessions: Vec<HistoricalSession>,
    },
    /// Response to GetVmConfig — carries the current VM config.
    VmConfig {
        request_id: String,
        config: VmConfigProto,
    },
    /// Ack for SetVmConfig (or error response for GetVmConfig when no config exists).
    VmConfigAck {
        request_id: String,
        success: bool,
        error: Option<String>,
    },
    /// Streaming log line from an in-progress BuildImage.
    BuildImageLog {
        request_id: String,
        line: String,
    },
    /// Final result of a BuildImage request.
    BuildImageResult {
        request_id: String,
        success: bool,
        error: Option<String>,
    },
}

// ── VM config wire types ───────────────────────────────────────────────────────

/// A single directory mount for the microVM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMountProto {
    pub name: String,
    pub host_path: String,
    pub guest_path: String,
    #[serde(default = "default_size_gb")]
    pub size_gb: u32,
    #[serde(default)]
    pub excludes: Vec<String>,
}

fn default_size_gb() -> u32 {
    20
}

/// Which Alpine packages to install in the rootfs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfigProto {
    #[serde(default)]
    pub extra_packages: Vec<String>,
}

/// Wire-safe representation of the client's container config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfigProto {
    pub enabled: bool,
    pub network_enabled: bool,
    /// Docker image tag to use (and build) for sessions.
    #[serde(default = "default_image")]
    pub image: String,
    /// Base image for the generated Dockerfile (FROM line).
    #[serde(default = "default_base_image")]
    pub base_image: String,
    pub data_dir: String,
    pub mounts: Vec<VolumeMountProto>,
    pub tools: ToolsConfigProto,
}

fn default_image() -> String {
    "claude-code:latest".to_string()
}

fn default_base_image() -> String {
    "alpine:latest".to_string()
}

impl Default for VmConfigProto {
    fn default() -> Self {
        Self {
            enabled: false,
            network_enabled: true,
            image: default_image(),
            base_image: default_base_image(),
            data_dir: String::new(),
            mounts: Vec::new(),
            tools: ToolsConfigProto::default(),
        }
    }
}

/// Response type used by AppState for pending VM config requests.
#[derive(Debug)]
pub enum VmConfigResponse {
    Config(VmConfigProto),
    Ack { success: bool, error: Option<String> },
    BuildResult { success: bool, error: Option<String> },
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
    },
    SendInput {
        session_id: String,
        text: String,
    },
    /// Like SendInput but carries attached files (images, PDFs, other) to be
    /// forwarded to Claude as multimodal content blocks.
    SendInputWithFiles {
        session_id: String,
        /// The message text / caption (may be empty if attachment-only).
        text: String,
        files: Vec<AttachedFile>,
    },
    KillSession {
        session_id: String,
    },
    /// Interrupt the current Claude response (SIGINT) without ending the
    /// session.  Claude stops generating and waits for the next input.
    InterruptSession {
        session_id: String,
    },
    QueryCommands, // server requests client to fetch slash commands
    /// Request client to return its current VM config.
    GetVmConfig {
        request_id: String,
    },
    /// Push an updated VM config to the client to save.
    SetVmConfig {
        request_id: String,
        config: VmConfigProto,
    },
    /// Ask the client to (re)build the Docker image from its Dockerfile.
    BuildImage {
        request_id: String,
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
    CommandList {
        commands: Vec<SlashCommand>,
    },
    Error {
        message: String,
    },
}
