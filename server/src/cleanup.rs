//! Background session cleanup task.
//!
//! Runs every 5 minutes and:
//! - Removes ended sessions (Completed/Failed/Killed) from memory after 2 hours
//! - Removes Pending sessions stuck for > 10 minutes (StartSession was never received)
//! - Kills Running sessions idle for > 4 hours (sends KillSession to client)

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

use crate::{
    protocol::{S2C, SessionStatus},
    state::AppState,
};

const CLEANUP_INTERVAL_SECS: u64 = 300; // 5 minutes
const ENDED_RETENTION_SECS: i64 = 2 * 60 * 60; // 2 hours
const STUCK_PENDING_SECS: i64 = 10 * 60; // 10 minutes
const MAX_RUNNING_SECS: i64 = 4 * 60 * 60; // 4 hours

pub async fn run(app_state: Arc<AppState>) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(CLEANUP_INTERVAL_SECS)).await;
        sweep(&app_state).await;
    }
}

async fn sweep(app_state: &Arc<AppState>) {
    let now = Utc::now();
    let mut to_remove: Vec<String> = Vec::new();
    let mut to_kill: Vec<String> = Vec::new();

    {
        let sessions = app_state.sessions.read().await;
        for (id, buf) in sessions.iter() {
            match buf.info.status {
                // Ended sessions: evict from memory after retention period
                SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Killed => {
                    let age = buf
                        .info
                        .ended_at
                        .map(|t| (now - t).num_seconds())
                        .unwrap_or(ENDED_RETENTION_SECS + 1);
                    if age > ENDED_RETENTION_SECS {
                        to_remove.push(id.clone());
                    }
                }

                // Pending sessions stuck waiting (client never received StartSession)
                SessionStatus::Pending => {
                    let age = (now - buf.info.created_at).num_seconds();
                    if age > STUCK_PENDING_SECS {
                        to_remove.push(id.clone());
                    }
                }

                // Running sessions that have been going too long
                SessionStatus::Running => {
                    let age = buf
                        .info
                        .started_at
                        .map(|t| (now - t).num_seconds())
                        .unwrap_or(0);
                    if age > MAX_RUNNING_SECS {
                        to_kill.push(id.clone());
                    }
                }
            }
        }
    }

    for id in &to_kill {
        info!("cleanup: killing idle session {id}");
        app_state
            .send_to_client(&S2C::KillSession {
                session_id: id.clone(),
            })
            .await;
    }

    if !to_remove.is_empty() {
        let mut sessions = app_state.sessions.write().await;
        for id in &to_remove {
            sessions.remove(id);
            info!("cleanup: evicted session {id} from memory");
        }
    }

    if !to_remove.is_empty() || !to_kill.is_empty() {
        info!(
            "cleanup: removed {} session(s), killed {} session(s)",
            to_remove.len(),
            to_kill.len()
        );
    }
}
