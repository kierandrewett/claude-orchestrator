use claude_db::{Db, ScheduleMode};
use claude_events::{EventListEntry, TaskId, TaskKind, TaskStateSummary};
use claude_ndjson::UsageStats;

use crate::task_manager::TaskRegistry;

/// Format a cost/usage display string.
fn format_cost(usage: &UsageStats) -> String {
    format!(
        "${:.4} ({} in / {} out, {} turns)",
        usage.total_cost_usd, usage.input_tokens, usage.output_tokens, usage.turns
    )
}

/// Format a duration as "4h 20m 2s", omitting leading zero units.
fn format_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    match (h, m, s) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s}s"),
        (h, m, s) => format!("{h}h {m}m {s}s"),
    }
}

/// Build a /status response text.
pub fn build_status(registry: &TaskRegistry, idle_timeout_hours: u64, db: Option<&Db>) -> String {
    let ids = registry.all_ids();

    let now = chrono::Utc::now();
    let idle_threshold_secs = idle_timeout_hours * 3600;

    let mut lines: Vec<String> = Vec::new();

    if ids.is_empty() {
        lines.push("No active tasks.".to_string());
    } else {
        lines.push("Tasks:".to_string());
        for id in &ids {
            if let Some(line) = registry.with(id, |t| {
                let (emoji, suffix) = match t.state.summary() {
                    TaskStateSummary::Running => {
                        // Idle watchdog only applies to Job tasks; scratchpad hibernates
                        // immediately after each response so a countdown would be misleading.
                        let countdown = if t.kind == TaskKind::Job {
                            let elapsed = (now - t.last_activity).num_seconds().max(0) as u64;
                            let remaining = idle_threshold_secs.saturating_sub(elapsed);
                            format!(" — sleeps in {}", format_duration(remaining))
                        } else {
                            String::new()
                        };
                        ("🟢", countdown)
                    }
                    TaskStateSummary::Hibernated => ("💤", String::new()),
                    TaskStateSummary::Dead => ("💀", String::new()),
                };
                format!("{emoji} {} — {}{} ({})", t.name, t.profile, suffix, format_cost(&t.usage))
            }) {
                lines.push(line);
            }
        }
    }

    // Append scheduled events section if db is available and has events
    if let Some(db) = db {
        let events = db.list_events();
        if !events.is_empty() {
            lines.push(String::new());
            lines.push(build_events_list(db));
        }
    }

    lines.join("\n")
}

/// Build the scheduled events list display.
pub fn build_events_list(db: &Db) -> String {
    let events = db.list_events();
    if events.is_empty() {
        return "No scheduled events.".to_string();
    }

    let active_count = events.iter().filter(|e| e.enabled).count();
    let paused_count = events.len() - active_count;

    let mut lines = vec![format!("Scheduled Events ({active_count} active, {paused_count} paused)")];

    for event in &events {
        let status = if event.enabled { "✅" } else { "⏸️" };
        let mode = match event.mode {
            ScheduleMode::Recurring => "🔁",
            ScheduleMode::Once => "🔂",
        };
        let next = event.next_run
            .map(|t| {
                let now = chrono::Utc::now();
                let diff = t.signed_duration_since(now);
                if diff.num_seconds() < 0 {
                    "overdue".to_string()
                } else if diff.num_hours() < 24 {
                    format!("in {}h {}m", diff.num_hours(), diff.num_minutes() % 60)
                } else {
                    format!("in {}d", diff.num_days())
                }
            })
            .unwrap_or_else(|| if event.enabled { "not scheduled".to_string() } else { "paused".to_string() });

        let id_short = &event.id[..8.min(event.id.len())];
        lines.push(format!(
            "  {status} {mode} \"{}\"  — ({})  — next: {next}  — from: {} [{}]",
            event.name, event.schedule, event.origin_task_name, id_short
        ));
    }

    lines.join("\n")
}

