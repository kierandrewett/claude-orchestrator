use std::sync::Arc;

use chrono::{Local, Utc};
use cron::Schedule;
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use claude_db::{Db, EventAction, ExecutionStatus, ScheduleMode, ScheduledEvent};
use claude_events::{BackendEvent, BackendSource, EventBus, MessageRef, OrchestratorEvent, TaskId};

/// Starts the scheduler background task.
/// Returns a join handle.
pub fn start(
    db: Db,
    bus: Arc<EventBus>,
    backend_tx: mpsc::Sender<BackendEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("scheduler: started, tick every 30s");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            tick(&db, &bus, &backend_tx).await;
        }
    })
}

async fn tick(db: &Db, bus: &Arc<EventBus>, backend_tx: &mpsc::Sender<BackendEvent>) {
    let now = Utc::now();
    let due = db.get_events_due(now);
    if due.is_empty() {
        debug!("scheduler: tick — no events due");
        return;
    }
    info!("scheduler: {} event(s) due", due.len());

    for event in due {
        fire_event(db, bus, backend_tx, &event, now).await;
    }
}

async fn fire_event(
    db: &Db,
    bus: &Arc<EventBus>,
    backend_tx: &mpsc::Sender<BackendEvent>,
    event: &ScheduledEvent,
    fired_at: chrono::DateTime<Utc>,
) {
    info!("scheduler: firing event '{}' ({})", event.name, event.id);

    let (status, detail) = execute_action(db, bus, backend_tx, event).await;

    let consecutive_failures = if matches!(status, ExecutionStatus::TaskNotFound) {
        event.consecutive_failures + 1
    } else {
        0
    };

    // Auto-disable after 3 consecutive TaskNotFound
    let enabled = if consecutive_failures >= 3 {
        warn!("scheduler: auto-disabling event '{}' after 3 consecutive TaskNotFound", event.id);
        false
    } else if matches!(event.mode, ScheduleMode::Once) && matches!(status, ExecutionStatus::Success | ExecutionStatus::Skipped) {
        false
    } else {
        event.enabled
    };

    // Recalculate next_run for recurring events
    let next_run = if enabled && matches!(event.mode, ScheduleMode::Recurring) {
        calc_next_run(&event.schedule)
    } else {
        None
    };

    db.log_execution(&event.id, status, detail.as_deref());
    db.update_event_after_fire(&event.id, fired_at, next_run, enabled, consecutive_failures);

    // Emit EventsChanged so backends can refresh their event displays
    bus.emit(OrchestratorEvent::ScheduledEventFired {
        event_id: event.id.clone(),
        event_name: event.name.clone(),
    });
}

async fn execute_action(
    db: &Db,
    bus: &Arc<EventBus>,
    backend_tx: &mpsc::Sender<BackendEvent>,
    event: &ScheduledEvent,
) -> (ExecutionStatus, Option<String>) {
    match &event.action {
        EventAction::SendToScratchpad { message } => {
            // Resolve scratchpad task id
            let tasks = db.list_tasks();
            let scratchpad = tasks.iter().find(|t| t.task_id == "scratchpad");
            match scratchpad {
                None => {
                    (ExecutionStatus::TaskNotFound, Some("Scratchpad task not found".to_string()))
                }
                Some(_) => {
                    let body = format_scheduler_message(message, event);
                    bus.emit(OrchestratorEvent::SchedulerMessage {
                        task_id: TaskId("scratchpad".to_string()),
                        text: body,
                        event_id: event.id.clone(),
                        event_name: event.name.clone(),
                    });
                    (ExecutionStatus::Success, None)
                }
            }
        }

        EventAction::SendMessage { task_id, message } => {
            let exists = db.get_task(task_id).is_some();
            if !exists {
                return (ExecutionStatus::TaskNotFound, Some(format!("Task '{task_id}' not found")));
            }
            let body = format_scheduler_message(message, event);
            bus.emit(OrchestratorEvent::SchedulerMessage {
                task_id: TaskId(task_id.clone()),
                text: body,
                event_id: event.id.clone(),
                event_name: event.name.clone(),
            });
            (ExecutionStatus::Success, None)
        }

        EventAction::PromptSession { task_id, prompt, wake_if_hibernating, skip_if_busy } => {
            let task = match db.get_task(task_id) {
                None => return (ExecutionStatus::TaskNotFound, Some(format!("Task '{task_id}' not found"))),
                Some(t) => t,
            };

            match task.session_status.as_str() {
                "stopped" => {
                    return (ExecutionStatus::Skipped, Some("session stopped".to_string()));
                }
                "hibernated" if !wake_if_hibernating => {
                    return (ExecutionStatus::Skipped, Some("session hibernated".to_string()));
                }
                "running" if *skip_if_busy => {
                    return (ExecutionStatus::Skipped, Some("session busy".to_string()));
                }
                _ => {}
            }

            // Inject as a UserMessage via the event bus
            // Use a synthetic source so the orchestrator knows it came from the scheduler
            let source = BackendSource::new("scheduler", "scheduler");
            let msg_ref = MessageRef::new("scheduler", format!("event:{}", event.id));
            let _ = backend_tx.send(BackendEvent::UserMessage {
                task_id: TaskId(task_id.clone()),
                text: prompt.clone(),
                message_ref: msg_ref,
                source,
            }).await;
            (ExecutionStatus::Success, None)
        }
    }
}

fn format_scheduler_message(message: &str, event: &ScheduledEvent) -> String {
    format!("{}\n\n`scheduled: \"{}\" ({})`", message, event.name, event.schedule)
}

/// Calculate the next run time for a cron expression.
/// The `cron` crate uses 6-field format (sec min hour dom month dow),
/// so we prefix a "0 " to make standard 5-field expressions valid.
pub fn calc_next_run(schedule_expr: &str) -> Option<chrono::DateTime<Utc>> {
    // Try as-is first (in case it's already 6-field), then with "0 " prefix
    let schedule = Schedule::from_str(schedule_expr)
        .or_else(|_| Schedule::from_str(&format!("0 {schedule_expr}")))
        .ok()?;
    let local_now = Local::now();
    schedule
        .after(&local_now)
        .next()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn validate_cron(schedule_expr: &str) -> Result<(), String> {
    Schedule::from_str(schedule_expr)
        .or_else(|_| Schedule::from_str(&format!("0 {schedule_expr}")))
        .map(|_| ())
        .map_err(|e| format!("Invalid cron expression: {e}"))
}
