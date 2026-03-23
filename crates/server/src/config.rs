use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestratorConfig {
    pub server: ServerConfig,
    pub docker: DockerConfig,
    pub auth: AuthConfig,
    pub orchestrator_llm: OrchestratorLlmConfig,
    pub backends: BackendsConfig,
    pub display: DisplayConfig,
    #[serde(default)]
    pub mounts: HashMap<String, MountConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub state_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            state_dir: "~/.local/share/claude-orchestrator".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerConfig {
    pub socket: String,
    pub default_profile: String,
    pub image_prefix: String,
    pub idle_timeout_hours: u64,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            socket: "unix:///var/run/docker.sock".to_string(),
            default_profile: "base".to_string(),
            image_prefix: "orchestrator/claude-code".to_string(),
            idle_timeout_hours: 12,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub credentials_dir: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            credentials_dir: "~/.local/share/claude-orchestrator/auth".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestratorLlmConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BackendsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub web: Option<WebConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub supergroup_id: i64,
    pub scratchpad_topic_name: String,
    pub allowed_users: Vec<i64>,
    pub voice_stt: Option<String>,
    pub voice_stt_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub guild_id: u64,
    pub voice_stt: Option<String>,
    pub voice_tts: Option<String>,
    pub voice_tts_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    pub enabled: bool,
    pub bind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DisplayConfig {
    pub show_thinking: bool,
    pub stream_coalesce_ms: u64,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            show_thinking: false,
            stream_coalesce_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MountConfig {
    pub host: String,
    pub container: String,
    pub read_only: bool,
}

impl OrchestratorConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("parsing config from {}", path.display()))?;
        Ok(config)
    }

    pub fn expand_path(s: &str) -> PathBuf {
        if let Some(stripped) = s.strip_prefix("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(stripped)
        } else {
            PathBuf::from(s)
        }
    }

    /// Resolve an "env:VAR_NAME" string or return the string as-is.
    pub fn resolve_env(s: &str) -> String {
        if let Some(var) = s.strip_prefix("env:") {
            std::env::var(var).unwrap_or_else(|_| {
                tracing::warn!("env var {var} not set");
                String::new()
            })
        } else {
            s.to_string()
        }
    }
}
