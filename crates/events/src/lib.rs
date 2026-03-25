//! Event bus and event types for the orchestrator ↔ backend message passing.

pub mod backend_events;
pub mod bus;
pub mod commands;
pub mod orchestrator_events;
pub mod types;

pub use backend_events::BackendEvent;
pub use bus::EventBus;
pub use commands::{parse as parse_command, ParsedCommand};
pub use orchestrator_events::OrchestratorEvent;
pub use types::{
    BackendSource, EventListEntry, McpEntry, MessageRef, SessionPhase, TaskId, TaskKind,
    TaskStateSummary, TaskSummary,
};
