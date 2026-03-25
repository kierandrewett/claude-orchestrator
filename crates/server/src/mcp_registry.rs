use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{error, info};

/// A single user-configured MCP server entry (persisted to disk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryData {
    /// User-added custom servers.
    #[serde(default)]
    custom: Vec<McpServerEntry>,
    /// Names of disabled servers (including built-ins like "orchestrator").
    #[serde(default)]
    disabled: Vec<String>,
}

/// Persistent registry of user-configured MCP servers.
pub struct McpServerRegistry {
    path: PathBuf,
    data: Mutex<RegistryData>,
}

impl McpServerRegistry {
    pub fn load(state_dir: &Path) -> Self {
        let path = state_dir.join("mcp_servers.json");
        let data = if path.exists() {
            match std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
            {
                Some(v) => v,
                None => {
                    error!("mcp_registry: failed to load {}", path.display());
                    RegistryData::default()
                }
            }
        } else {
            RegistryData::default()
        };
        Self { path, data: Mutex::new(data) }
    }

    fn persist(path: &Path, data: &RegistryData) {
        match serde_json::to_string_pretty(data) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    error!("mcp_registry: failed to save: {e}");
                }
            }
            Err(e) => error!("mcp_registry: failed to serialize: {e}"),
        }
    }

    /// Returns a snapshot of all custom entries (including disabled ones).
    pub fn custom_servers(&self) -> Vec<McpServerEntry> {
        self.data.lock().unwrap().custom.clone()
    }

    /// Returns names of disabled servers (including built-ins like "orchestrator").
    pub fn disabled_names(&self) -> Vec<String> {
        self.data.lock().unwrap().disabled.clone()
    }

    /// Add a new custom server. Returns Err if name already exists.
    pub fn add(&self, entry: McpServerEntry) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        if data.custom.iter().any(|s| s.name == entry.name) {
            return Err(format!("MCP server '{}' already exists", entry.name));
        }
        info!("mcp_registry: adding server '{}'", entry.name);
        data.custom.push(entry);
        Self::persist(&self.path, &data);
        Ok(())
    }

    /// Remove a custom server by name. Returns Err if not found.
    pub fn remove(&self, name: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let before = data.custom.len();
        data.custom.retain(|s| s.name != name);
        if data.custom.len() == before {
            return Err(format!("MCP server '{}' not found", name));
        }
        // Also remove from disabled list
        data.disabled.retain(|n| n != name);
        info!("mcp_registry: removed server '{name}'");
        Self::persist(&self.path, &data);
        Ok(())
    }

    /// Disable a server by name (including built-ins like "orchestrator").
    pub fn disable(&self, name: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        // Mark disabled on the custom entry if present
        for s in data.custom.iter_mut() {
            if s.name == name {
                s.disabled = true;
            }
        }
        // Add to global disabled list
        if !data.disabled.iter().any(|n| n == name) {
            data.disabled.push(name.to_string());
        }
        info!("mcp_registry: disabled server '{name}'");
        Self::persist(&self.path, &data);
        Ok(())
    }

    /// Enable a previously disabled server by name.
    pub fn enable(&self, name: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let was_disabled = data.disabled.iter().any(|n| n == name);
        let is_custom = data.custom.iter().any(|s| s.name == name);
        let is_builtin = name == "orchestrator";
        if !was_disabled {
            if !is_custom && !is_builtin {
                return Err(format!("MCP server '{}' not found", name));
            }
            return Ok(()); // already enabled
        }
        // Clear disabled flag on the custom entry if present
        for s in data.custom.iter_mut() {
            if s.name == name {
                s.disabled = false;
            }
        }
        data.disabled.retain(|n| n != name);
        info!("mcp_registry: enabled server '{name}'");
        Self::persist(&self.path, &data);
        Ok(())
    }

    /// Return a structured snapshot of all MCP servers for UI rendering.
    pub fn entries(&self) -> Vec<claude_events::McpEntry> {
        let data = self.data.lock().unwrap();
        let disabled_set: std::collections::HashSet<&str> =
            data.disabled.iter().map(|s| s.as_str()).collect();
        let mut entries = vec![claude_events::McpEntry {
            name: "orchestrator".to_string(),
            is_builtin: true,
            enabled: !disabled_set.contains("orchestrator"),
            command: None,
            args: vec![],
        }];
        for s in &data.custom {
            entries.push(claude_events::McpEntry {
                name: s.name.clone(),
                is_builtin: false,
                enabled: !s.disabled && !disabled_set.contains(s.name.as_str()),
                command: Some(s.command.clone()),
                args: s.args.clone(),
            });
        }
        entries
    }

    /// Build a plain-text status display string (used by non-Telegram backends).
    #[allow(dead_code)]
    pub fn list_display(&self) -> String {
        let data = self.data.lock().unwrap();
        let disabled_set: std::collections::HashSet<&str> =
            data.disabled.iter().map(|s| s.as_str()).collect();

        let mut lines = Vec::new();

        // Built-in orchestrator server
        let orch_status = if disabled_set.contains("orchestrator") { "❌ disabled" } else { "✅ enabled" };
        lines.push(format!("orchestrator [built-in] — {orch_status}"));

        // Custom servers
        for s in &data.custom {
            let status = if s.disabled || disabled_set.contains(s.name.as_str()) {
                "❌ disabled"
            } else {
                "✅ enabled"
            };
            let args_str = if s.args.is_empty() {
                String::new()
            } else {
                format!(" {}", s.args.join(" "))
            };
            lines.push(format!("{} [`{}{}`] — {status}", s.name, s.command, args_str));
        }

        if data.custom.is_empty() {
            lines.push("No custom MCP servers configured. Use /mcp add to add one.".to_string());
        }

        lines.join("\n")
    }
}
