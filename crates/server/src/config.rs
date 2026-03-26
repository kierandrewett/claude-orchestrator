use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestratorConfig {
    pub server: ServerConfig,
    pub docker: DockerConfig,
pub backends: BackendsConfig,
    pub display: DisplayConfig,
    #[serde(default)]
    pub mounts: HashMap<String, MountConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub state_dir: String,
    /// Address to bind the client-daemon WebSocket server on.
    #[serde(default = "default_client_bind")]
    pub client_bind: String,
    /// Optional bearer token required for client-daemon connections.
    /// If None, no authentication is performed.
    #[serde(default)]
    pub client_token: Option<String>,
    /// Optional system prompt passed to every new Claude Code session.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

fn default_client_bind() -> String {
    "0.0.0.0:8765".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            state_dir: "~/.local/share/claude-orchestrator".to_string(),
            client_bind: default_client_bind(),
            client_token: None,
            system_prompt: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerConfig {
    #[serde(default = "DockerConfig::default_socket")]
    pub socket: String,
    #[serde(default = "DockerConfig::default_profile")]
    pub default_profile: String,
    #[serde(default = "DockerConfig::default_image_prefix")]
    pub image_prefix: String,
    #[serde(default = "DockerConfig::default_idle_timeout_hours")]
    pub idle_timeout_hours: u64,
}

impl DockerConfig {
    fn default_socket() -> String { "unix:///var/run/docker.sock".to_string() }
    fn default_profile() -> String { "base".to_string() }
    fn default_image_prefix() -> String { "orchestrator/claude-code".to_string() }
    fn default_idle_timeout_hours() -> u64 { 12 }
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            socket: Self::default_socket(),
            default_profile: Self::default_profile(),
            image_prefix: Self::default_image_prefix(),
            idle_timeout_hours: Self::default_idle_timeout_hours(),
        }
    }
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
    /// Tool calls still executed by Claude but not shown to the user in this backend.
    #[serde(default)]
    pub hidden_tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub guild_id: u64,
    pub voice_stt: Option<String>,
    pub voice_tts: Option<String>,
    pub voice_tts_api_key: Option<String>,
    /// Tool calls still executed by Claude but not shown to the user in this backend.
    #[serde(default)]
    pub hidden_tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    pub enabled: bool,
    pub bind: Option<String>,
    /// Port/address for the Node.js dashboard server (default: 0.0.0.0:3001).
    #[serde(default)]
    pub dashboard_bind: Option<String>,
    /// Bearer token required to access the dashboard UI. None = no auth required.
    #[serde(default)]
    pub dashboard_token: Option<String>,
    /// External URL of the dashboard (used in Telegram "Open Dashboard" button etc).
    #[serde(default)]
    pub dashboard_url: Option<String>,
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

/// Replace every TOML string value of the form `"env:VAR_NAME"` with the
/// value of the named environment variable.  Works on the raw TOML text before
/// parsing so it applies to every field automatically.
///
/// The substitution is conservative: it only matches values that appear as
/// `= "env:…"` (possibly with surrounding whitespace) so it won't touch keys
/// or comments.
/// Recursively walk a `toml::Value`, expanding any string of the form `"env:VAR"`
/// by reading the named environment variable.  Tables that contain
/// `enabled = false` are skipped entirely — their env refs are never evaluated.
fn expand_env_in_value(value: &mut toml::Value) -> anyhow::Result<()> {
    match value {
        toml::Value::Table(table) => {
            // Skip disabled sections — don't require their env vars to be set.
            if matches!(table.get("enabled"), Some(toml::Value::Boolean(false))) {
                return Ok(());
            }
            for v in table.iter_mut().map(|(_, v)| v) {
                expand_env_in_value(v)?;
            }
        }
        toml::Value::Array(arr) => {
            for v in arr.iter_mut() {
                expand_env_in_value(v)?;
            }
        }
        toml::Value::String(s) => {
            if let Some(var) = s.strip_prefix("env:") {
                match std::env::var(var) {
                    Ok(val) if val.is_empty() => anyhow::bail!(
                        "config references ${{{var}}} but it is set to an empty string"
                    ),
                    Ok(val) => *s = val,
                    Err(_) => anyhow::bail!(
                        "config references ${{{var}}} which is not set"
                    ),
                }
            }
        }
        _ => {}
    }
    Ok(())
}

impl OrchestratorConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        let mut value: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("parsing config from {}", path.display()))?;
        expand_env_in_value(&mut value)?;
        value.try_into()
            .with_context(|| format!("deserialising config from {}", path.display()))
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
}
