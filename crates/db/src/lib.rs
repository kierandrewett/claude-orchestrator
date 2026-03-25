pub mod legacy;
pub mod migrate;

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    Once,
    Recurring,
}

impl ScheduleMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Recurring => "recurring",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "once" => Some(Self::Once),
            "recurring" => Some(Self::Recurring),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventAction {
    SendMessage {
        task_id: String,
        message: String,
    },
    SendToScratchpad {
        message: String,
    },
    PromptSession {
        task_id: String,
        prompt: String,
        #[serde(default)]
        wake_if_hibernating: bool,
        #[serde(default)]
        skip_if_busy: bool,
    },
}

impl EventAction {
    pub fn action_type(&self) -> &'static str {
        match self {
            Self::SendMessage { .. } => "send_message",
            Self::SendToScratchpad { .. } => "send_to_scratchpad",
            Self::PromptSession { .. } => "prompt_session",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventOrigin {
    pub task_id: String,
    pub task_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Success,
    Skipped,
    Failed,
    TaskNotFound,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::TaskNotFound => "task_not_found",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "skipped" => Some(Self::Skipped),
            "failed" => Some(Self::Failed),
            "task_not_found" => Some(Self::TaskNotFound),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventExecution {
    pub id: i64,
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub status: ExecutionStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub schedule: String,
    pub mode: ScheduleMode,
    pub action: EventAction,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub origin_task_id: String,
    pub origin_task_name: String,
    pub consecutive_failures: i32,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub task_id: String,
    pub task_name: String,
    pub session_id: Option<String>,
    pub session_status: String,
    pub created_at: String,
    pub last_activity: Option<String>,
}

// ── Db ────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (or create) the database at `data_dir/orchestrator.db` and run migrations.
    pub fn open(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("orchestrator.db");
        let conn = Connection::open(&db_path)?;
        migrate::run_migrations(&conn, data_dir)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    // ── Tasks ─────────────────────────────────────────────────────────────────

    pub fn list_tasks(&self) -> Vec<TaskRow> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT task_id, task_name, session_id, session_status, created_at, last_activity FROM tasks ORDER BY created_at ASC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(TaskRow {
                task_id: row.get(0)?,
                task_name: row.get(1)?,
                session_id: row.get(2)?,
                session_status: row.get(3)?,
                created_at: row.get(4)?,
                last_activity: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn search_tasks(&self, query: &str) -> Vec<TaskRow> {
        let conn = self.conn();
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare("SELECT task_id, task_name, session_id, session_status, created_at, last_activity FROM tasks WHERE task_name LIKE ?1 ORDER BY created_at ASC")
            .unwrap();
        stmt.query_map([&pattern], |row| {
            Ok(TaskRow {
                task_id: row.get(0)?,
                task_name: row.get(1)?,
                session_id: row.get(2)?,
                session_status: row.get(3)?,
                created_at: row.get(4)?,
                last_activity: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_task(&self, task_id: &str) -> Option<TaskRow> {
        let conn = self.conn();
        conn.query_row(
            "SELECT task_id, task_name, session_id, session_status, created_at, last_activity FROM tasks WHERE task_id = ?1",
            [task_id],
            |row| Ok(TaskRow {
                task_id: row.get(0)?,
                task_name: row.get(1)?,
                session_id: row.get(2)?,
                session_status: row.get(3)?,
                created_at: row.get(4)?,
                last_activity: row.get(5)?,
            }),
        ).ok()
    }

    pub fn upsert_task(&self, row: &TaskRow) {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tasks (task_id, task_name, session_id, session_status, created_at, last_activity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(task_id) DO UPDATE SET
               task_name = excluded.task_name,
               session_id = excluded.session_id,
               session_status = excluded.session_status,
               last_activity = excluded.last_activity",
            params![row.task_id, row.task_name, row.session_id, row.session_status, row.created_at, row.last_activity],
        ).ok();
    }

    pub fn delete_task(&self, task_id: &str) {
        let conn = self.conn();
        conn.execute("DELETE FROM tasks WHERE task_id = ?1", [task_id]).ok();
    }

    // ── Scheduled events ──────────────────────────────────────────────────────

    pub fn list_events(&self) -> Vec<ScheduledEvent> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, name, description, schedule, mode, action_type, action_data, enabled, created_at, last_run, next_run, origin_task_id, origin_task_name, consecutive_failures FROM scheduled_events ORDER BY created_at ASC")
            .unwrap();
        stmt.query_map([], |row| row_to_event(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn get_event(&self, event_id: &str) -> Option<ScheduledEvent> {
        let conn = self.conn();
        conn.query_row(
            "SELECT id, name, description, schedule, mode, action_type, action_data, enabled, created_at, last_run, next_run, origin_task_id, origin_task_name, consecutive_failures FROM scheduled_events WHERE id = ?1",
            [event_id],
            |row| row_to_event(row),
        ).ok()
    }

    pub fn get_events_due(&self, before: DateTime<Utc>) -> Vec<ScheduledEvent> {
        let conn = self.conn();
        let ts = before.to_rfc3339();
        let mut stmt = conn
            .prepare("SELECT id, name, description, schedule, mode, action_type, action_data, enabled, created_at, last_run, next_run, origin_task_id, origin_task_name, consecutive_failures FROM scheduled_events WHERE enabled = 1 AND next_run IS NOT NULL AND next_run <= ?1")
            .unwrap();
        stmt.query_map([&ts], |row| row_to_event(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn upsert_event(&self, event: &ScheduledEvent) {
        let conn = self.conn();
        let action_data = serde_json::to_string(&event.action).unwrap_or_default();
        conn.execute(
            "INSERT INTO scheduled_events
                (id, name, description, schedule, mode, action_type, action_data, enabled, created_at, last_run, next_run, origin_task_id, origin_task_name, consecutive_failures)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, description=excluded.description,
               schedule=excluded.schedule, mode=excluded.mode,
               action_type=excluded.action_type, action_data=excluded.action_data,
               enabled=excluded.enabled, last_run=excluded.last_run,
               next_run=excluded.next_run, consecutive_failures=excluded.consecutive_failures",
            params![
                event.id,
                event.name,
                event.description,
                event.schedule,
                event.mode.as_str(),
                event.action.action_type(),
                action_data,
                event.enabled as i32,
                event.created_at.to_rfc3339(),
                event.last_run.map(|t| t.to_rfc3339()),
                event.next_run.map(|t| t.to_rfc3339()),
                event.origin_task_id,
                event.origin_task_name,
                event.consecutive_failures,
            ],
        ).ok();
    }

    pub fn delete_event(&self, event_id: &str) {
        let conn = self.conn();
        conn.execute("DELETE FROM scheduled_events WHERE id = ?1", [event_id]).ok();
    }

    pub fn set_event_enabled(&self, event_id: &str, enabled: bool) {
        let conn = self.conn();
        conn.execute(
            "UPDATE scheduled_events SET enabled = ?1 WHERE id = ?2",
            params![enabled as i32, event_id],
        ).ok();
    }

    pub fn update_event_after_fire(
        &self,
        event_id: &str,
        last_run: DateTime<Utc>,
        next_run: Option<DateTime<Utc>>,
        enabled: bool,
        consecutive_failures: i32,
    ) {
        let conn = self.conn();
        conn.execute(
            "UPDATE scheduled_events SET last_run=?1, next_run=?2, enabled=?3, consecutive_failures=?4 WHERE id=?5",
            params![
                last_run.to_rfc3339(),
                next_run.map(|t| t.to_rfc3339()),
                enabled as i32,
                consecutive_failures,
                event_id,
            ],
        ).ok();
    }

    // ── Executions ────────────────────────────────────────────────────────────

    pub fn log_execution(&self, event_id: &str, status: ExecutionStatus, detail: Option<&str>) {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO event_executions (event_id, timestamp, status, detail) VALUES (?1, ?2, ?3, ?4)",
            params![
                event_id,
                Utc::now().to_rfc3339(),
                status.as_str(),
                detail,
            ],
        ).ok();
    }

    pub fn get_executions(&self, event_id: &str, limit: i64) -> Vec<EventExecution> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, event_id, timestamp, status, detail FROM event_executions WHERE event_id = ?1 ORDER BY timestamp DESC LIMIT ?2")
            .unwrap();
        stmt.query_map(params![event_id, limit], |row| {
            let status_str: String = row.get(3)?;
            Ok(EventExecution {
                id: row.get(0)?,
                event_id: row.get(1)?,
                timestamp: parse_dt(&row.get::<_, String>(2)?),
                status: ExecutionStatus::from_str(&status_str).unwrap_or(ExecutionStatus::Failed),
                detail: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<ScheduledEvent> {
    let mode_str: String = row.get(4)?;
    let action_data: String = row.get(6)?;
    let enabled: i32 = row.get(7)?;
    let created_at_str: String = row.get(8)?;
    let last_run_str: Option<String> = row.get(9)?;
    let next_run_str: Option<String> = row.get(10)?;

    Ok(ScheduledEvent {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        schedule: row.get(3)?,
        mode: ScheduleMode::from_str(&mode_str).unwrap_or(ScheduleMode::Recurring),
        action: serde_json::from_str(&action_data).unwrap_or(EventAction::SendToScratchpad { message: String::new() }),
        enabled: enabled != 0,
        created_at: parse_dt(&created_at_str),
        last_run: last_run_str.as_deref().map(parse_dt),
        next_run: next_run_str.as_deref().map(parse_dt),
        origin_task_id: row.get(11)?,
        origin_task_name: row.get(12)?,
        consecutive_failures: row.get(13)?,
    })
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

pub fn new_event_id() -> String {
    Uuid::new_v4().to_string()
}
