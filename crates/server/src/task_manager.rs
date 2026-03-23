use std::sync::Mutex;

use chrono::{DateTime, Utc};
use dashmap::DashMap;

use claude_containers::ContainerHandle;
use claude_events::{TaskId, TaskKind, TaskStateSummary};
use claude_ndjson::UsageStats;

// ── TaskState ─────────────────────────────────────────────────────────────────

/// `ContainerHandle` contains `NdjsonTransport` which holds `Box<dyn AsyncRead/Write + Send>`
/// — these are `Send` but not `Sync`. We wrap in `Mutex` so `TaskState: Send + Sync`.
pub enum TaskState {
    Running(Box<Mutex<ContainerHandle>>),
    Hibernated,
    Dead,
}

impl TaskState {
    pub fn summary(&self) -> TaskStateSummary {
        match self {
            TaskState::Running(_) => TaskStateSummary::Running,
            TaskState::Hibernated => TaskStateSummary::Hibernated,
            TaskState::Dead => TaskStateSummary::Dead,
        }
    }
}

// ── TaskConfig ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TaskConfig {
    pub show_thinking: bool,
}

// ── Task ──────────────────────────────────────────────────────────────────────

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub profile: String,
    pub state: TaskState,
    pub usage: UsageStats,
    pub last_activity: DateTime<Utc>,
    pub kind: TaskKind,
    pub config: TaskConfig,
    /// True when Claude is not currently processing a turn.
    pub claude_idle: bool,
    pub current_trigger: Option<claude_events::MessageRef>,
}

impl Task {
    pub fn new(
        id: TaskId,
        name: String,
        profile: String,
        state: TaskState,
        kind: TaskKind,
    ) -> Self {
        Self {
            id,
            name,
            profile,
            state,
            usage: UsageStats::default(),
            last_activity: Utc::now(),
            kind,
            config: TaskConfig::default(),
            claude_idle: true,
            current_trigger: None,
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
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_events::TaskId;

    fn make_task(name: &str) -> Task {
        Task::new(
            TaskId(format!("id-{name}")),
            name.to_string(),
            "test".to_string(),
            TaskState::Hibernated,
            TaskKind::Job,
        )
    }

    #[test]
    fn insert_and_retrieve() {
        let reg = TaskRegistry::new();
        let id = TaskId("id-foo".to_string());
        reg.insert(make_task("foo"));
        let name = reg.with(&id, |t| t.name.clone()).unwrap();
        assert_eq!(name, "foo");
    }

    #[test]
    fn remove_returns_task() {
        let reg = TaskRegistry::new();
        let id = TaskId("id-bar".to_string());
        reg.insert(make_task("bar"));
        let task = reg.remove(&id).unwrap();
        assert_eq!(task.name, "bar");
        assert!(reg.with(&id, |_| ()).is_none());
    }

    #[test]
    fn all_ids_empty_then_populated() {
        let reg = TaskRegistry::new();
        assert!(reg.all_ids().is_empty());
        reg.insert(make_task("a"));
        reg.insert(make_task("b"));
        let mut ids: Vec<String> = reg.all_ids().into_iter().map(|id| id.0).collect();
        ids.sort();
        assert_eq!(ids, ["id-a", "id-b"]);
    }

    #[test]
    fn with_mut_updates_field() {
        let reg = TaskRegistry::new();
        let id = TaskId("id-c".to_string());
        reg.insert(make_task("c"));
        reg.with_mut(&id, |t| t.claude_idle = false);
        let idle = reg.with(&id, |t| t.claude_idle).unwrap();
        assert!(!idle);
    }
}
