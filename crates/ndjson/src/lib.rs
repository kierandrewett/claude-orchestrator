//! Claude Code NDJSON streaming protocol — types, transport, coalescer, usage stats.

pub mod coalescer;
pub mod transport;
pub mod types;
pub mod usage;

pub use coalescer::{CoalescedEvent, CoalescedStream};
pub use transport::NdjsonTransport;
pub use types::{
    AssistantMessage, ClaudeEvent, ContentBlock, FinalResult, SystemInfo, ToolResultEvent,
    ToolUseRequest, TokenUsage, UserInput, UserMessage,
};
pub use usage::UsageStats;
