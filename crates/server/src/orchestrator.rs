use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, error, info};

use claude_containers::{ContainerConfig, ContainerManager};
use claude_events::{
    BackendEvent, EventBus, MessageRef, OrchestratorEvent, ParsedCommand, SessionPhase, TaskId,
    TaskKind, TaskStateSummary,
};
use claude_ndjson::CoalescedEvent;
use tokio::sync::mpsc;

use crate::commands::{build_cost, build_status};
use crate::config::OrchestratorConfig;
use crate::persistence::StateStore;
use crate::task_manager::{QueuedInput, Task, TaskRegistry, TaskState};

pub struct Orchestrator {
    pub bus: Arc<EventBus>,
    pub registry: Arc<TaskRegistry>,
    pub containers: Arc<ContainerManager>,
    pub store: Arc<StateStore>,
    pub config: OrchestratorConfig,
}

impl Orchestrator {
    pub fn new(
        bus: Arc<EventBus>,
        registry: Arc<TaskRegistry>,
        containers: Arc<ContainerManager>,
        store: Arc<StateStore>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            bus,
            registry,
            containers,
            store,
            config,
        }
    }

    /// Run the main orchestrator loop.
    pub async fn run(self: Arc<Self>, mut backend_rx: mpsc::Receiver<BackendEvent>) {
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
        }
    }

    async fn handle_user_message(
        &self,
        task_id: &TaskId,
        text: String,
        msg_ref: Option<MessageRef>,
    ) {
        // Mark acknowledged.
        self.bus.emit(OrchestratorEvent::PhaseChanged {
            task_id: task_id.clone(),
            phase: SessionPhase::Acknowledged,
            trigger_message: msg_ref.clone(),
        });

        let is_idle = self
            .registry
            .with(task_id, |t| t.claude_idle)
            .unwrap_or(false);

        if is_idle {
            // Send directly.
            self.send_to_container(task_id, text, msg_ref).await;
        } else {
            // Queue it.
            self.registry.with_mut(task_id, |t| {
                t.stdin_queue.push_back(QueuedInput {
                    text,
                    message_ref: msg_ref,
                });
            });
        }
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
        // Take the task out of the registry, hibernate, put back.
        if let Some(mut task) = self.registry.remove(task_id) {
            if let TaskState::Running(handle_mutex) = task.state {
                let handle = handle_mutex.into_inner().unwrap_or_else(|e| e.into_inner());
                match self.containers.hibernate(handle).await {
                    Ok(session_data) => {
                        task.state = TaskState::Hibernated(session_data);
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
                        task.state = TaskState::Dead(claude_containers::SessionData::new(
                            String::new(),
                            String::new(),
                            ContainerConfig::default(),
                        ));
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
                    TaskState::Running(std::sync::Mutex::new(handle)),
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

    #[allow(dead_code)]
    async fn handle_claude_event(
        &self,
        _task_id: &TaskId,
        _event: Result<Option<CoalescedEvent>>,
    ) {
        // Placeholder — real implementation polls the CoalescedStream
    }
}
