use chrono::{DateTime, Utc};

use claude_ndjson::NdjsonTransport;

use crate::config::SessionData;

/// A live, running container with its stdio transport.
pub struct ContainerHandle {
    pub container_id: String,
    pub session_data: SessionData,
    pub transport: NdjsonTransport,
    pub started_at: DateTime<Utc>,
}

impl ContainerHandle {
    pub fn new(container_id: String, session_data: SessionData, transport: NdjsonTransport) -> Self {
        Self {
            container_id,
            session_data,
            transport,
            started_at: Utc::now(),
        }
    }

    pub fn claude_session_id(&self) -> &str {
        &self.session_data.claude_session_id
    }
}
