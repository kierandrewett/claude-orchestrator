use serde::{Deserialize, Serialize};

use claude_events::{TaskSummary};

/// A structured interpretation of a voice transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InterpretedVoiceCommand {
    /// "Start a new rust task to fix the parser"
    NewTask {
        profile: Option<String>,
        prompt: String,
    },

    /// "Tell the auth task to update the middleware tests"
    SendMessage {
        task_hint: String,
        message: String,
    },

    /// "What's the status?" / "How much has it cost?"
    RunCommand { command: String },

    /// "Stop the parser task"
    StopTask { task_hint: String },

    /// "Hibernate everything"
    HibernateTask { task_hint: String },

    /// Couldn't interpret — pass through as a message to the current task.
    Passthrough { text: String },
}

/// Context provided to `interpret_voice` so the LLM can resolve task references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceContext {
    pub active_tasks: Vec<TaskSummary>,
    pub available_profiles: Vec<String>,
    /// The task associated with the channel/topic the voice message came from.
    pub current_task: Option<TaskSummary>,
}

/// Configuration for the orchestrator LLM.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestratorLlmConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}
