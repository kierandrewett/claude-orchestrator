use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use claude_containers::{ContainerHandle, SessionData};
use claude_events::{MessageRef, TaskId, TaskKind, TaskStateSummary, TaskSummary};
use claude_ndjson::UsageStats;

// ── TaskState ─────────────────────────────────────────────────────────────────

/// `ContainerHandle` contains `NdjsonTransport` which holds `Box<dyn AsyncRead/Write + Send>`
/// — these are `Send` but not `Sync`. We wrap in `Mutex` so `TaskState: Send + Sync`.
pub enum TaskState {
    Running(Mutex<ContainerHandle>),
    Hibernated(SessionData),
    Dead(SessionData),
}

impl TaskState {
    pub fn summary(&self) -> TaskStateSummary {
        match self {
            TaskState::Running(_) => TaskStateSummary::Running,
            TaskState::Hibernated(_) => TaskStateSummary::Hibernated,
            TaskState::Dead(_) => TaskStateSummary::Dead,
        }
    }
}

// ── TaskConfig ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TaskConfig {
    pub show_thinking: bool,
}

// ── QueuedInput ───────────────────────────────────────────────────────────────

pub struct QueuedInput {
    pub text: String,
    pub message_ref: Option<MessageRef>,
}

// ── Task ──────────────────────────────────────────────────────────────────────

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub profile: String,
    pub state: TaskState,
    pub usage: UsageStats,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub stdin_queue: VecDeque<QueuedInput>,
    pub kind: TaskKind,
    pub config: TaskConfig,
    /// True when Claude is not currently processing a turn.
    pub claude_idle: bool,
    pub current_trigger: Option<MessageRef>,
    /// Mapping of backend_name → channel/topic identifier.
    pub backend_channels: HashMap<String, String>,
}

impl Task {
    pub fn new(
        id: TaskId,
        name: String,
        profile: String,
        state: TaskState,
        kind: TaskKind,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            profile,
            state,
            usage: UsageStats::default(),
            created_at: now,
            last_activity: now,
            stdin_queue: VecDeque::new(),
            kind,
            config: TaskConfig::default(),
            claude_idle: true,
            current_trigger: None,
            backend_channels: HashMap::new(),
        }
    }

    pub fn summary(&self) -> TaskSummary {
        TaskSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            profile: self.profile.clone(),
            state: self.state.summary(),
            kind: self.kind.clone(),
        }
    }
}

// ── TaskRegistry ──────────────────────────────────────────────────────────────

pub struct TaskRegistry {
    tasks: DashMap<TaskId, Task>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            tasks: DashMap::new(),
        }
    }

    pub fn insert(&self, task: Task) {
        self.tasks.insert(task.id.clone(), task);
    }

    pub fn with<F, R>(&self, id: &TaskId, f: F) -> Option<R>
    where
        F: FnOnce(&Task) -> R,
    {
        self.tasks.get(id).map(|t| f(&t))
    }

    pub fn with_mut<F, R>(&self, id: &TaskId, f: F) -> Option<R>
    where
        F: FnOnce(&mut Task) -> R,
    {
        self.tasks.get_mut(id).map(|mut t| f(&mut t))
    }

    pub fn remove(&self, id: &TaskId) -> Option<Task> {
        self.tasks.remove(id).map(|(_, t)| t)
    }

    pub fn all_ids(&self) -> Vec<TaskId> {
        self.tasks.iter().map(|r| r.key().clone()).collect()
    }

    pub fn all_summaries(&self) -> Vec<TaskSummary> {
        self.tasks.iter().map(|r| r.summary()).collect()
    }

    /// Fuzzy-match a hint string against task names. Returns the matching task
    /// ID if exactly one task matches, or an error if ambiguous/not found.
    pub fn resolve_hint(&self, hint: &str) -> anyhow::Result<TaskId> {
        let lower = hint.to_lowercase();
        let matches: Vec<TaskId> = self
            .tasks
            .iter()
            .filter(|r| r.name.to_lowercase().contains(&lower))
            .map(|r| r.key().clone())
            .collect();

        match matches.len() {
            0 => anyhow::bail!("no task matching '{hint}'"),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => anyhow::bail!("ambiguous task hint '{hint}' — be more specific"),
        }
    }

    /// Drain the stdin queue for a task and concatenate all messages.
    /// Returns `None` if the queue is empty.
    pub fn drain_queue(&self, id: &TaskId) -> Option<(String, Vec<MessageRef>)> {
        let mut combined = String::new();
        let mut refs = Vec::new();

        if let Some(mut task) = self.tasks.get_mut(id) {
            if task.stdin_queue.is_empty() {
                return None;
            }
            let mut first = true;
            while let Some(item) = task.stdin_queue.pop_front() {
                if !first {
                    combined.push_str("\n---\n");
                }
                combined.push_str(&item.text);
                if let Some(r) = item.message_ref {
                    refs.push(r);
                }
                first = false;
            }
        }

        if combined.is_empty() {
            None
        } else {
            Some((combined, refs))
        }
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}
