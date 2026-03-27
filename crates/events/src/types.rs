use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── TaskId ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── MessageRef — opaque backend message reference ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageRef {
    pub backend: String,
    pub opaque_id: String,
}

impl MessageRef {
    pub fn new(backend: impl Into<String>, opaque_id: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            opaque_id: opaque_id.into(),
        }
    }
}

// ── BackendSource — which backend + user sent a message ────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BackendSource {
    pub backend_name: String,
    pub user_id: String,
}

impl BackendSource {
    pub fn new(backend_name: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            backend_name: backend_name.into(),
            user_id: user_id.into(),
        }
    }
}

// ── TaskKind ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    /// The always-on Scratchpad (never auto-hibernated).
    Scratchpad,
    /// A regular job task.
    Job,
}

// ── TaskStateSummary — serialisable snapshot of a task's state ─────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStateSummary {
    Running,
    Hibernated,
    Dead,
}

// ── McpEntry — snapshot of a single MCP server ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEntry {
    pub name: String,
    pub is_builtin: bool,
    pub enabled: bool,
    /// Set for URL-based (HTTP/SSE) servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// None for built-in or URL-based servers.
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// true = URL server with no valid OAuth token stored; user must authorise.
    #[serde(default)]
    pub needs_oauth: bool,
}

// ── EventListEntry — lightweight view of a scheduled event for UI rendering ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventListEntry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// `"once"` or `"recurring"`
    pub mode: String,
    pub schedule: String,
    /// Pre-formatted local-time string, e.g. `"25 Mar 09:00"` or `"tomorrow 09:00"`.
    pub next_run: Option<String>,
    pub origin_task_name: String,
}

// ── TaskSummary — lightweight info about a task for LLM context ───────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: TaskId,
    pub name: String,
    pub profile: String,
    pub state: TaskStateSummary,
    pub kind: TaskKind,
}

// ── SessionPhase ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionPhase {
    Acknowledged,
    Starting,
    ToolUse,
    Thinking,
    Responding,
    Complete,
    Error,
}

impl SessionPhase {
    pub fn emoji(&self) -> &'static str {
        match self {
            SessionPhase::Acknowledged => "👀",
            SessionPhase::Starting => "⚡",
            SessionPhase::ToolUse => "👨‍💻",
            SessionPhase::Thinking => "🤔",
            SessionPhase::Responding => "✍️",
            SessionPhase::Complete => "👍",
            SessionPhase::Error => "🤬",
        }
    }
}
