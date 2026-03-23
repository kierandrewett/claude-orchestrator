//! Discord messaging backend (stub).

use anyhow::Result;
use async_trait::async_trait;
use backend_traits::MessagingBackend;
use claude_events::{BackendEvent, OrchestratorEvent};
use tokio::sync::{broadcast, mpsc};
use tracing::info;

pub struct DiscordBackend {
    bot_token: String,
}

impl DiscordBackend {
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
        }
    }
}

#[async_trait]
impl MessagingBackend for DiscordBackend {
    fn name(&self) -> &str {
        "discord"
    }

    async fn run(
        &self,
        mut orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        _backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()> {
        info!("discord backend: not yet implemented");

        // Drain orchestrator events until the channel closes, then exit.
        loop {
            match orchestrator_events.recv().await {
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    info!("discord backend: orchestrator event channel closed, exiting");
                    return Ok(());
                }
            }
        }
    }
}
