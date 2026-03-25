use claude_events::{TaskId, TaskKind, TaskStateSummary};
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
pub fn build_status(registry: &TaskRegistry, idle_timeout_hours: u64) -> String {
    let ids = registry.all_ids();
    if ids.is_empty() {
        return "No active tasks.".to_string();
    }

    let now = chrono::Utc::now();
    let idle_threshold_secs = idle_timeout_hours * 3600;

    let mut lines = vec!["Tasks:".to_string()];
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
    lines.join("\n")
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
