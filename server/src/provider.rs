//! Provider trait and shared session-orchestration utilities.
//!
//! Messaging providers (Telegram, Discord, …) implement [`MessagingProvider`]
//! and supply a [`Responder`] for each response cycle to receive output
//! callbacks. The shared [`collect_and_respond`] loop drives event collection
//! and calls the responder's methods — keeping all platform-specific formatting
//! out of the core loop.

use std::{
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    protocol::{SessionInfo, SessionStats, SessionStatus, S2C, S2D},
    state::{AppState, SessionBuffer},
};

// ---------------------------------------------------------------------------
// Per-conversation session state
// ---------------------------------------------------------------------------

/// Provider-agnostic per-conversation session state.
pub struct ConversationSession {
    pub session_id: String,
    pub created_at: Instant,
    /// True while a [`collect_and_respond`] task is active for this session.
    /// Prevents spawning duplicate collectors when messages arrive mid-response.
    pub collector_running: Arc<AtomicBool>,
}

impl ConversationSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            created_at: Instant::now(),
            collector_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_expired(&self, lifetime: Duration) -> bool {
        self.created_at.elapsed() > lifetime
    }
}

// ---------------------------------------------------------------------------
// Tool-call entry
// ---------------------------------------------------------------------------

/// A single tool call with its accumulated JSON input and timing information.
pub struct ToolEntry {
    pub name: String,
    /// Accumulated (possibly partial) JSON input string.
    pub input: String,
    pub started: Instant,
    /// `None` while the tool input is still streaming.
    pub ended: Option<Instant>,
}

impl ToolEntry {
    pub fn elapsed_secs(&self) -> u64 {
        match self.ended {
            Some(end) => end.duration_since(self.started).as_secs(),
            None => self.started.elapsed().as_secs(),
        }
    }
}

// ---------------------------------------------------------------------------
// Responder trait
// ---------------------------------------------------------------------------

/// Output callbacks the shared collection loop uses to deliver Claude's
/// response to the messaging platform.
///
/// Providers implement this per-conversation and pass it to
/// [`collect_and_respond`].
#[async_trait]
pub trait Responder: Send + Sync {
    /// The first tool call has started — show a "working" indicator.
    async fn on_working(&self);

    /// Tool-call status changed or the 2-second tick fired.
    ///
    /// Called on every tool completion and periodically so elapsed times stay
    /// current. `done` is `true` on the final call once all tool calls finish.
    async fn update_tool_status(
        &self,
        tools: &[ToolEntry],
        current: Option<&ToolEntry>,
        session_start: Instant,
        done: bool,
    );

    /// Deliver the final response text. Empty string means Claude produced no
    /// text output (tool-only turn), which the provider may choose to ignore.
    async fn send_text(&self, text: &str);

    /// The response cycle is complete (success, timeout, or session end).
    async fn on_done(&self);
}

// ---------------------------------------------------------------------------
// MessagingProvider trait
// ---------------------------------------------------------------------------

/// A messaging platform provider.
///
/// `main` creates one per enabled platform and calls [`start`](Self::start)
/// in a background task.
#[async_trait]
pub trait MessagingProvider: Send + Sync + 'static {
    /// Short identifier used in logs (e.g. `"telegram"`, `"discord"`).
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Start the provider. This typically runs a polling/webhook loop and only
    /// returns when the process exits.
    async fn start(&self, app_state: Arc<AppState>);
}

// ---------------------------------------------------------------------------
// Shared session-creation helper
// ---------------------------------------------------------------------------

/// Creates a new Claude session in `app_state` and sends `StartSession` to the
/// connected client daemon.
///
/// `name` is a human-readable label shown in the dashboard (provider-specific,
/// e.g. `"Telegram 123456"`).
pub async fn create_session(
    app_state: &Arc<AppState>,
    name: Option<String>,
    initial_prompt: Option<String>,
) -> String {
    let session_id = Uuid::new_v4().to_string();
    let claude_session_id = Uuid::new_v4().to_string();

    let hostname = {
        let guard = app_state.client.read().await;
        guard.as_ref().map(|c| c.hostname.clone())
    };

    let info = SessionInfo {
        id: session_id.clone(),
        name,
        cwd: String::new(),
        status: SessionStatus::Pending,
        created_at: Utc::now(),
        started_at: None,
        ended_at: None,
        stats: SessionStats::default(),
        client_hostname: hostname,
        claude_session_id: Some(claude_session_id.clone()),
    };

    app_state.store.save_session(&info).await.ok();

    {
        let mut guard = app_state.sessions.write().await;
        guard.insert(
            session_id.clone(),
            SessionBuffer {
                info: info.clone(),
                events: std::collections::VecDeque::new(),
            },
        );
    }

    app_state
        .broadcast(&S2D::SessionCreated { session: info })
        .await;

    app_state
        .send_to_client(&S2C::StartSession {
            session_id: session_id.clone(),
            initial_prompt,
            extra_args: Vec::new(),
            claude_session_id,
            is_resume: false,
        })
        .await;

    session_id
}

