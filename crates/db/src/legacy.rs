use serde::{Deserialize, Serialize};

/// Minimal shape of the old state.json
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct LegacyState {
    #[serde(default)]
    pub tasks: Vec<LegacyTask>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyTask {
    pub task_id: String,
    pub task_name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_status: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_activity: Option<String>,
}

/// Minimal shape of the old telegram_state.json (kept for reference)
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct LegacyTelegramState {
    #[serde(default)]
    pub mappings: Vec<LegacyTelegramMapping>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LegacyTelegramMapping {
    pub task_id: String,
    pub topic_id: i64,
    pub chat_id: i64,
    #[serde(default)]
    pub message_count: i64,
    #[serde(default)]
    pub last_message_id: Option<i64>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}
