use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use anyhow::Result;

use crate::protocol::{SessionInfo};

pub struct Store {
    data_dir: PathBuf,
}

impl Store {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into() }
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.data_dir.join("sessions").join(session_id)
    }

    fn meta_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("meta.json")
    }

    fn events_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("events.jsonl")
    }

    /// Save or update session metadata atomically (write temp, rename).
    pub async fn save_session(&self, info: &SessionInfo) -> Result<()> {
        let dir = self.session_dir(&info.id);
        fs::create_dir_all(&dir).await?;
        let tmp = dir.join("meta.json.tmp");
        let json = serde_json::to_vec_pretty(info)?;
        fs::write(&tmp, &json).await?;
        fs::rename(tmp, self.meta_path(&info.id)).await?;
        Ok(())
    }

    /// Append a single NDJSON event to the session's event log.
    pub async fn append_event(&self, session_id: &str, event: &serde_json::Value) -> Result<()> {
        let dir = self.session_dir(session_id);
        fs::create_dir_all(&dir).await?;
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path(session_id))
            .await?;
        file.write_all(&line).await?;
        Ok(())
    }

    /// Load all sessions from disk. Returns (SessionInfo, events) pairs.
    pub async fn load_all(&self) -> Vec<(SessionInfo, Vec<serde_json::Value>)> {
        let sessions_dir = self.data_dir.join("sessions");
        let mut results = Vec::new();

        let mut entries = match fs::read_dir(&sessions_dir).await {
            Ok(e) => e,
            Err(_) => return results,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(info) = Self::load_meta(entry.path().join("meta.json")).await {
                let events = Self::load_events(entry.path().join("events.jsonl")).await;
                results.push((info, events));
            }
        }

        results
    }

    async fn load_meta(path: PathBuf) -> Option<SessionInfo> {
        let bytes = fs::read(&path).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    async fn load_events(path: PathBuf) -> Vec<serde_json::Value> {
        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        content
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}
