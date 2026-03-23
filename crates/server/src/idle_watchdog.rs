use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};

use claude_events::{EventBus, OrchestratorEvent, TaskKind, TaskStateSummary};

use crate::task_manager::{TaskRegistry, TaskState};

const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60); // 5 minutes

pub async fn run(registry: Arc<TaskRegistry>, bus: Arc<EventBus>, idle_hours: u64) {
    let idle_threshold = Duration::from_secs(idle_hours * 3600);

    loop {
        tokio::time::sleep(CHECK_INTERVAL).await;

        let now = Utc::now();
        let ids = registry.all_ids();

        for id in ids {
            let should_hibernate = registry.with(&id, |t| {
                // Skip Scratchpad and non-running tasks.
                if t.kind == TaskKind::Scratchpad {
                    return false;
                }
                if !matches!(t.state, TaskState::Running(_)) {
                    return false;
                }
                let idle = (now - t.last_activity).to_std().unwrap_or(Duration::ZERO);
                idle >= idle_threshold
            });

            if should_hibernate.unwrap_or(false) {
                warn!("idle-watchdog: task {id} has been idle, requesting hibernate");
                bus.emit(OrchestratorEvent::TaskStateChanged {
                    task_id: id.clone(),
                    old_state: TaskStateSummary::Running,
                    new_state: TaskStateSummary::Hibernated,
                });
                // The actual hibernate is handled by the orchestrator main loop
                // when it sees this event (for now we signal intent via a sentinel).
                // In the full implementation, this would call into orchestrator state.
                info!("idle-watchdog: emitted hibernate request for {id}");
            }
        }
    }
}
