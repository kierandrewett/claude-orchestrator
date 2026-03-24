use std::sync::Arc;

use tracing::{error, info, warn};
use uuid::Uuid;

use claude_events::{
    BackendEvent, EventBus, MessageRef, OrchestratorEvent, ParsedCommand, SessionPhase, TaskId,
    TaskKind, TaskStateSummary,
};
use claude_shared::S2C;

use crate::client_registry::ClientRegistry;
use crate::commands::{build_cost, build_status};
use crate::config::OrchestratorConfig;
use crate::mcp_registry::{McpServerEntry, McpServerRegistry};
use crate::task_manager::{Task, TaskRegistry, TaskState};

pub struct Orchestrator {
    pub bus: Arc<EventBus>,
    pub registry: Arc<TaskRegistry>,
    pub clients: Arc<ClientRegistry>,
    #[allow(dead_code)]
    pub config: OrchestratorConfig,
    pub store: Arc<crate::persistence::StateStore>,
    pub mcp_registry: Arc<McpServerRegistry>,
    /// Merged MCP env extras from all backend capability announcements.
    backend_caps: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl Orchestrator {
    pub fn new(
        bus: Arc<EventBus>,
        registry: Arc<TaskRegistry>,
        clients: Arc<ClientRegistry>,
        config: OrchestratorConfig,
        store: Arc<crate::persistence::StateStore>,
        mcp_registry: Arc<McpServerRegistry>,
    ) -> Self {
        Self { bus, registry, clients, config, store, mcp_registry, backend_caps: Default::default() }
    }

    fn backend_mcp_extra_env(&self) -> std::collections::HashMap<String, String> {
        self.backend_caps.lock().unwrap().clone()
    }

    /// Build the system prompt to inject into every new session.
    ///
    /// Prepends a fixed instruction so Claude always calls `rename_conversation`
    /// after its first response, then appends any user-configured system prompt.
    fn session_system_prompt(&self, is_scratchpad: bool) -> Option<String> {
        const RENAME_INSTRUCTION: &str =
            "You have access to a tool called `mcp__orchestrator__rename_conversation`. \
             Use it to keep the conversation title up to date. \
             Call it after your first response, and again whenever the topic has meaningfully shifted. \
             The title should reflect what is being discussed right now, not just the opening message. \
             Format the title as an emoji followed by a short phrase (3-5 words). \
             CRITICAL RULES: \
             (1) Call `mcp__orchestrator__rename_conversation` directly by that exact name — NEVER use ToolSearch to look it up first. \
             (2) The rename call must be the absolute last thing in your response — no text before or after it, no acknowledgement, nothing.";

        let base = if is_scratchpad {
            // Scratchpad has a fixed title — no rename instruction.
            self.config.server.system_prompt.clone()
        } else {
            match &self.config.server.system_prompt {
                Some(user_prompt) => Some(format!("{RENAME_INSTRUCTION}\n\n{user_prompt}")),
                None => Some(RENAME_INSTRUCTION.to_string()),
            }
        };
        base
    }

    fn is_scratchpad(task_id: &TaskId) -> bool {
        task_id.0 == "scratchpad"
    }

    /// Build the list of custom MCP servers and disabled server names to pass to StartSession.
    fn mcp_session_args(&self) -> (Vec<claude_shared::McpServerDef>, Vec<String>) {
        let custom = self.mcp_registry.custom_servers();
        let disabled = self.mcp_registry.disabled_names();
        let servers = custom
            .into_iter()
            .filter(|s| !s.disabled)
            .map(|s| claude_shared::McpServerDef {
                name: s.name,
                command: s.command,
                args: s.args,
                env: s.env,
            })
            .collect();
        (servers, disabled)
    }

    fn save_state(&self) {
        use crate::persistence::{PersistedState, PersistedTask, PersistedTaskState};
        let tasks = self.registry.all_ids().into_iter().filter_map(|id| {
            self.registry.with(&id, |t| PersistedTask {
                id: t.id.clone(),
                name: t.name.clone(),
                profile: t.profile.clone(),
                claude_session_id: t.claude_session_id.clone(),
                usage: t.usage.clone(),
                created_at: t.created_at,
                last_activity: t.last_activity,
                state: match t.state {
                    crate::task_manager::TaskState::Running { .. } => PersistedTaskState::Hibernated,
                    crate::task_manager::TaskState::Hibernated => PersistedTaskState::Hibernated,
                    crate::task_manager::TaskState::Dead => PersistedTaskState::Dead,
                },
                kind: t.kind.clone(),
            })
        }).collect();
        self.store.save(&PersistedState { tasks });
    }

    /// Run the main orchestrator loop.
    pub async fn run(self: Arc<Self>, mut backend_rx: tokio::sync::mpsc::Receiver<BackendEvent>) {
        info!("orchestrator: starting main loop");
        loop {
            match backend_rx.recv().await {
                Some(event) => self.handle_backend_event(event).await,
                None => {
                    info!("orchestrator: backend channel closed, shutting down");
                    break;
                }
            }
        }
    }

    async fn handle_backend_event(&self, event: BackendEvent) {
        match event {
            BackendEvent::BackendCapabilities { backend_name, mcp_env } => {
                info!("orchestrator: received backend capabilities from '{backend_name}'");
                let mut caps = self.backend_caps.lock().unwrap();
                caps.extend(mcp_env);
            }
            BackendEvent::UserMessage { task_id, text, message_ref, .. } => {
                self.handle_user_message(&task_id, text, Some(message_ref)).await;
            }
            BackendEvent::Command { command, task_id, message_ref, .. } => {
                self.handle_command(command, task_id, message_ref).await;
            }
            BackendEvent::FileUpload {
                task_id, filename, data, mime_type, caption, message_ref, ..
            } => {
                use base64::{engine::general_purpose::STANDARD, Engine as _};
                let file = claude_shared::AttachedFile {
                    mime_type: mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
                    data_base64: STANDARD.encode(&*data),
                    filename,
                };
                self.handle_user_message_with_files(
                    &task_id,
                    caption.unwrap_or_default(),
                    vec![file],
                    Some(message_ref),
                ).await;
            }
            BackendEvent::InterruptTask { task_id, .. } => {
                self.interrupt_task(&task_id).await;
            }
            BackendEvent::CancelQueuedMessage { task_id, message_ref, .. } => {
                self.cancel_queued_message(&task_id, message_ref).await;
            }
        }
    }

    async fn interrupt_task(&self, task_id: &TaskId) {
        info!("orchestrator: interrupting task {task_id}");
        let session_id = self.running_session(task_id);
        if let Some(sid) = session_id {
            self.clients.send_to_session(&sid, S2C::InterruptSession { session_id: sid.clone() });
        } else {
            warn!("orchestrator: task {task_id} is not Running, cannot interrupt");
        }
        self.bus.emit(OrchestratorEvent::CommandResponse {
            task_id: Some(task_id.clone()),
            text: "⏹ Interrupting…".to_string(),
            trigger_ref: None,
        });
    }

    async fn cancel_queued_message(&self, task_id: &TaskId, message_ref: MessageRef) {
        info!("orchestrator: cancel queued message {:?} for task {task_id}", message_ref.opaque_id);
        let session_id = self.running_session(task_id);
        if let Some(sid) = session_id {
            self.clients.send_to_session(&sid, S2C::CancelQueuedInput {
                session_id: sid.clone(),
                message_ref_opaque_id: message_ref.opaque_id.clone(),
            });
        }
        // Optimistically clear the queued indicator so backends remove the ⏰ reaction.
        self.bus.emit(OrchestratorEvent::QueuedMessageDelivered {
            task_id: task_id.clone(),
            original_ref: message_ref,
        });
    }

    async fn handle_user_message(
        &self,
        task_id: &TaskId,
        text: String,
        msg_ref: Option<MessageRef>,
    ) {
        self.handle_user_message_with_files(task_id, text, vec![], msg_ref).await;
    }

    async fn handle_user_message_with_files(
        &self,
        task_id: &TaskId,
        text: String,
        files: Vec<claude_shared::AttachedFile>,
        msg_ref: Option<MessageRef>,
    ) {
        if self.clients.client_count() == 0 {
            self.bus.emit(OrchestratorEvent::Error {
                task_id: Some(task_id.clone()),
                error: "No client daemon is connected.".to_string(),
                next_steps: vec!["Start the claude-client daemon and ensure it can reach this server.".to_string()],
                trigger_ref: msg_ref,
            });
            return;
        }

        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Acknowledged,
            trigger_message: msg_ref.clone(),
        });

