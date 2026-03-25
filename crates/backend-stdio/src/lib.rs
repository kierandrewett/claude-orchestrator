//! Stdio backend for local development and testing.

use anyhow::Result;
use async_trait::async_trait;
use backend_traits::MessagingBackend;
use claude_events::{
    BackendEvent, BackendSource, MessageRef, OrchestratorEvent, TaskId, TaskStateSummary,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};

pub struct StdioBackend;

#[async_trait]
impl MessagingBackend for StdioBackend {
    fn name(&self) -> &str {
        "stdio"
    }

    async fn run(
        &self,
        mut orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut lines = BufReader::new(stdin).lines();
        let default_task_id = TaskId("scratchpad".to_string());

        loop {
            tokio::select! {
                biased;

                event = orchestrator_events.recv() => {
                    match event {
                        Ok(ev) => print_event(&ev),
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("stdio: lagged behind by {n} orchestrator events");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("stdio: orchestrator event channel closed, exiting");
                            return Ok(());
                        }
                    }
                }

                line = lines.next_line() => {
                    match line {
                        Ok(None) => {
                            info!("stdio: stdin EOF, exiting");
                            return Ok(());
                        }
                        Ok(Some(text)) => {
                            let text = text.trim().to_string();
                            if text.is_empty() { continue; }
                            let ts = now_ms();
                            let msg_ref = MessageRef::new("stdio", format!("stdin-{ts}"));
                            let source = BackendSource::new("stdio", "local");
                            if text.starts_with('/') {
                                match claude_events::parse_command(&text) {
                                    Ok(cmd) => {
                                        if backend_sender.send(BackendEvent::Command {
                                            command: cmd,
                                            task_id: Some(default_task_id.clone()),
                                            message_ref: msg_ref,
                                            source,
                                        }).await.is_err() { return Ok(()); }
                                    }
                                    Err(e) => eprintln!("parse error: {e}"),
                                }
                            } else if backend_sender.send(BackendEvent::UserMessage {
                                task_id: default_task_id.clone(),
                                text,
                                message_ref: msg_ref,
                                source,
                            }).await.is_err() { return Ok(()); }
                        }
                        Err(e) => {
                            error!("stdio: stdin read error: {e}");
                            return Err(e.into());
                        }
                    }
                }
            }
        }
    }
}

fn print_event(ev: &OrchestratorEvent) {
    match ev {
        OrchestratorEvent::PhaseChanged { task_id, phase, .. } => {
            println!("[{task_id}] {} Phase: {phase:?}", phase.emoji());
        }
        OrchestratorEvent::TextOutput { task_id, text, is_continuation, .. } => {
            if *is_continuation { print!("{text}"); } else { println!("\n[{task_id}] 💬 {text}"); }
        }
        OrchestratorEvent::ToolStarted { task_id, tool_name, summary, .. } => {
            println!("[{task_id}] 🔧 {tool_name}: {summary}");
        }
        OrchestratorEvent::ToolCompleted { task_id, tool_name, is_error, output_preview, .. } => {
            let s = if *is_error { "❌" } else { "✅" };
            println!("[{task_id}] 🔧 {tool_name} → {s} {}", output_preview.as_deref().unwrap_or(""));
        }
        OrchestratorEvent::Thinking { task_id, text, .. } => {
            println!("[{task_id}] 🤔 {text}");
        }
        OrchestratorEvent::TurnComplete { task_id, usage, duration_secs, .. } => {
            println!("[{task_id}] ✅ Done — {duration_secs:.1}s, ${:.4} ({} in / {} out)",
                usage.total_cost_usd, usage.input_tokens, usage.output_tokens);
        }
        OrchestratorEvent::TaskCreated { task_id, name, profile, kind, .. } => {
            println!("[{task_id}] 🆕 Task '{name}' created (profile={profile}, kind={kind:?})");
        }
        OrchestratorEvent::TaskStateChanged { task_id, old_state, new_state } => {
            println!("[{task_id}] {} → {}", state_emoji(old_state), state_emoji(new_state));
        }
        OrchestratorEvent::Error { task_id, error, next_steps, .. } => {
            let id = task_id.as_ref().map(|t| t.to_string()).unwrap_or_else(|| "—".to_string());
            eprintln!("[{id}] ❌ {error}");
            for s in next_steps { eprintln!("  • {s}"); }
        }
        OrchestratorEvent::FileOutput { task_id, filename, .. } => {
            println!("[{task_id}] 📎 {filename}");
        }
        OrchestratorEvent::CommandResponse { task_id, text, .. } => {
            let id = task_id.as_ref().map(|t| t.to_string()).unwrap_or_else(|| "—".to_string());
            println!("[{id}] ℹ️  {text}");
        }
        OrchestratorEvent::QueuedMessageDelivered { task_id, .. } => {
            println!("[{task_id}] 📥 Queued message delivered");
        }
        OrchestratorEvent::MessageQueued { task_id, .. } => {
            println!("[{task_id}] ⏰ Message queued (Claude is busy)");
        }
        OrchestratorEvent::ConversationRenamed { task_id, title } => {
            println!("[{task_id}] ✏️  Conversation renamed to '{title}'");
        }
        OrchestratorEvent::McpList { entries, .. } => {
            println!("MCP servers:");
            for e in entries {
                let status = if e.enabled { "✅" } else { "❌" };
                let detail = e.command.as_deref().unwrap_or("built-in");
                println!("  {status} {} — {detail}", e.name);
            }
        }
        OrchestratorEvent::ClientConnected { client_id, hostname } => {
            println!("[client] 🟢 Connected: {client_id} ({hostname})");
        }
        OrchestratorEvent::ClientDisconnected { client_id, hostname } => {
            println!("[client] 🔴 Disconnected: {client_id} ({hostname})");
        }
    }
}

fn state_emoji(s: &TaskStateSummary) -> &'static str {
    match s {
        TaskStateSummary::Running => "🟢",
        TaskStateSummary::Hibernated => "💤",
        TaskStateSummary::Dead => "💀",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
