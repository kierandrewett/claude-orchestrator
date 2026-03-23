use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use claude_containers::SessionData;
use claude_events::{TaskId, TaskKind};
use claude_ndjson::UsageStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersistedTaskState {
    Running,
    Hibernated,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTask {
    pub id: TaskId,
    pub name: String,
    pub profile: String,
    pub container_id: Option<String>,
    pub session_data: SessionData,
    pub usage: UsageStats,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub state: PersistedTaskState,
    pub kind: TaskKind,
    pub backend_channels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedState {
    pub tasks: Vec<PersistedTask>,
}

pub struct StateStore {
    path: PathBuf,
}

impl StateStore {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("state.json"),
        }
    }

    pub fn load(&self) -> Result<PersistedState> {
        if !self.path.exists() {
            return Ok(PersistedState::default());
        }

        let json = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading state from {}", self.path.display()))?;

        match serde_json::from_str::<PersistedState>(&json) {
            Ok(state) => {
                info!("persistence: loaded {} tasks", state.tasks.len());
                Ok(state)
            }
            Err(e) => {
                // Back up the corrupt file and start fresh.
                let backup = self.path.with_extension("json.corrupt");
                warn!(
                    "persistence: corrupt state file ({e}), backing up to {}",
                    backup.display()
                );
                if let Err(copy_err) = std::fs::copy(&self.path, &backup) {
                    error!("persistence: failed to backup corrupt state: {copy_err}");
                }
                Ok(PersistedState::default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::new(dir.path());
        let state = store.load().unwrap();
        assert!(state.tasks.is_empty());
    }

    #[test]
    fn load_valid_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"tasks":[]}"#;
        std::fs::write(dir.path().join("state.json"), json).unwrap();
        let store = StateStore::new(dir.path());
        let state = store.load().unwrap();
        assert!(state.tasks.is_empty());
    }

    #[test]
    fn load_corrupt_file_returns_empty_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"not valid json{{{").unwrap();
        let store = StateStore::new(dir.path());
        let state = store.load().unwrap();
        assert!(state.tasks.is_empty());
        // Backup file should exist
        assert!(dir.path().join("state.json.corrupt").exists());
    }
}