// ---------------------------------------------------------------------------
// Shared response-collection loop
// ---------------------------------------------------------------------------

/// Tick interval for real-time elapsed-time updates.
const TICK_INTERVAL: Duration = Duration::from_secs(2);

/// Maximum time to wait for a Claude response before giving up.
pub const COLLECT_TIMEOUT: Duration = Duration::from_secs(300);

/// Subscribes to the broadcast channel and drives the response cycle for
/// `session_id`, invoking `responder` callbacks as events arrive.
///
/// Returns once the session produces a `result` event, `SessionEnded` fires,
/// or the timeout expires. The caller is responsible for clearing
/// `collector_running` after this future resolves.
pub async fn collect_and_respond(
    session_id: String,
    mut sse_rx: broadcast::Receiver<String>,
    responder: &(impl Responder + ?Sized),
) {
    let deadline = tokio::time::Instant::now() + COLLECT_TIMEOUT;

    let mut text = String::new();

    // Completed tool calls with timing.
    let mut tools: Vec<ToolEntry> = Vec::new();
    // The tool whose input JSON is still streaming.
    let mut current: Option<ToolEntry> = None;
    // Absolute start of the tool-call block (set on first tool call).
    let mut session_start: Option<Instant> = None;
    // Whether on_working() has been called yet.
    let mut working_called = false;

    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;

            _ = tokio::time::sleep_until(deadline) => {
                warn!("provider: response timeout for session {session_id}");
                break;
            }

            result = sse_rx.recv() => {
                match result {
                    Ok(raw) => {
                        let Ok(s2d) = serde_json::from_str::<S2D>(&raw) else {
                            continue;
                        };
                        match s2d {
                            S2D::SessionEvent { session_id: sid, event }
                                if sid == session_id =>
                            {
                                let evt_type =
                                    event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                match evt_type {
                                    "stream_event" => {
                                        let Some(inner) = event.get("event") else {
                                            continue;
                                        };
                                        let inner_type = inner
                                            .get("type")
                                            .and_then(|t| t.as_str())
                                            .unwrap_or("");

                                        match inner_type {
                                            "content_block_start" => {
                                                if inner
                                                    .pointer("/content_block/type")
                                                    .and_then(|t| t.as_str())
                                                    == Some("tool_use")
                                                {
                                                    let name = inner
                                                        .pointer("/content_block/name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown")
                                                        .to_string();
                                                    let now = Instant::now();
                                                    current = Some(ToolEntry {
                                                        name,
                                                        input: String::new(),
                                                        started: now,
                                                        ended: None,
                                                    });
                                                    session_start.get_or_insert(now);

                                                    if !working_called {
                                                        responder.on_working().await;
                                                        working_called = true;
                                                    }
                                                }
                                            }
                                            "content_block_delta" => {
                                                if let Some(t) = inner
                                                    .pointer("/delta/text")
                                                    .and_then(|t| t.as_str())
                                                {
                                                    text.push_str(t);
                                                }
                                                if let Some(partial) = inner
                                                    .pointer("/delta/partial_json")
                                                    .and_then(|t| t.as_str())
                                                {
                                                    if let Some(cur) = current.as_mut() {
                                                        cur.input.push_str(partial);
                                                    }
                                                }
                                            }
                                            "content_block_stop" => {
                                                if let Some(mut entry) = current.take() {
                                                    entry.ended = Some(Instant::now());
                                                    tools.push(entry);
                                                    let start =
                                                        session_start.unwrap_or_else(Instant::now);
                                                    responder
                                                        .update_tool_status(
                                                            &tools, None, start, false,
                                                        )
                                                        .await;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    "result" => {
                                        info!(
                                            "provider: session {session_id} result, {} chars",
                                            text.len()
                                        );
                                        if !tools.is_empty() {
                                            let start =
                                                session_start.unwrap_or_else(Instant::now);
                                            responder
                                                .update_tool_status(&tools, None, start, true)
                                                .await;
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            S2D::SessionEnded {
                                session_id: sid,
                                exit_code,
                                error,
                                ..
                            } if sid == session_id => {
                                if exit_code != 0 {
                                    let msg = error.unwrap_or_else(|| {
                                        format!("Session failed (exit code {exit_code})")
                                    });
                                    warn!("provider: session {session_id} failed: {msg}");
                                    text = format!("❌ {msg}");
                                } else {
                                    info!("provider: session {session_id} ended");
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            "provider: collect_and_respond lagged {n} for session {session_id}"
                        );
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!(
                            "provider: broadcast channel closed for session {session_id}"
                        );
                        break;
                    }
                }
            }

            _ = ticker.tick() => {
                if !tools.is_empty() || current.is_some() {
                    let start = session_start.unwrap_or_else(Instant::now);
                    responder
                        .update_tool_status(&tools, current.as_ref(), start, false)
                        .await;
                }
            }
        }
    }

    responder.send_text(&text).await;
    responder.on_done().await;
}
