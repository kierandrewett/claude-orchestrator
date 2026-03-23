use tokio::sync::{broadcast, mpsc};

use crate::backend_events::BackendEvent;
use crate::orchestrator_events::OrchestratorEvent;

/// Capacity of the orchestrator broadcast channel.
const ORCH_CHANNEL_CAPACITY: usize = 512;
/// Capacity of the backends→orchestrator mpsc channel.
const BACKEND_CHANNEL_CAPACITY: usize = 256;

/// The central event bus.
///
/// - Orchestrator → backends: `broadcast::Sender<OrchestratorEvent>`.
///   Every backend holds its own `broadcast::Receiver` and sees all events.
///
/// - Backends → orchestrator: `mpsc::Sender<BackendEvent>` (one per backend),
///   all feeding into the single `mpsc::Receiver` the orchestrator polls.
pub struct EventBus {
    orch_tx: broadcast::Sender<OrchestratorEvent>,
    backend_tx: mpsc::Sender<BackendEvent>,
    backend_rx: Option<mpsc::Receiver<BackendEvent>>,
}

impl EventBus {
    pub fn new() -> Self {
        let (orch_tx, _) = broadcast::channel(ORCH_CHANNEL_CAPACITY);
        let (backend_tx, backend_rx) = mpsc::channel(BACKEND_CHANNEL_CAPACITY);
        Self {
            orch_tx,
            backend_tx,
            backend_rx: Some(backend_rx),
        }
    }

    /// Subscribe to orchestrator events. Each subscriber gets all events.
    pub fn subscribe_orchestrator(&self) -> broadcast::Receiver<OrchestratorEvent> {
        self.orch_tx.subscribe()
    }

    /// Get a clone of the sender that backends use to push events to the orchestrator.
    pub fn backend_sender(&self) -> mpsc::Sender<BackendEvent> {
        self.backend_tx.clone()
    }

    /// Emit an event to all subscribed backends.
    pub fn emit(&self, event: OrchestratorEvent) {
        // A send error simply means no backends are subscribed yet — that's fine.
        let _ = self.orch_tx.send(event);
    }

    /// Take the receiver so the orchestrator can poll backend events.
    /// Panics if called more than once.
    pub fn take_backend_receiver(&mut self) -> mpsc::Receiver<BackendEvent> {
        self.backend_rx
            .take()
            .expect("take_backend_receiver called more than once")
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator_events::OrchestratorEvent;
    use crate::types::{SessionPhase, TaskId};

    #[tokio::test]
    async fn bus_emit_subscribe_roundtrip() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_orchestrator();

        bus.emit(OrchestratorEvent::CommandResponse {
            task_id: None,
            text: "hello".to_string(),
        });

        let event = rx.try_recv().expect("event should be available");
        assert!(matches!(event, OrchestratorEvent::CommandResponse { .. }));
    }

    #[tokio::test]
    async fn bus_backend_sender_roundtrip() {
        use crate::backend_events::BackendEvent;
        use crate::types::{BackendSource, MessageRef};

        let mut bus = EventBus::new();
        let tx = bus.backend_sender();
        let mut rx = bus.take_backend_receiver();

        let msg_ref = MessageRef::new("test", "msg-1");
        let source = BackendSource::new("test", "user-1");

        tx.send(BackendEvent::UserMessage {
            task_id: TaskId("t1".to_string()),
            text: "hello".to_string(),
            message_ref: msg_ref,
            source,
        })
        .await
        .unwrap();

        let event = rx.recv().await.expect("should receive event");
        assert!(matches!(event, BackendEvent::UserMessage { .. }));
    }
}
