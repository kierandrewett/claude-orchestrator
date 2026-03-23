//! Discord messaging backend powered by serenity + poise.
//!
//! Each Claude task gets its own public thread inside a configured text
//! channel. Orchestrator events are rendered and posted to the thread.
//! Slash commands (/new, /stop, /status, etc.) are registered via poise;
//! plain text messages in a task thread are forwarded as UserMessage events.

mod backend;
mod commands;
mod formatting;

pub use backend::{DiscordBackend, DiscordConfig};
