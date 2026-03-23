//! Lightweight LLM client for voice interpretation and event summarisation.

pub mod interpreter;
pub mod summariser;
pub mod types;

pub use interpreter::OrchestratorLlm;
pub use types::{InterpretedVoiceCommand, OrchestratorLlmConfig, VoiceContext};
