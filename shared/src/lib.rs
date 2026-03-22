use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

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
    pub input_tokens:  u64,
    pub output_tokens: u64,
    pub tool_calls:    HashMap<String, u32>,
    pub turns:         u32,
    pub cost_usd:      Option<f64>,
    pub stop_reason:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id:               String,
    pub name:             Option<String>,
    pub cwd:              String,
    pub status:           SessionStatus,
    pub created_at:       DateTime<Utc>,
    pub started_at:       Option<DateTime<Utc>>,
    pub ended_at:         Option<DateTime<Utc>>,
    pub stats:            SessionStats,
    pub client_hostname:  Option<String>,
    pub claude_session_id: Option<String>,   // Claude's own session UUID
}

// ── Slash command discovery ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name:        String,
    pub description: String,
}

// ── Wire protocol ──────────────────────────────────────────────────────────────

/// Client daemon → Server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum C2S {
    Hello          { client_id: String, hostname: String },
    SessionStarted { session_id: String, pid: u32, cwd: String },
    SessionEvent   { session_id: String, event: serde_json::Value },
    SessionEnded   { session_id: String, exit_code: i32, stats: SessionStats },
    CommandList    { commands: Vec<SlashCommand> },
}

/// Server → Client daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum S2C {
    StartSession {
        session_id:        String,
        initial_prompt:    Option<String>,
        extra_args:        Vec<String>,
        claude_session_id: String,   // pre-generated UUID for --session-id or --resume
        is_resume:         bool,     // true = use --resume, false = use --session-id
    },
    SendInput    { session_id: String, text: String },
    KillSession  { session_id: String },
    QueryCommands,                   // server requests client to fetch slash commands
}

/// Dashboard → Server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum D2S {
    CreateSession { name: Option<String>, initial_prompt: Option<String> },
    SendInput     { session_id: String, text: String },
    KillSession   { session_id: String },
    GetHistory    { session_id: String },
}

/// Server → Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum S2D {
    SessionList    { sessions: Vec<SessionInfo> },
    SessionCreated { session: SessionInfo },
    SessionUpdated { session: SessionInfo },
    SessionEvent   { session_id: String, event: serde_json::Value },
    SessionEnded   { session_id: String, stats: SessionStats, exit_code: i32 },
    SessionHistory { session_id: String, events: Vec<serde_json::Value> },
    ClientStatus   { connected: bool, hostname: Option<String> },
    CommandList    { commands: Vec<SlashCommand> },
    Error          { message: String },
}
