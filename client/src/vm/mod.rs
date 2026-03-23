//! Docker container session runner.
//!
//! Each session runs inside a named Docker container:
//!
//!   claude-<claude_session_id>
//!
//! The container is created once with `docker run -d sleep infinity` and then
//! `docker exec -i` is used each time to run Claude inside it.  This means:
//!
//!   * Container filesystem state persists across session resumes.
//!   * The same container is reused when a session is resumed (same
//!     `claude_session_id`).
//!   * The `sleep infinity` sentinel keeps the container alive between execs.
//!
//! A background cleanup task (see `cleanup`) removes containers that no longer
//! have an active Claude process.

pub mod cleanup;
pub mod config;
pub mod rootfs;

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::protocol::{C2S, S2C, SessionStats};
use crate::session_runner::{update_stats, ws_send, WsSink};
use config::VmConfig;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run_vm_session(
    config: crate::session_runner::SessionConfig,
    ws_tx: Arc<Mutex<WsSink>>,
    mut cmd_rx: mpsc::Receiver<S2C>,
    vm_cfg: VmConfig,
) {
    let session_id = config.session_id.clone();
    info!("docker: starting session {session_id}");

    let result = do_run(&config, &ws_tx, &mut cmd_rx, &vm_cfg).await;

    if let Err(e) = result {
        warn!("docker: session {session_id} error: {e:#}");
        ws_send(
            &ws_tx,
            &C2S::SessionEnded {
                session_id,
                exit_code: -1,
                stats: SessionStats::default(),
                error: Some(e.to_string()),
            },
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Inner implementation
// ---------------------------------------------------------------------------

/// Canonical container name for a Claude session.
pub fn container_name(claude_session_id: &str) -> String {
    format!("claude-{claude_session_id}")
}

async fn do_run(
    config: &crate::session_runner::SessionConfig,
    ws_tx: &Arc<Mutex<WsSink>>,
    cmd_rx: &mut mpsc::Receiver<S2C>,
    vm_cfg: &VmConfig,
) -> Result<()> {
    let session_id = &config.session_id;
    let name = container_name(&config.claude_session_id);

    // Ensure the container exists and is running.
    ensure_container(&name, config, vm_cfg).await?;

    // docker exec -i <container> claude --print ...
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-i"]);

    // Working directory for this exec (may differ from container default).
    cmd.args(["-w", &config.default_cwd]);

    // Pass the current API key through to the exec'd process.
    cmd.args(["-e", "ANTHROPIC_API_KEY"]);

    cmd.arg(&name);

    cmd.args([
        "claude",
        "--print",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--dangerously-skip-permissions",
    ]);

    if config.is_resume {
        cmd.args(["--resume", &config.claude_session_id]);
    } else {
        cmd.args(["--session-id", &config.claude_session_id]);
    }

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("spawn docker exec")?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");

    // Send the initial prompt before anything else.
    if let Some(ref prompt) = config.initial_prompt {
        let line = format_user_message(prompt);
        stdin
            .write_all(line.as_bytes())
            .await
            .context("write initial prompt")?;
    }

    ws_send(
        ws_tx,
        &C2S::SessionStarted {
            session_id: session_id.clone(),
            pid: child.id().unwrap_or(0),
            cwd: config.default_cwd.clone(),
        },
    )
    .await;

    info!("docker: exec claude in container {name}");

    let mut stats = SessionStats::default();
    let mut stdout_lines = BufReader::new(stdout).lines();

    // Kill the session if no stdin has been received for this long.
    const IDLE_TIMEOUT: Duration = Duration::from_secs(12 * 3600);
    let idle_sleep = tokio::time::sleep(IDLE_TIMEOUT);
    tokio::pin!(idle_sleep);

    loop {
        tokio::select! {
            biased;

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(S2C::SendInput { text, .. }) => {
                        idle_sleep.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                        let line = format_user_message(&text);
                        if let Err(e) = stdin.write_all(line.as_bytes()).await {
                            warn!("docker: stdin write error: {e}");
                            break;
                        }
                    }
                    Some(S2C::SendInputWithFiles { text, files, .. }) => {
                        idle_sleep.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                        let formatted = crate::session_runner::format_user_message_with_files(&text, &files).await;
                        if !formatted.is_empty() {
                            if let Err(e) = stdin.write_all(formatted.as_bytes()).await {
                                warn!("docker: stdin write error: {e}");
                                break;
                            }
                        }
                    }
                    Some(S2C::KillSession { .. }) => {
                        info!("docker: KillSession — stopping exec");
                        let _ = child.kill().await;
                        break;
                    }
                    Some(_) | None => break,
                }
            }

            line_result = stdout_lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(event) => {
                                update_stats(&mut stats, &event);
                                ws_send(ws_tx, &C2S::SessionEvent {
                                    session_id: session_id.clone(),
                                    event,
                                }).await;
                            }
                            Err(e) => {
                                warn!("docker: failed to parse JSON: {e}\nline: {line}");
                            }
                        }
                    }
                    Ok(None) => {
                        info!("docker: stdout EOF, session ended");
                        break;
                    }
                    Err(e) => {
                        warn!("docker: stdout read error: {e}");
                        break;
                    }
                }
            }

            _ = &mut idle_sleep => {
                warn!("docker: session {session_id} idle for 12h with no stdin — terminating");
                let _ = child.kill().await;
                break;
            }
        }
    }

    let exit_code = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(s)) => s.code().unwrap_or(-1),
        _ => { let _ = child.kill().await; -1 }
    };

    ws_send(
        ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
            stats,
            error: None,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Container lifecycle helpers
// ---------------------------------------------------------------------------

/// Returns the current state of a container ("running", "exited", etc.), or
/// `None` if the container does not exist.
async fn container_state(name: &str) -> Option<String> {
    let out = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", name])
        .output()
        .await
        .ok()?;

    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Ensures the named container exists and is in the `running` state.
///
/// * If it doesn't exist  → create it with `docker run -d sleep infinity`.
/// * If it is stopped     → restart it with `docker start`.
/// * If it is running     → no-op.
async fn ensure_container(
    name: &str,
    config: &crate::session_runner::SessionConfig,
    vm_cfg: &VmConfig,
) -> Result<()> {
    match container_state(name).await.as_deref() {
        Some("running") => {
            info!("docker: reusing running container {name}");
            return Ok(());
        }
        Some(state @ ("exited" | "stopped" | "created" | "paused")) => {
            info!("docker: restarting {state} container {name}");
            let s = Command::new("docker")
                .args(["start", name])
                .status()
                .await
                .context("docker start")?;
            anyhow::ensure!(s.success(), "docker start {name} failed");
            return Ok(());
        }
        Some(other) => {
            // Unexpected state (dead, removing, …) — remove and recreate.
            warn!("docker: container {name} is in unexpected state '{other}', removing");
            let _ = Command::new("docker").args(["rm", "-f", name]).status().await;
        }
        None => {
            // Container does not exist.
        }
    }

    // Create the container. It runs `sleep infinity` so it stays alive between
    // Claude exec invocations.
    let mut cmd = Command::new("docker");
    cmd.args(["run", "-d", "--name", name]);

    // Networking
    if vm_cfg.network_enabled {
        cmd.args(["--network", "bridge"]);
    } else {
        cmd.args(["--network", "none"]);
    }

    // Bind-mount host directories.
    for mount in &vm_cfg.mounts {
        cmd.args(["-v", &format!("{}:{}", mount.host_path, mount.guest_path)]);
    }

    // Default working directory.
    cmd.args(["-w", &config.default_cwd]);

    // Pass the current API key into the container environment.
    cmd.args(["--env", "ANTHROPIC_API_KEY"]);

    // Labels for lifecycle management.
    cmd.args([
        "--label", "claude.managed=true",
        "--label", &format!("claude.session_id={}", config.session_id),
        "--label", &format!("claude.claude_session_id={}", config.claude_session_id),
    ]);

    // Image + sentinel command.
    cmd.arg(&vm_cfg.image);
    cmd.args(["sleep", "infinity"]);

    let status = cmd
        .status()
        .await
        .context("docker run (create container)")?;
    anyhow::ensure!(status.success(), "docker run failed for container {name}");

    info!("docker: created container {name}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_user_message(text: &str) -> String {
    let content_json = serde_json::to_string(text).unwrap_or_else(|_| format!("{text:?}"));
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}
