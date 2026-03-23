use std::collections::HashMap;

use claude_events::SessionPhase;

/// Tracks the current reaction per user message to avoid redundant API calls.
pub struct ReactionTracker {
    /// Map of message_id → current SessionPhase emoji string.
    current: HashMap<String, String>,
}

impl ReactionTracker {
    pub fn new() -> Self {
        Self {
            current: HashMap::new(),
        }
    }

    /// Returns the emoji for the phase if it changed (to avoid duplicate setMessageReaction calls).
    pub fn should_update(&mut self, message_id: &str, phase: &SessionPhase) -> Option<&'static str> {
        let emoji = phase.emoji();
        let current = self.current.get(message_id).map(|s| s.as_str());
        if current != Some(emoji) {
            self.current.insert(message_id.to_string(), emoji.to_string());
            Some(emoji)
        } else {
            None
        }
    }

    pub fn clear(&mut self, message_id: &str) {
        self.current.remove(message_id);
    }
}

impl Default for ReactionTracker {
    fn default() -> Self {
        Self::new()
    }
}
