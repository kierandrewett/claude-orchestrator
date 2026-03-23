//! The MessagingBackend trait shared by all backend implementations.

use anyhow::Result;
use async_trait::async_trait;
use claude_events::{BackendEvent, OrchestratorEvent};
use tokio::sync::{broadcast, mpsc};

/// A messaging backend (Telegram, Discord, stdio, web, …).
///
/// Implementations receive all orchestrator events via a `broadcast::Receiver`
/// and push `BackendEvent`s to the orchestrator via an `mpsc::Sender`.
#[async_trait]
pub trait MessagingBackend: Send + Sync + 'static {
    /// A short identifier for this backend (e.g. "telegram", "stdio").
    fn name(&self) -> &str;

    /// Run the backend until it terminates or returns an error.
    ///
    /// The orchestrator will restart the backend after a short delay if this
    /// returns an `Err`.
    async fn run(
        &self,
        orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()>;
}
