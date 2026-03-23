use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
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
    last_save: Arc<Mutex<Option<Instant>>>,
}

impl StateStore {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("state.json"),
            last_save: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn save(&self, state: &PersistedState) -> Result<()> {
        // Debounce: at most once per second.
        {
            let mut last = self.last_save.lock().await;
            if let Some(t) = *last {
                if t.elapsed() < Duration::from_secs(1) {
                    return Ok(());
                }
            }
            *last = Some(Instant::now());
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating state dir {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(state).context("serialising state")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .with_context(|| format!("writing state to {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("renaming state file to {}", self.path.display()))?;

        info!("persistence: saved {} tasks", state.tasks.len());
        Ok(())
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