/// Build detailed info for a single scheduled event.
pub fn build_events_info(db: &Db, event_id: &str) -> String {
    // Try exact match, then prefix match
    let event = db.get_event(event_id).or_else(|| {
        db.list_events().into_iter().find(|e| e.id.starts_with(event_id))
    });

    let event = match event {
        Some(e) => e,
        None => return format!("Event not found: {event_id}"),
    };

    let status = if event.enabled { "✅ enabled" } else { "⏸️ paused" };
    let mode = match event.mode {
        ScheduleMode::Recurring => "recurring 🔁",
        ScheduleMode::Once => "once 🔂",
    };
    let next = event.next_run
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "none".to_string());
    let last = event.last_run
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "never".to_string());

    let action_desc = match &event.action {
        claude_db::EventAction::SendMessage { task_id, message } =>
            format!("send_message to task {task_id}: {}", truncate(message, 80)),
        claude_db::EventAction::SendToScratchpad { message } =>
            format!("send_to_scratchpad: {}", truncate(message, 80)),
        claude_db::EventAction::PromptSession { task_id, prompt, wake_if_hibernating, skip_if_busy } =>
            format!("prompt_session task {task_id} (wake={wake_if_hibernating}, skip_busy={skip_if_busy}): {}", truncate(prompt, 80)),
    };

    let mut lines = vec![
        format!("Event: \"{}\" [{}]", event.name, event.id),
        format!("  Status: {status}"),
        format!("  Mode: {mode}"),
        format!("  Schedule: {}", event.schedule),
        format!("  Action: {action_desc}"),
        format!("  Next run: {next}"),
        format!("  Last run: {last}"),
        format!("  Failures: {}", event.consecutive_failures),
        format!("  Created by: {} ({})", event.origin_task_name, event.origin_task_id),
    ];
    if let Some(desc) = &event.description {
        lines.insert(1, format!("  Description: {desc}"));
    }

    // Show recent executions
    let executions = db.get_executions(&event.id, 5);
    if !executions.is_empty() {
        lines.push("  Recent runs:".to_string());
        for exec in &executions {
            let ts = exec.timestamp.format("%m-%d %H:%M").to_string();
            let detail = exec.detail.as_deref().unwrap_or("");
            lines.push(format!("    [{ts}] {:?} {detail}", exec.status));
        }
    }

    lines.join("\n")
}

/// Build structured event entries for the Telegram button UI.
pub fn build_events_list_entries(db: &Db) -> Vec<EventListEntry> {
    db.list_events()
        .into_iter()
        .map(|e| {
            let next_run = e.next_run.map(|t| {
                let local = t.with_timezone(&chrono::Local);
                let now_local = chrono::Local::now();
                let days = (local.date_naive() - now_local.date_naive()).num_days();
                let time = local.format("%H:%M").to_string();
                match days {
                    0 => format!("today {time}"),
                    1 => format!("tomorrow {time}"),
                    _ => local.format("%-d %b %H:%M").to_string(),
                }
            });
            EventListEntry {
                id: e.id,
                name: e.name,
                enabled: e.enabled,
                mode: e.mode.as_str().to_string(),
                schedule: e.schedule,
                next_run,
                origin_task_name: e.origin_task_name,
            }
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

/// Build a /cost response text for one or all tasks.
pub fn build_cost(
    registry: &TaskRegistry,
    all: bool,
    current_task_id: Option<&TaskId>,
) -> String {
    if all {
        let ids = registry.all_ids();
        let mut total = UsageStats::default();
        let mut lines = vec!["Cost by task:".to_string()];
        for id in &ids {
            if let Some(line) = registry.with(id, |t| {
                total.input_tokens += t.usage.input_tokens;
                total.output_tokens += t.usage.output_tokens;
                total.total_cost_usd += t.usage.total_cost_usd;
                total.turns += t.usage.turns;
                format!("  {} — {}", t.name, format_cost(&t.usage))
            }) {
                lines.push(line);
            }
        }
        lines.push(format!("Total: {}", format_cost(&total)));
        lines.join("\n")
    } else if let Some(id) = current_task_id {
        registry
            .with(id, |t| {
                format!("Cost for '{}': {}", t.name, format_cost(&t.usage))
            })
            .unwrap_or_else(|| "Unknown task.".to_string())
    } else {
        "No current task. Use /cost all to see all tasks.".to_string()
    }
}
