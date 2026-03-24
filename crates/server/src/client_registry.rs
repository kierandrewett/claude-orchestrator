use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use claude_shared::S2C;

/// Routes S2C messages to the correct client-daemon WebSocket connection.
///
/// One entry per connected client; sessions within that client are multiplexed
/// over the same connection because the client routes by `session_id` itself.
pub struct ClientRegistry {
    /// client_id → unbounded sender for this client's WebSocket write half.
    clients: DashMap<String, mpsc::UnboundedSender<S2C>>,
    /// session_id → client_id, so we can route by session.
    session_to_client: DashMap<String, String>,
}

impl ClientRegistry {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            session_to_client: DashMap::new(),
        }
    }

    /// Register a connected client daemon.
    pub fn register_client(&self, client_id: String, tx: mpsc::UnboundedSender<S2C>) {
        debug!("client-registry: registered client {client_id}");
        self.clients.insert(client_id, tx);
    }

    /// Unregister a client and all its sessions.
    pub fn unregister_client(&self, client_id: &str) {
        self.clients.remove(client_id);
        self.session_to_client.retain(|_, v| v.as_str() != client_id);
        debug!("client-registry: unregistered client {client_id}");
    }

    /// Return all session_ids currently mapped to the given client.
    pub fn sessions_for_client(&self, client_id: &str) -> Vec<String> {
        self.session_to_client
            .iter()
            .filter(|e| e.value().as_str() == client_id)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Associate a session with the client that is running it.
    pub fn register_session(&self, session_id: String, client_id: String) {
        self.session_to_client.insert(session_id, client_id);
    }

    /// Remove a session association.
    pub fn unregister_session(&self, session_id: &str) {
        self.session_to_client.remove(session_id);
    }

    /// Send an S2C message to the client holding `session_id`.
    /// Returns `true` if the message was delivered.
    pub fn send_to_session(&self, session_id: &str, msg: S2C) -> bool {
        let client_id = match self.session_to_client.get(session_id) {
            Some(c) => c.clone(),
            None => {
                warn!("client-registry: no client for session {session_id}");
                return false;
            }
        };
        self.send_to_client(&client_id, msg)
    }

    /// Send an S2C message to a specific client by ID.
    pub fn send_to_client(&self, client_id: &str, msg: S2C) -> bool {
        match self.clients.get(client_id) {
            Some(tx) => {
                if tx.send(msg).is_err() {
                    warn!("client-registry: client {client_id} channel closed");
                    false
                } else {
                    true
                }
            }
            None => {
                warn!("client-registry: client {client_id} not found");
                false
            }
        }
    }

    /// Send to any connected client (used for new session starts).
    /// Returns `true` if at least one client received the message.
    pub fn send_to_any_client(&self, msg: S2C) -> bool {
        for entry in self.clients.iter() {
            if entry.value().send(msg.clone()).is_ok() {
                return true;
            }
        }
        warn!("client-registry: no clients connected");
        false
    }

    #[allow(dead_code)]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self::new()
    }
}
