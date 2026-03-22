//! VM configuration — loaded from `~/.config/claude-client/vm.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use claude_shared::{ToolsConfigProto, VmConfigProto, VolumeMountProto};

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Short identifier used in logs and Telegram commands.
    pub name: String,
    /// Absolute path on the host to sync into the VM.
    pub host_path: String,
    /// Absolute path inside the VM where the volume is mounted.
    pub guest_path: String,
    /// Size of the ext4 image in GiB (default 20).
    #[serde(default = "default_size_gb")]
    pub size_gb: u32,
    /// rsync --exclude patterns (e.g. "node_modules", "target").
    #[serde(default)]
    pub excludes: Vec<String>,
}

fn default_size_gb() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Additional Alpine packages to install in the rootfs beyond the defaults.
    #[serde(default)]
    pub extra_packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Set to true to route all sessions through Firecracker.
    #[serde(default)]
    pub enabled: bool,

    /// Set to true to give the VM internet access via NAT (requires root/CAP_NET_ADMIN).
    #[serde(default)]
    pub network_enabled: bool,

    /// Path to the `firecracker` binary.
    pub firecracker_path: String,

    /// Path to the kernel ELF image (`vmlinux`, not `bzImage`).
    pub kernel_path: String,

    /// Path to the Alpine-based rootfs ext4 image.
    pub rootfs_path: String,

    /// Directory for persistent VM data (volume images, sockets, etc.).
    pub data_dir: String,

    #[serde(default = "default_vcpus")]
    pub vcpus: u32,

    #[serde(default = "default_memory_mb")]
    pub memory_mb: u32,

    #[serde(default)]
    pub mounts: Vec<VolumeMount>,

    #[serde(default)]
    pub tools: ToolsConfig,
}

fn default_vcpus() -> u32 {
    2
}
fn default_memory_mb() -> u32 {
    2048
}

impl VmConfig {
    /// `~/.config/claude-client/vm.toml`
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-client")
            .join("vm.toml")
    }

    /// Default XDG data directory for VM artefacts.
    fn default_data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-client")
            .join("vm")
    }

    /// Build a config with auto-detected values. Used when no `vm.toml` exists
    /// yet so the user can see sensible defaults before saving.
    pub fn detect_defaults() -> Self {
        let data_dir = Self::default_data_dir();
        let firecracker_path = which::which("firecracker")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/usr/bin/firecracker".to_string());
        Self {
            enabled: false,
            network_enabled: false,
            firecracker_path,
            kernel_path: data_dir.join("vmlinux").to_string_lossy().into_owned(),
            rootfs_path: data_dir.join("rootfs.ext4").to_string_lossy().into_owned(),
            data_dir: data_dir.to_string_lossy().into_owned(),
            vcpus: 2,
            memory_mb: 2048,
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

    /// Directory where volume images live: `<data_dir>/volumes/`.
    pub fn volumes_dir(&self) -> PathBuf {
        PathBuf::from(&self.data_dir).join("volumes")
    }

    /// Path for the ext4 image of a named volume.
    pub fn volume_image_path(&self, name: &str) -> PathBuf {
        self.volumes_dir().join(format!("{name}.ext4"))
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
            firecracker_path: c.firecracker_path,
            kernel_path: c.kernel_path,
            rootfs_path: c.rootfs_path,
            data_dir: c.data_dir,
            vcpus: c.vcpus,
            memory_mb: c.memory_mb,
            mounts: c
                .mounts
                .into_iter()
                .map(|m| VolumeMountProto {
                    name: m.name,
                    host_path: m.host_path,
                    guest_path: m.guest_path,
                    size_gb: m.size_gb,
                    excludes: m.excludes,
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
            firecracker_path: p.firecracker_path,
            kernel_path: p.kernel_path,
            rootfs_path: p.rootfs_path,
            data_dir: p.data_dir,
            vcpus: p.vcpus,
            memory_mb: p.memory_mb,
            mounts: p
                .mounts
                .into_iter()
                .map(|m| VolumeMount {
                    name: m.name,
                    host_path: m.host_path,
                    guest_path: m.guest_path,
                    size_gb: m.size_gb,
                    excludes: m.excludes,
                })
                .collect(),
            tools: ToolsConfig {
                extra_packages: p.tools.extra_packages,
            },
        }
    }
}
