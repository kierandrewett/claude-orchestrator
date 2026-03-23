use std::sync::Arc;

use tracing::{debug, error, info};

use claude_containers::{ContainerConfig, ContainerManager};
use claude_events::{
    BackendEvent, EventBus, MessageRef, OrchestratorEvent, ParsedCommand, SessionPhase, TaskId,
    TaskKind, TaskStateSummary,
};

use crate::commands::{build_cost, build_status};
use crate::config::OrchestratorConfig;
use crate::task_manager::{Task, TaskRegistry, TaskState};

pub struct Orchestrator {
    pub bus: Arc<EventBus>,
    pub registry: Arc<TaskRegistry>,
    pub containers: Arc<ContainerManager>,
    pub config: OrchestratorConfig,
}

impl Orchestrator {
    pub fn new(
        bus: Arc<EventBus>,
        registry: Arc<TaskRegistry>,
        containers: Arc<ContainerManager>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            bus,
            registry,
            containers,
            config,
        }
    }

    /// Run the main orchestrator loop.
    pub async fn run(self: Arc<Self>, mut backend_rx: tokio::sync::mpsc::Receiver<BackendEvent>) {
        info!("orchestrator: starting main loop");

        loop {
            // Collect all running task IDs.
            let running_ids: Vec<TaskId> = self
                .registry
                .all_ids()
                .into_iter()
                .filter(|id| {
                    self.registry
                        .with(id, |t| matches!(t.state, TaskState::Running(_)))
                        .unwrap_or(false)
                })
                .collect();

            // Poll one event from the backend channel (non-blocking).
            if let Ok(event) = backend_rx.try_recv() {
                self.handle_backend_event(event).await;
                continue;
            }

            // No running tasks and no backend events — wait for a backend event.
            if running_ids.is_empty() {
                match backend_rx.recv().await {
                    Some(event) => self.handle_backend_event(event).await,
                    None => {
                        info!("orchestrator: backend channel closed, shutting down");
                        break;
                    }
                }
                continue;
            }

            // No work — wait for a backend event.
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
            BackendEvent::UserMessage {
                task_id,
                text,
                message_ref,
                ..
            } => {
                self.handle_user_message(&task_id, text, Some(message_ref))
                    .await;
            }
            BackendEvent::Command {
                command,
                task_id,
                message_ref,
                ..
            } => {
                self.handle_command(command, task_id, message_ref).await;
            }
            BackendEvent::FileUpload {
                task_id,
                filename,
                mime_type,
                caption,
                message_ref,
                ..
            } => {
                let text = format!(
                    "A file was uploaded: {} ({})\n{}",
                    filename,
                    mime_type.as_deref().unwrap_or("unknown"),
                    caption.as_deref().unwrap_or("")
                );
                self.handle_user_message(&task_id, text, Some(message_ref))
                    .await;
            }
            BackendEvent::InterruptTask { task_id, .. } => {
                self.interrupt_task(&task_id).await;
            }
        }
    }

    async fn interrupt_task(&self, task_id: &TaskId) {
        info!("orchestrator: interrupting task {task_id}");
        // TODO: route S2C::InterruptSession to the client holding this task
        // once client session routing is wired up.
        self.bus.emit(OrchestratorEvent::CommandResponse {
            task_id: Some(task_id.clone()),
            text: "⏹ Interrupting…".to_string(),
        });
    }

    async fn handle_user_message(
        &self,
        task_id: &TaskId,
        text: String,
        msg_ref: Option<MessageRef>,
    ) {
        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Acknowledged,
            trigger_message: msg_ref.clone(),
        });

        self.send_to_container(task_id, text, msg_ref).await;
    }

    async fn send_to_container(
        &self,
        task_id: &TaskId,
        text: String,
        msg_ref: Option<MessageRef>,
    ) {
        self.registry.with_mut(task_id, |t| {
            t.claude_idle = false;
            t.current_trigger = msg_ref;
            t.last_activity = chrono::Utc::now();
        });

        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Responding,
            trigger_message: None,
        });

        // The actual stdin write happens via the container handle.
        // In the full implementation, this would write to the transport.
        debug!("orchestrator: would send to {task_id}: {text:?}");
    }

    async fn handle_command(
        &self,
        command: ParsedCommand,
        task_id: Option<TaskId>,
        _msg_ref: MessageRef,
    ) {
        match command {
            ParsedCommand::Status => {
                let text = build_status(&self.registry);
                self.bus.emit(OrchestratorEvent::CommandResponse {
                    task_id: task_id.clone(),
                    text,
                });
            }
            ParsedCommand::Cost { all } => {
                let text = build_cost(&self.registry, all, task_id.as_ref());
                self.bus
                    .emit(OrchestratorEvent::CommandResponse { task_id, text });
            }
            ParsedCommand::ProfileList => {
                let profiles = claude_containers::load_profiles(
                    &std::path::PathBuf::from("docker/profiles"),
                )
                .unwrap_or_default();
                let names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
                self.bus.emit(OrchestratorEvent::CommandResponse {
                    task_id,
                    text: format!("Available profiles: {}", names.join(", ")),
                });
            }
            ParsedCommand::Config { key, value } => {
                if let Some(id) = &task_id {
                    self.registry.with_mut(id, |t| {
                        if key == "thinking" {
                            t.config.show_thinking = value == "on" || value == "true";
                        }
                    });
                    self.bus.emit(OrchestratorEvent::CommandResponse {
                        task_id,
                        text: format!("Config updated: {key} = {value}"),
                    });
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
            ParsedCommand::SlashCommands => {
                let containers = Arc::clone(&self.containers);
                let bus = self.bus.clone();
                tokio::spawn(async move {
                    let image = "orchestrator/claude-code:base";
                    match containers.discover_slash_commands(image).await {
                        Ok(cmds) => {
                            let text = if cmds.is_empty() {
                                "No slash commands found.".to_string()
                            } else {
                                cmds.iter()
                                    .map(|c| {
                                        if c.description.is_empty() {
                                            c.name.clone()
                                        } else {
                                            format!("{} — {}", c.name, c.description)
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            };
                            bus.emit(OrchestratorEvent::CommandResponse { task_id, text });
                        }
                        Err(e) => {
                            bus.emit(OrchestratorEvent::Error {
                                task_id,
                                error: format!("slash command discovery failed: {e}"),
                                next_steps: vec![],
                            });
                        }
                    }
                });
            }
        }
    }

    async fn stop_task(&self, task_id: &TaskId) {
        let old_state = self.registry.with(task_id, |t| t.state.summary());
        if let Some(old) = old_state {
            if let Some(task) = self.registry.remove(task_id) {
                if let TaskState::Running(handle_mutex) = task.state {
                    let handle = handle_mutex.into_inner().unwrap_or_else(|e| e.into_inner());
                    let _ = self.containers.destroy(&handle.container_id).await;
                }
            }
            self.bus.emit(OrchestratorEvent::TaskStateChanged {
                task_id: task_id.clone(),
                old_state: old,
                new_state: TaskStateSummary::Dead,
            });
        }
    }

    pub async fn hibernate_task(&self, task_id: &TaskId) {
        if let Some(mut task) = self.registry.remove(task_id) {
            if let TaskState::Running(handle_mutex) = task.state {
                let handle = handle_mutex.into_inner().unwrap_or_else(|e| e.into_inner());
                match self.containers.hibernate(handle).await {
                    Ok(_session_data) => {
                        task.state = TaskState::Hibernated;
                        let id = task.id.clone();
                        self.bus.emit(OrchestratorEvent::TaskStateChanged {
                            task_id: id.clone(),
                            old_state: TaskStateSummary::Running,
                            new_state: TaskStateSummary::Hibernated,
                        });
                        self.registry.insert(task);
                    }
                    Err(e) => {
                        error!("failed to hibernate {task_id}: {e}");
                        task.state = TaskState::Dead;
                        self.registry.insert(task);
                    }
                }
            } else {
                // Not running — put it back unchanged.
                self.registry.insert(task);
            }
        }
    }

    async fn create_task(&self, profile: String, prompt: String, kind: TaskKind) {
        let task_id = TaskId::new();
        let name = format!("task-{}", &task_id.0[..8]);

        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Starting,
            trigger_message: None,
        });

        // Build container config from profile.
        let config = ContainerConfig {
            image: format!(
                "{}/{}:{}",
                self.config.docker.image_prefix, profile, "latest"
            ),
            ..Default::default()
        };

        match self
            .containers
            .spawn(config, Some(prompt), None)
            .await
        {
            Ok(handle) => {
                let task = Task::new(
                    task_id.clone(),
                    name.clone(),
                    profile.clone(),
                    TaskState::Running(Box::new(std::sync::Mutex::new(handle))),
                    kind.clone(),
                );
                self.registry.insert(task);
                self.bus.emit(OrchestratorEvent::TaskCreated {
                    task_id,
                    name,
                    profile,
                    kind,
                });
            }
            Err(e) => {
                error!("failed to create task: {e}");
                self.bus.emit(OrchestratorEvent::Error {
                    task_id: Some(task_id),
                    error: format!("Failed to start container: {e}"),
                    next_steps: vec![
                        "Check Docker is running".to_string(),
                        "Run `claude-orchestrator setup` to rebuild images".to_string(),
                    ],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_containers::AuthManager;
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
        // bollard::Docker::connect_with_socket_defaults() doesn't open a connection
        // until the first request, so this succeeds even when Docker isn't running.
        let containers = Arc::new(
            ContainerManager::new(
                "unix:///var/run/docker.sock",
                AuthManager::new("/tmp/test-auth".into()),
                "/tmp".into(),
            )
            .unwrap(),
        );
        let orch = Arc::new(Orchestrator::new(
            Arc::clone(&bus),
            Arc::clone(&registry),
            containers,
            OrchestratorConfig::default(),
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
