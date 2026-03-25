use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::commands::ParsedCommand;
use crate::types::{BackendSource, MessageRef, TaskId};

/// Events emitted by backends into the orchestrator's mpsc channel.
///
/// Must be `Clone` so it can be forwarded between tasks.
/// Byte data uses `Arc<Vec<u8>>` to make cloning cheap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackendEvent {
    /// A plain text message from a user.
    UserMessage {
        task_id: TaskId,
        text: String,
        message_ref: MessageRef,
        source: BackendSource,
    },

    /// A slash command from a user.
    Command {
        command: ParsedCommand,
        /// The task the command pertains to, if determinable from context.
        task_id: Option<TaskId>,
        message_ref: MessageRef,
        source: BackendSource,
    },

    /// Interrupt the current Claude response for a task (SIGINT — session
    /// stays alive, Claude returns to the input prompt).
    InterruptTask {
        task_id: TaskId,
        source: BackendSource,
    },

    /// Cancel a specific queued message before it is delivered to Claude.
    CancelQueuedMessage {
        task_id: TaskId,
        /// The MessageRef that was returned in OrchestratorEvent::MessageQueued.
        message_ref: MessageRef,
        source: BackendSource,
    },

    /// Request the orchestrator to re-emit the current state of all tasks.
    /// Sent by backends on startup so they can sync (e.g. rename hibernated topics).
    SyncRequest,

    /// Backend-specific capability hints for the MCP helper.
    /// The orchestrator merges these into every new session's helper environment.
    BackendCapabilities {
        backend_name: String,
        /// Key-value pairs injected into the helper subprocess env.
        /// Example: `{"ORCHESTRATOR_ALLOWED_EMOJIS": "🎯,📌,🔥,..."}`
        mcp_env: std::collections::HashMap<String, String>,
    },

    /// A file upload from a user (image, PDF, attachment, …).
    FileUpload {
        task_id: TaskId,
        filename: String,
        #[serde(skip)]
        data: Arc<Vec<u8>>,
        mime_type: Option<String>,
        caption: Option<String>,
        message_ref: MessageRef,
        source: BackendSource,
    },
}

// Implement Serialize/Deserialize manually for ParsedCommand so it crosses
// the channel boundary. We skip the #[derive] on ParsedCommand itself since
// it contains TaskId which is already Serde-able.
//
// For now the serde derives on BackendEvent skip ParsedCommand by using
// #[serde(skip)] — we add a manual Serialize/Deserialize here.

impl Serialize for ParsedCommand {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Represent as a debug string for logging; not meant to round-trip.
        s.serialize_str(&format!("{:?}", self))
    }
}

impl<'de> Deserialize<'de> for ParsedCommand {
    fn deserialize<D: serde::Deserializer<'de>>(_d: D) -> Result<Self, D::Error> {
        // We never deserialise ParsedCommand from the wire.
        Err(serde::de::Error::custom("ParsedCommand cannot be deserialised"))
    }
}
