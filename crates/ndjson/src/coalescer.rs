use std::time::Duration;

use anyhow::Result;
use tokio::time::Instant;

use crate::transport::NdjsonTransport;
use crate::types::{ClaudeEvent, ContentBlock};

const COALESCE_WINDOW: Duration = Duration::from_millis(500);

/// A coalesced event emitted by `CoalescedStream`.
#[derive(Debug)]
pub enum CoalescedEvent {
    /// One or more assistant text chunks buffered for up to 500 ms.
    Text {
        text: String,
        /// `false` for the first chunk of a new response, `true` for continuations.
        is_continuation: bool,
    },
    /// A thinking/internal-monologue block from the assistant.
    Thinking(String),
    /// Tool call started.
    ToolStarted { name: String, input: serde_json::Value },
    /// Tool result returned (marks a turn boundary for the stdin queue).
    ToolResult { is_error: bool, preview: Option<String> },
    /// Claude Code's final result for this turn.
    TurnComplete(crate::types::FinalResult),
    /// Session initialisation info.
    System(crate::types::SystemInfo),
    /// The transport has reached EOF.
    Eof,
}

/// Wraps `NdjsonTransport` and coalesces assistant text chunks.
///
/// Text events are buffered for up to 500 ms so that a rapid burst of small
/// deltas is combined into fewer, larger messages. Tool events, ToolResult, and
/// Result bypass the buffer immediately (flushing any pending text first).
pub struct CoalescedStream {
    transport: NdjsonTransport,
    pending_text: String,
    /// `true` until we have emitted at least one Text event in this response.
    first_text_chunk: bool,
    /// When the 500 ms window closes (set when text first arrives).
    deadline: Option<Instant>,
    /// A non-text event read while we were flushing pending text.
    stash: Option<ClaudeEvent>,
}

impl CoalescedStream {
    pub fn new(transport: NdjsonTransport) -> Self {
        Self {
            transport,
            pending_text: String::new(),
            first_text_chunk: true,
            deadline: None,
            stash: None,
        }
    }

    /// Return the next coalesced event.
    pub async fn next_coalesced(&mut self) -> Result<CoalescedEvent> {
        loop {
            // Return the stashed non-text event if one is waiting.
            if let Some(ev) = self.stash.take() {
                if let Some(coalesced) = self.convert_passthrough(ev) {
                    return Ok(coalesced);
                }
                continue;
            }

            // If we have pending text, wait for either the deadline or a new event.
            if !self.pending_text.is_empty() {
                let deadline = self
                    .deadline
                    .get_or_insert_with(|| Instant::now() + COALESCE_WINDOW);
                let remaining = deadline.saturating_duration_since(Instant::now());

                tokio::select! {
                    biased;

                    event = self.transport.next_event() => {
                        match event? {
                            None => return Ok(self.flush_text()),
                            Some(ev) => {
                                if self.try_append_text(&ev) {
                                    continue;
                                }
                                // Non-text: flush first, stash the event.
                                let flushed = self.flush_text();
                                self.stash = Some(ev);
                                return Ok(flushed);
                            }
                        }
                    }

                    _ = tokio::time::sleep(remaining) => {
                        self.deadline = None;
                        return Ok(self.flush_text());
                    }
                }
            }

            // No pending text — read the next event normally.
            match self.transport.next_event().await? {
                None => return Ok(CoalescedEvent::Eof),
                Some(ev) => {
                    if self.try_append_text(&ev) {
                        continue;
                    }
                    if let Some(coalesced) = self.convert_passthrough(ev) {
                        return Ok(coalesced);
                    }
                }
            }
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    /// Try to extract text content and append to the buffer. Returns `true` if
    /// the event was fully consumed as text.
    fn try_append_text(&mut self, ev: &ClaudeEvent) -> bool {
        if let ClaudeEvent::Assistant(ref msg) = ev {
            if let Some(ref body) = msg.message {
                let mut found = false;
                for block in &body.content {
                    if let ContentBlock::Text { ref text } = block {
                        self.pending_text.push_str(text);
                        self.deadline.get_or_insert_with(|| Instant::now() + COALESCE_WINDOW);
                        found = true;
                    }
                }
                return found;
            }
        }
        false
    }

    fn flush_text(&mut self) -> CoalescedEvent {
        let text = std::mem::take(&mut self.pending_text);
        let is_continuation = !self.first_text_chunk;
        self.first_text_chunk = false;
        self.deadline = None;
        CoalescedEvent::Text { text, is_continuation }
    }

    fn convert_passthrough(&mut self, ev: ClaudeEvent) -> Option<CoalescedEvent> {
        match ev {
            ClaudeEvent::System(s) => Some(CoalescedEvent::System(s)),
            ClaudeEvent::Assistant(ref msg) => {
                if let Some(ref body) = msg.message {
                    for block in &body.content {
                        match block {
                            ContentBlock::Thinking { ref thinking } => {
                                return Some(CoalescedEvent::Thinking(thinking.clone()));
                            }
                            ContentBlock::ToolUse { ref name, ref input, .. } => {
                                return Some(CoalescedEvent::ToolStarted {
                                    name: name.clone(),
                                    input: input.clone(),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            ClaudeEvent::ToolUse(tu) => Some(CoalescedEvent::ToolStarted {
                name: tu.name.unwrap_or_default(),
                input: tu.input,
            }),
            ClaudeEvent::ToolResult(tr) => {
                let preview = tr.content.as_ref().and_then(|c| {
                    let s = c
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| c.to_string());
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.chars().take(200).collect())
                    }
                });
                Some(CoalescedEvent::ToolResult {
                    is_error: tr.is_error.unwrap_or(false),
                    preview,
                })
            }
            ClaudeEvent::Result(fr) => {
                self.first_text_chunk = true; // reset for next turn
                Some(CoalescedEvent::TurnComplete(fr))
            }
            ClaudeEvent::Unknown => None,
        }
    }
}