        let claude_idle = self.registry.with(task_id, |t| t.claude_idle).unwrap_or(true);
        if !claude_idle {
            if let Some(ref mr) = msg_ref {
                self.bus.emit(OrchestratorEvent::MessageQueued {
                    task_id: task_id.clone(),
                    message_ref: mr.clone(),
                });
            }
        }

        self.send_to_client(task_id, text, files, msg_ref).await;
    }

    async fn send_to_client(
        &self,
        task_id: &TaskId,
        text: String,
        files: Vec<claude_shared::AttachedFile>,
        msg_ref: Option<MessageRef>,
    ) {
        let task_summary = self.registry.with(task_id, |t| t.state.summary());

        match task_summary {
            Some(TaskStateSummary::Running) => {
                // Happy path: update state and forward the message.
                let session_id = self.registry.with_mut(task_id, |t| {
                    t.claude_idle = false;
                    t.current_trigger = msg_ref.clone();
                    t.last_activity = chrono::Utc::now();
                    if let TaskState::Running { ref session_id } = t.state {
                        Some(session_id.clone())
                    } else {
                        None
                    }
                }).flatten();

                self.bus.emit(OrchestratorEvent::PhaseChanged {
                    task_id: task_id.clone(),
                    phase: SessionPhase::Responding,
                    trigger_message: msg_ref.clone(),
                });

                if let Some(sid) = session_id {
                    let message_ref_opaque_id = msg_ref.map(|r| r.opaque_id);
                    if files.is_empty() {
                        self.clients.send_to_session(&sid, S2C::SendInput {
                            session_id: sid.clone(),
                            text,
                            message_ref_opaque_id,
                        });
                    } else {
                        self.clients.send_to_session(&sid, S2C::SendInputWithFiles {
                            session_id: sid.clone(),
                            text,
                            files,
                            message_ref_opaque_id,
                        });
                    }
                }
            }
            None | Some(TaskStateSummary::Hibernated) => {
                // Task doesn't exist or was hibernated — (re)start session.
                // We use the existing task_id rather than generating a new one so
                // backends (e.g. Telegram) that address by fixed IDs keep working.
                let session_id = Uuid::new_v4().to_string();
                // Resume the previous conversation if we have the session ID, otherwise start fresh.
                let (claude_session_id, is_resume) = self.registry
                    .with(task_id, |t| t.claude_session_id.clone())
                    .flatten()
                    .map(|id| (id, true))
                    .unwrap_or_else(|| (Uuid::new_v4().to_string(), false));
                let (name, profile) = self.registry
                    .with(task_id, |t| (t.name.clone(), t.profile.clone()))
                    .unwrap_or_else(|| (task_id.0.clone(), self.config.docker.default_profile.clone()));

                let mut task = Task::new(
                    task_id.clone(),
                    name,
                    profile,
                    TaskState::Running { session_id: session_id.clone() },
                    TaskKind::Scratchpad,
                );
                task.current_trigger = msg_ref.clone();
                task.claude_idle = false;
                task.claude_session_id = Some(claude_session_id.clone());
                self.registry.insert(task);

                self.bus.emit(OrchestratorEvent::PhaseChanged {
                    task_id: task_id.clone(),
                    phase: SessionPhase::Starting,
                    trigger_message: msg_ref.clone(),
                });

                let scratchpad = Self::is_scratchpad(task_id);
                let (mcp_servers, disabled_mcp_servers) = self.mcp_session_args();
                let suppress_mcp_tools = if scratchpad {
                    vec!["rename_conversation".to_string()]
                } else {
                    vec![]
                };
                info!("orchestrator: starting session for {task_id} (resume={is_resume})");
                let delivered = self.clients.send_to_any_client(S2C::StartSession {
                    session_id,
                    initial_prompt: Some(text),
                    initial_files: files,
                    extra_args: vec![],
                    claude_session_id,
                    is_resume,
                    system_prompt: self.session_system_prompt(scratchpad),
                    mcp_servers,
                    disabled_mcp_servers,
                    suppress_mcp_tools,
                    mcp_extra_env: self.backend_mcp_extra_env(),
                });

                if !delivered {
                    error!("orchestrator: no client connected for auto-created task {task_id}");
                    self.registry.with_mut(task_id, |t| t.state = TaskState::Dead);
                    self.bus.emit(OrchestratorEvent::Error {
                        task_id: Some(task_id.clone()),
                        error: "No client daemon connected.".to_string(),
                        next_steps: vec!["Start the claude-client daemon.".to_string()],
                        trigger_ref: msg_ref,
                    });
                }
            }
            Some(TaskStateSummary::Dead) => {
                warn!("orchestrator: task {task_id} is Dead, dropping message");
            }
        }
    }

    async fn handle_command(
        &self,
        command: ParsedCommand,
        task_id: Option<TaskId>,
        msg_ref: MessageRef,
    ) {
        let trigger_ref = Some(msg_ref);
        match command {
            ParsedCommand::Status => {
                let text = build_status(&self.registry);
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::Cost { all } => {
                let text = build_cost(&self.registry, all, task_id.as_ref());
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::Config { key, value } => {
                if let Some(ref id) = task_id {
                    self.registry.with_mut(id, |t| {
                        if key == "thinking" {
                            t.config.show_thinking = value == "on" || value == "true";
                        }
                    });
                    self.bus.emit(OrchestratorEvent::CommandResponse {
                        task_id,
                        text: format!("Config updated: {key} = {value}"),
                        trigger_ref,
                    });
                }
            }
            ParsedCommand::Cancel => {
                if let Some(id) = task_id {
                    self.interrupt_task(&id).await;
                }
            }
            ParsedCommand::Stop { task_id: stop_id } => {
                let id = stop_id.or(task_id);
                if let Some(id) = id {
                    self.stop_task(&id).await;
                }
            }
            ParsedCommand::Hibernate => {
                if let Some(id) = task_id {
                    self.hibernate_task(&id).await;
                }
            }
            ParsedCommand::New { profile, prompt } => {
                self.create_task(profile, prompt, TaskKind::Job).await;
            }
            ParsedCommand::McpList => {
                let text = self.mcp_registry.list_display();
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::McpAdd { name, command, args } => {
                let text = match self.mcp_registry.add(McpServerEntry {
                    name: name.clone(),
                    command: command.clone(),
                    args: args.clone(),
                    env: Default::default(),
                    disabled: false,
                }) {
                    Ok(()) => format!("Added MCP server '{name}' ({command})"),
                    Err(e) => format!("Error: {e}"),
                };
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::McpRemove { name } => {
                let text = match self.mcp_registry.remove(&name) {
                    Ok(()) => format!("Removed MCP server '{name}'"),
                    Err(e) => format!("Error: {e}"),
                };
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::McpDisable { name } => {
                let text = match self.mcp_registry.disable(&name) {
                    Ok(()) => format!("Disabled MCP server '{name}'"),
                    Err(e) => format!("Error: {e}"),
                };
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
            ParsedCommand::McpEnable { name } => {
                let text = match self.mcp_registry.enable(&name) {
                    Ok(()) => format!("Enabled MCP server '{name}'"),
                    Err(e) => format!("Error: {e}"),
                };
                self.bus.emit(OrchestratorEvent::CommandResponse { task_id, text, trigger_ref });
            }
        }
    }

    async fn stop_task(&self, task_id: &TaskId) {
        let old_state = self.registry.with(task_id, |t| t.state.summary());
        if let Some(old) = old_state {
            if let Some(task) = self.registry.remove(task_id) {
                if let TaskState::Running { session_id } = task.state {
                    self.clients.send_to_session(
                        &session_id,
                        S2C::KillSession { session_id: session_id.clone() },
                    );
                }
            }
            self.bus.emit(OrchestratorEvent::TaskStateChanged {
                task_id: task_id.clone(),
                old_state: old,
                new_state: TaskStateSummary::Dead,
            });
            self.save_state();
        }
    }

    pub async fn hibernate_task(&self, task_id: &TaskId) {
        if let Some(mut task) = self.registry.remove(task_id) {
            let old_state = task.state.summary();
            if let TaskState::Running { ref session_id } = task.state {
                self.clients.send_to_session(
                    session_id,
                    S2C::KillSession { session_id: session_id.clone() },
                );
            }
            task.state = TaskState::Hibernated;
            let id = task.id.clone();
            self.bus.emit(OrchestratorEvent::TaskStateChanged {
                task_id: id.clone(),
                old_state,
                new_state: TaskStateSummary::Hibernated,
            });
            self.registry.insert(task);
            self.save_state();
        }
    }

    async fn create_task(&self, profile: String, prompt: String, kind: TaskKind) {
        let task_id = TaskId::new();
        let name = format!("task-{}", &task_id.0[..8]);
        let session_id = Uuid::new_v4().to_string();
        let claude_session_id = Uuid::new_v4().to_string();

        // Insert the task as Running immediately so messages can be routed.
        let mut task = Task::new(
            task_id.clone(),
            name.clone(),
            profile.clone(),
            TaskState::Running { session_id: session_id.clone() },
            kind.clone(),
        );
        task.claude_session_id = Some(claude_session_id.clone());
        self.registry.insert(task);

        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Starting,
            trigger_message: None,
        });

        let (mcp_servers, disabled_mcp_servers) = self.mcp_session_args();
        let delivered = self.clients.send_to_any_client(S2C::StartSession {
            session_id,
            initial_prompt: if prompt.is_empty() { None } else { Some(prompt.clone()) },
            initial_files: vec![],
            extra_args: vec![],
            claude_session_id,
            is_resume: false,
            system_prompt: self.session_system_prompt(false),
            mcp_servers,
            disabled_mcp_servers,
            suppress_mcp_tools: vec![],
            mcp_extra_env: self.backend_mcp_extra_env(),
        });

        if !delivered {
            error!("orchestrator: no client daemon connected to handle new task {task_id}");
            self.registry.with_mut(&task_id, |t| t.state = TaskState::Dead);
            self.bus.emit(OrchestratorEvent::Error {
                task_id: Some(task_id.clone()),
                error: "No client daemon connected.".to_string(),
                next_steps: vec!["Start the claude-client daemon and ensure it is connected to the server.".to_string()],
                trigger_ref: None,
            });
            return;
        }

        let initial_prompt_opt = if prompt.is_empty() { None } else { Some(prompt) };
        self.bus.emit(OrchestratorEvent::TaskCreated { task_id, name, profile, kind, initial_prompt: initial_prompt_opt });
        self.save_state();
    }

    /// Returns the session_id if this task is currently Running, else None.
    fn running_session(&self, task_id: &TaskId) -> Option<String> {
        self.registry
            .with(task_id, |t| {
                if let TaskState::Running { ref session_id } = t.state {
                    Some(session_id.clone())
                } else {
                    None
                }
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_events::{BackendSource, MessageRef};
    use tokio::sync::broadcast;
    use tokio::time::{timeout, Duration};
    use claude_events::TaskKind;
    use crate::task_manager::{Task, TaskState};

    // ── helpers ─────────────────────────────────────────────────────────────

    fn make_orchestrator() -> (
        Arc<Orchestrator>,
        tokio::sync::mpsc::Sender<BackendEvent>,
        broadcast::Receiver<OrchestratorEvent>,
        Arc<TaskRegistry>,
    ) {
        let mut bus = EventBus::new();
        let backend_rx = bus.take_backend_receiver();
        let backend_tx = bus.backend_sender();
        let orch_rx = bus.subscribe_orchestrator();
        let bus = Arc::new(bus);
        let registry = Arc::new(TaskRegistry::new());
        let clients = Arc::new(ClientRegistry::new());
        let store = Arc::new(crate::persistence::StateStore::new(std::path::Path::new("/tmp")));
        let orch = Arc::new(Orchestrator::new(
            Arc::clone(&bus),
            Arc::clone(&registry),
            clients,
            OrchestratorConfig::default(),
            store,
        ));
        let orch_clone = Arc::clone(&orch);
        tokio::spawn(async move { orch_clone.run(backend_rx).await });
        (orch, backend_tx, orch_rx, registry)
    }

    fn make_task(name: &str) -> Task {
        Task::new(
            TaskId(format!("id-{name}")),
            name.to_string(),
            "test-profile".to_string(),
            TaskState::Hibernated,
            TaskKind::Job,
        )
    }

    fn msg_ref() -> MessageRef {
        MessageRef::new("test-backend", "msg-1")
    }

    fn source() -> BackendSource {
        BackendSource::new("test-backend", "user-1")
    }

    /// Collect up to `count` orchestrator events, waiting at most `ms` milliseconds total.
    async fn collect(
        rx: &mut broadcast::Receiver<OrchestratorEvent>,
        count: usize,
        ms: u64,
    ) -> Vec<OrchestratorEvent> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        let mut events = Vec::new();
        while events.len() < count {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match timeout(remaining, rx.recv()).await {
                Ok(Ok(e)) => events.push(e),
                _ => break,
            }
        }
        events
    }

    // ── tests ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn status_no_tasks() {
        let (_orch, tx, mut rx, _reg) = make_orchestrator();

        tx.send(BackendEvent::Command {
            command: ParsedCommand::Status,
            task_id: None,
            message_ref: msg_ref(),
            source: source(),
        })
        .await
        .unwrap();

        let events = collect(&mut rx, 1, 500).await;
        assert_eq!(events.len(), 1);
        let OrchestratorEvent::CommandResponse { text, .. } = &events[0] else {
            panic!("expected CommandResponse");
        };
        assert_eq!(text, "No active tasks.");
    }

    #[tokio::test]
    async fn status_with_task_shows_name() {
        let (_orch, tx, mut rx, registry) = make_orchestrator();
        registry.insert(make_task("my-task"));

        tx.send(BackendEvent::Command {
            command: ParsedCommand::Status,
            task_id: None,
            message_ref: msg_ref(),
            source: source(),
        })
        .await
        .unwrap();

        let events = collect(&mut rx, 1, 500).await;
        let OrchestratorEvent::CommandResponse { text, .. } = &events[0] else {
            panic!("expected CommandResponse");
        };
        assert!(text.contains("my-task"), "status text was: {text}");
    }

    #[tokio::test]
    async fn user_message_emits_acknowledged_then_responding() {
        let (_orch, tx, mut rx, registry) = make_orchestrator();
        let task_id = TaskId("id-mytask".to_string());
        registry.insert(make_task("mytask"));

        tx.send(BackendEvent::UserMessage {
            task_id,
            text: "hello claude".to_string(),
            message_ref: msg_ref(),
            source: source(),
        })
        .await
        .unwrap();

        let events = collect(&mut rx, 2, 500).await;
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            OrchestratorEvent::PhaseChanged { phase: SessionPhase::Acknowledged, .. }
        ));
        assert!(matches!(
            &events[1],
            OrchestratorEvent::PhaseChanged { phase: SessionPhase::Responding, .. }
        ));
    }

    #[tokio::test]
    async fn stop_removes_task_and_emits_state_change() {
        let (_orch, tx, mut rx, registry) = make_orchestrator();
        let task_id = TaskId("id-stoptask".to_string());
        registry.insert(make_task("stoptask"));

        tx.send(BackendEvent::Command {
            command: ParsedCommand::Stop {
                task_id: Some(task_id.clone()),
            },
            task_id: None,
            message_ref: msg_ref(),
            source: source(),
        })
        .await
        .unwrap();

        let events = collect(&mut rx, 1, 500).await;
        assert!(matches!(
            &events[0],
            OrchestratorEvent::TaskStateChanged {
                new_state: TaskStateSummary::Dead,
                ..
            }
        ));
        assert!(registry.with(&task_id, |_| ()).is_none(), "task should be removed");
    }

    #[tokio::test]
    async fn config_command_updates_show_thinking() {
        let (_orch, tx, mut rx, registry) = make_orchestrator();
        let task_id = TaskId("id-cfg".to_string());
        registry.insert(make_task("cfg"));

        tx.send(BackendEvent::Command {
            command: ParsedCommand::Config {
                key: "thinking".to_string(),
                value: "on".to_string(),
            },
            task_id: Some(task_id.clone()),
            message_ref: msg_ref(),
            source: source(),
        })
        .await
        .unwrap();

        collect(&mut rx, 1, 500).await; // drain CommandResponse

        let show_thinking = registry.with(&task_id, |t| t.config.show_thinking).unwrap();
        assert!(show_thinking);
    }
}
