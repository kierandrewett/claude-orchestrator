use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::types::{MessageRef, SessionPhase, TaskId, TaskKind, TaskStateSummary};
use claude_ndjson::UsageStats;

/// Events emitted by the orchestrator core, broadcast to all backends.
///
/// Must be `Clone` so it can go through a `tokio::sync::broadcast` channel.
/// Byte data uses `Arc<Vec<u8>>` to make cloning cheap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestratorEvent {
    /// The session phase of a task changed (triggers a reaction on the
    /// originating backend message).
    PhaseChanged {
        task_id: TaskId,
        phase: SessionPhase,
        trigger_message: Option<MessageRef>,
    },

    /// A chunk of assistant text is ready.
    TextOutput {
        task_id: TaskId,
        text: String,
        /// `false` = start of a new message, `true` = edit the previous message.
        is_continuation: bool,
        /// The user message that triggered this response (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// Claude started calling a tool.
    ToolStarted {
        task_id: TaskId,
        tool_name: String,
        summary: String,
        /// The user message that triggered this turn (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// A tool call completed.
    ToolCompleted {
        task_id: TaskId,
        tool_name: String,
        summary: String,
        is_error: bool,
        output_preview: Option<String>,
        /// The user message that triggered this turn (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// Thinking/internal-monologue text (only emitted when show_thinking=true).
    Thinking {
        task_id: TaskId,
        text: String,
        /// The user message that triggered this turn (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// A turn finished — includes usage stats and wall-clock duration.
    TurnComplete {
        task_id: TaskId,
        usage: UsageStats,
        duration_secs: f64,
        /// The user message that triggered this turn (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// A new task was created.
    TaskCreated {
        task_id: TaskId,
        name: String,
        profile: String,
        kind: TaskKind,
    },

    /// A task's state changed (Running → Hibernated, etc.).
    TaskStateChanged {
        task_id: TaskId,
        old_state: TaskStateSummary,
        new_state: TaskStateSummary,
    },

    /// An error occurred, with optional actionable next steps.
    Error {
        task_id: Option<TaskId>,
        error: String,
        next_steps: Vec<String>,
        /// The message that triggered the error, if any (for reaction/reply).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },

    /// A user message was queued because Claude is currently processing another turn.
    MessageQueued {
        task_id: TaskId,
        message_ref: MessageRef,
    },

    /// A message that was queued mid-turn has now been delivered to Claude.
    QueuedMessageDelivered {
        task_id: TaskId,
        original_ref: MessageRef,
    },

    /// Claude produced a file output (e.g. generated image or exported data).
    FileOutput {
        task_id: TaskId,
        filename: String,
        #[serde(skip)]
        data: Arc<Vec<u8>>,
        mime_type: Option<String>,
        caption: Option<String>,
    },

    /// Response to a slash command.
    CommandResponse {
        task_id: Option<TaskId>,
        text: String,
        /// The command message that triggered this response (for reply threading).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        trigger_ref: Option<MessageRef>,
    },
}
