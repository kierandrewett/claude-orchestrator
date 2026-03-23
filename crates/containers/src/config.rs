use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── ContainerConfig ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Docker image to use (e.g. "orchestrator/claude-code:rust").
    pub image: String,
    /// Volume mounts for the container.
    pub mounts: Vec<MountPoint>,
    /// Additional environment variables to inject.
    pub env: Vec<(String, String)>,
    /// Working directory inside the container.
    pub workdir: String,
    /// Network mode.
    pub network: NetworkMode,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            image: "orchestrator/claude-code:base".to_string(),
            mounts: Vec::new(),
            env: Vec::new(),
            workdir: "/workspace".to_string(),
            network: NetworkMode::Bridge,
        }
    }
}

// ── MountPoint ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountPoint {
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub read_only: bool,
}

// ── NetworkMode ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NetworkMode {
    Bridge,
    None,
    Host,
}

impl NetworkMode {
    pub fn as_str(&self) -> &str {
        match self {
            NetworkMode::Bridge => "bridge",
            NetworkMode::None => "none",
            NetworkMode::Host => "host",
        }
    }
}

// ── SessionData ──────────────────────────────────────────────────────────────

/// Persisted data about a container session, used to recreate or resume it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// The container ID (may be stale if the container was removed).
    pub container_id: String,
    /// Claude Code's own session UUID, used with `--resume`.
    pub claude_session_id: String,
    /// The config used when the container was spawned.
    pub config: ContainerConfig,
    /// When the container was last seen running.
    pub last_active: chrono::DateTime<chrono::Utc>,
}

impl SessionData {
    pub fn new(container_id: String, claude_session_id: String, config: ContainerConfig) -> Self {
        Self {
            container_id,
            claude_session_id,
            config,
            last_active: chrono::Utc::now(),
        }
    }
}

/// Generate a new random Claude session ID.
pub fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}
