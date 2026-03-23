use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use backend_traits::MessagingBackend;
use claude_events::{BackendEvent, OrchestratorEvent};
use tokio::sync::{broadcast, mpsc};
use tracing::info;

use crate::api::{router as api_router, ApiState};
use crate::ws::handle_ws_client;

pub struct WebBackend {
    bind: String,
}

impl WebBackend {
    pub fn new() -> Self {
        Self {
            bind: "0.0.0.0:8080".to_string(),
        }
    }

    pub fn with_bind(bind: impl Into<String>) -> Self {
        Self { bind: bind.into() }
    }
}

impl Default for WebBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessagingBackend for WebBackend {
    fn name(&self) -> &str {
        "web"
    }

    async fn run(
        &self,
        orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()> {
        // Keep the sender and a way to create new receivers from it.
        // We extract the Sender from the Receiver so all new WS clients get their own receiver.
        let orch_tx = orchestrator_events.resubscribe();
        // Drop the original receiver — we resubscribe per-client below.
        drop(orchestrator_events);

        // We need access to the broadcast sender to hand out new receivers.
        // Re-create a channel pair. Actually we can use the existing sender from the bus.
        // The simplest approach: keep a broadcast channel the backend owns for forwarding.
        let (fwd_tx, _fwd_rx) = broadcast::channel::<OrchestratorEvent>(512);
        let fwd_tx = Arc::new(fwd_tx);

        // Forward orchestrator events into our own broadcast channel.
        {
            let fwd_tx_clone = Arc::clone(&fwd_tx);
            let mut orch_rx = orch_tx;
            tokio::spawn(async move {
                loop {
                    match orch_rx.recv().await {
                        Ok(event) => {
                            let _ = fwd_tx_clone.send(event);
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("web: lagged {n} events");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }

        let api_state = ApiState {
            backend_tx: backend_sender.clone(),
            orch_tx: (*fwd_tx).clone(),
        };

        // WebSocket handler that subscribes to our fwd_tx.
        let fwd_tx_ws = Arc::clone(&fwd_tx);
        let backend_tx_ws = backend_sender.clone();
        let ws_handler = move |ws: WebSocketUpgrade| {
            let orch_rx = fwd_tx_ws.subscribe();
            let tx = backend_tx_ws.clone();
            async move {
                ws.on_upgrade(move |socket| handle_ws_client(socket, orch_rx, tx))
            }
        };

        let app = api_router(api_state)
            .route("/ws", get(ws_handler))
            .layer(tower_http::cors::CorsLayer::permissive());

        let bind = self.bind.clone();
        info!("web backend: listening on http://{bind}");

        let listener = tokio::net::TcpListener::bind(&bind).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}
