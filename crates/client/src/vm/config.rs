//! Docker container configuration — loaded from `~/.config/claude-client/vm.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use claude_shared::{ToolsConfigProto, VmConfigProto, VolumeMountProto};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub name: String,
    pub host_path: String,
    pub guest_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Additional packages to install in the Docker image beyond the defaults.
    #[serde(default)]
    pub extra_packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Set to true to run sessions inside Docker.
    #[serde(default)]
    pub enabled: bool,

    /// Docker image tag to run (and build) for sessions.
    #[serde(default = "default_image")]
    pub image: String,

    /// Base image for the generated Dockerfile (FROM line).
    #[serde(default = "default_base_image")]
    pub base_image: String,

    /// Give the container internet access (--network bridge vs --network none).
    #[serde(default = "default_true")]
    pub network_enabled: bool,

    /// Directory for persistent data (image build cache, etc.).
    pub data_dir: String,

    #[serde(default)]
    pub mounts: Vec<VolumeMount>,

    #[serde(default)]
    pub tools: ToolsConfig,
}

fn default_image() -> String {
    "claude-code:latest".to_string()
}

fn default_base_image() -> String {
    "alpine:latest".to_string()
}

fn default_true() -> bool {
    true
}

impl VmConfig {
    /// `~/.config/claude-client/vm.toml`
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-client")
            .join("vm.toml")
    }

    fn default_data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-client")
            .join("vm")
    }

    /// Path to the generated Dockerfile on disk.
    pub fn dockerfile_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-client")
            .join("Dockerfile")
    }

    /// Build a config with sensible defaults.
    pub fn detect_defaults() -> Self {
        let data_dir = Self::default_data_dir();
        Self {
            enabled: false,
            image: default_image(),
            base_image: default_base_image(),
            network_enabled: true,
            data_dir: data_dir.to_string_lossy().into_owned(),
            mounts: Vec::new(),
            tools: ToolsConfig::default(),
        }
    }

    /// Load from disk. Returns `None` if the file doesn't exist.
    pub fn load() -> anyhow::Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(Some(toml::from_str(&text)?))
    }

    /// Persist to disk, creating parent directories as needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Proto conversions
// ---------------------------------------------------------------------------

impl From<VmConfig> for VmConfigProto {
    fn from(c: VmConfig) -> Self {
        VmConfigProto {
            enabled: c.enabled,
            network_enabled: c.network_enabled,
            image: c.image,
            base_image: c.base_image,
            data_dir: c.data_dir,
            mounts: c
                .mounts
                .into_iter()
                .map(|m| VolumeMountProto {
                    name: m.name,
                    host_path: m.host_path,
                    guest_path: m.guest_path,
                    size_gb: 0,
                    excludes: vec![],
                })
                .collect(),
            tools: ToolsConfigProto {
                extra_packages: c.tools.extra_packages,
            },
        }
    }
}

impl From<VmConfigProto> for VmConfig {
    fn from(p: VmConfigProto) -> Self {
        VmConfig {
            enabled: p.enabled,
            network_enabled: p.network_enabled,
            image: p.image,
            base_image: p.base_image,
            data_dir: p.data_dir,
            mounts: p
                .mounts
                .into_iter()
                .map(|m| VolumeMount {
                    name: m.name,
                    host_path: m.host_path,
                    guest_path: m.guest_path,
                })
                .collect(),
            tools: ToolsConfig {
                extra_packages: p.tools.extra_packages,
            },
        }
    }
}
