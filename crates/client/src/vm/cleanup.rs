//! Background container cleanup task.
//!
//! With the new design, Claude IS the main container process:
//!
//!   * **Running** containers → Claude is actively working; never touched.
//!   * **Stopped / exited** containers → Claude finished or crashed; removed
//!     after a 1-hour grace period (so a session can be resumed within an hour
//!     of the previous run ending).
//!
//! On client startup: stop (not remove) any running managed containers so
//! the client doesn't fight over their I/O.  Stopped containers are left
//! alone — they can be resumed normally when the session is accessed again.

use std::time::{Duration, SystemTime};

use tokio::process::Command;
use tracing::{info, warn};

const SWEEP_INTERVAL: Duration = Duration::from_secs(60);
/// How long a stopped container is kept before being removed.
const STOPPED_GRACE: Duration = Duration::from_secs(3600);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Called once at startup.
///
/// Stops any running managed containers (their I/O is no longer connected to
/// anything) but leaves stopped/exited containers in place — they can still
/// be resumed when the session is next requested.
pub async fn startup_sweep() {
    let running = match list_containers(&["status=running"]).await {
        Some(v) => v,
        None => return,
    };
    if running.is_empty() {
        return;
    }
    info!(
        "cleanup: startup sweep stopping {} running container(s)",
        running.len()
    );
    for name in running {
        info!("cleanup: stopping orphaned running container {name}");
        let _ = Command::new("docker").args(["stop", &name]).status().await;
    }
}

/// Runs forever, sweeping managed containers on each interval tick.
pub async fn run() {
    loop {
        tokio::time::sleep(SWEEP_INTERVAL).await;
        sweep_stopped_expired().await;
    }
}

// ---------------------------------------------------------------------------
// Sweep: stopped containers past the grace period
// ---------------------------------------------------------------------------

async fn sweep_stopped_expired() {
    let names = match list_containers(&["status=exited", "status=dead"]).await {
        Some(v) => v,
        None => return,
    };

    for name in names {
        match finished_at(&name).await {
            Some(finished) => {
                let age = SystemTime::now()
                    .duration_since(finished)
                    .unwrap_or(Duration::ZERO);
                if age >= STOPPED_GRACE {
                    info!(
                        "cleanup: container {name} stopped {:.0}m ago — removing",
                        age.as_secs_f64() / 60.0
                    );
                    remove_container(&name).await;
                }
            }
            None => {
                // Can't determine finish time — remove it to be safe.
                warn!("cleanup: container {name} has unknown finish time — removing");
                remove_container(&name).await;
            }
        }
    }
}

/// Returns the time the container finished, parsed from `docker inspect`.
async fn finished_at(name: &str) -> Option<SystemTime> {
    let out = Command::new("docker")
        .args(["inspect", "--format", "{{.State.FinishedAt}}", name])
        .output()
        .await
        .ok()?;

    if !out.status.success() {
        return None;
    }

    // Docker returns RFC 3339, e.g. "2024-01-15T10:30:00.123456789Z"
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();

    // Zero time means "never finished" (shouldn't happen for exited containers,
    // but guard against it).
    if s.starts_with("0001-01-01") {
        return None;
    }

    // Parse with chrono if available, otherwise fall back to a simple heuristic.
    parse_rfc3339(s)
}

fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    // Parse manually: "2024-01-15T10:30:00.123456789Z"
    // We only need second-level precision.
    let s = s.trim_end_matches('Z');
    let s = s.split('.').next().unwrap_or(s); // drop sub-seconds
    // Format: YYYY-MM-DDTHH:MM:SS
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<u32> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_parts: Vec<u32> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return None;
    }
    let (year, month, day) = (date_parts[0] as i32, date_parts[1], date_parts[2]);
    let (hour, min, sec) = (time_parts[0], time_parts[1], time_parts[2]);

    // Convert to Unix timestamp via days since epoch.
    let days = days_since_epoch(year, month, day)?;
    let secs = days as u64 * 86400 + hour as u64 * 3600 + min as u64 * 60 + sec as u64;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

fn days_since_epoch(year: i32, month: u32, day: u32) -> Option<i64> {
    // Compute days since 1970-01-01 using the proleptic Gregorian calendar.
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = month as i64;
    let d = day as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// List container names with `claude.managed=true` and any of the extra
/// filters supplied (each is passed as a separate `--filter` argument).
async fn list_containers(extra_filters: &[&str]) -> Option<Vec<String>> {
    let mut full_args: Vec<&str> = vec!["ps", "-a", "--filter", "label=claude.managed=true"];
    for f in extra_filters {
        full_args.push("--filter");
        full_args.push(f);
    }
    full_args.extend_from_slice(&["--format", "{{.Names}}"]);

    let out = match Command::new("docker").args(&full_args).output().await {
        Ok(o) => o,
        Err(e) => {
            warn!("cleanup: docker ps failed: {e}");
            return None;
        }
    };

    if !out.status.success() {
        warn!(
            "cleanup: docker ps returned error: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(
        stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
    )
}

async fn remove_container(name: &str) {
    match Command::new("docker")
        .args(["rm", "-f", name])
        .output()
        .await
    {
        Ok(o) if o.status.success() => info!("cleanup: removed {name}"),
        Ok(o) => warn!(
            "cleanup: failed to remove {name}: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => warn!("cleanup: docker rm {name}: {e}"),
    }
}
