//! Background container cleanup task.
//!
//! Runs every 60 seconds and removes managed Docker containers that no longer
//! have an active Claude process:
//!
//!   * **Running** containers where `docker top` shows no `claude` process are
//!     stopped and removed — the session has ended but the `sleep infinity`
//!     sentinel is still running.
//!
//!   * **Stopped / exited** containers are removed immediately — they either
//!     crashed or were left behind by a previous client run.
//!
//! Containers where `claude` is actively running are never touched.

use tokio::process::Command;
use tracing::{info, warn};

const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Called once at startup: removes every managed container unconditionally.
///
/// After a client restart the session map is empty, so any container left
/// over from the previous run is orphaned — there is no live session that
/// could be using it.
pub async fn startup_sweep() {
    let names = match list_containers(&[]).await {
        Some(v) => v,
        None => return,
    };
    if names.is_empty() {
        return;
    }
    info!("cleanup: startup sweep removing {} orphaned container(s)", names.len());
    for name in names {
        remove_container(&name).await;
    }
}

/// Runs forever, sweeping managed containers on each interval tick.
pub async fn run() {
    loop {
        tokio::time::sleep(SWEEP_INTERVAL).await;
        sweep().await;
    }
}

async fn sweep() {
    sweep_running_idle().await;
    sweep_stopped().await;
}

// ---------------------------------------------------------------------------
// Sweep: running containers with no active claude process
// ---------------------------------------------------------------------------

async fn sweep_running_idle() {
    let names = match list_containers(&["status=running"]).await {
        Some(v) => v,
        None => return,
    };

    for name in names {
        if claude_running_in(&name).await {
            // Claude is actively working — leave it alone.
            continue;
        }
        info!("cleanup: container {name} has no active claude process — removing");
        remove_container(&name).await;
    }
}

/// Returns `true` if a process whose command contains "claude" is running
/// inside the container.
async fn claude_running_in(name: &str) -> bool {
    let out = match Command::new("docker")
        .args(["top", name])
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    String::from_utf8_lossy(&out.stdout).contains("claude")
}

// ---------------------------------------------------------------------------
// Sweep: stopped / exited containers
// ---------------------------------------------------------------------------

async fn sweep_stopped() {
    let names = match list_containers(&["status=exited", "status=dead"]).await {
        Some(v) => v,
        None => return,
    };

    for name in names {
        info!("cleanup: removing stopped container {name}");
        remove_container(&name).await;
    }
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
