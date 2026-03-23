//! Docker container session runner.
//!
//! Each Claude session runs inside a named, persistent Docker container:
//!
//!   claude-<claude_session_id>
//!
//! The container's main process IS Claude (via the baked-in entrypoint script).
//! The entrypoint handles first-run vs resume automatically using a flag file:
//!
//!   * New container  → `docker run -i` → entrypoint runs `claude --session-id`
//!   * Stopped        → `docker start -a -i` → entrypoint runs `claude --resume`
//!
//! stdin/stdout are piped directly through the container's I/O streams.
//! The container is NOT removed on exit (`--rm` is absent), so state persists
//! and a stopped container can be restarted to resume the session.

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

    // Build the docker command depending on whether the container already exists.
    let mut cmd = match container_state(&name).await.as_deref() {
        Some("running") => {
            // Already running — another client instance may be using it.
            // Attach so we can relay I/O.
            info!("docker: attaching to already-running container {name}");
            let mut c = Command::new("docker");
            c.args(["attach", "--no-stdin=false", &name]);
            c
        }
        Some("exited" | "stopped" | "created" | "paused") => {
            // Stopped container exists — restart it; entrypoint will --resume.
            info!("docker: restarting stopped container {name}");
            let mut c = Command::new("docker");
            c.args(["start", "-a", "-i", &name]);
            c
        }
        Some(other) => {
            // Unexpected state — remove it and fall through to create a new one.
            warn!("docker: container {name} is in state '{other}', removing before recreate");
            let _ = Command::new("docker").args(["rm", "-f", &name]).status().await;
            new_container_cmd(config, vm_cfg, &name)?
        }
        None => {
            // No container yet — create a fresh one.
            new_container_cmd(config, vm_cfg, &name)?
        }
    };

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().context("spawn docker")?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Collect stderr in the background so we can include it in error reports.
    let stderr_buf = Arc::new(tokio::sync::Mutex::new(String::new()));
    {
        let buf = Arc::clone(&stderr_buf);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!("docker stderr: {line}");
                let mut b = buf.lock().await;
                b.push_str(&line);
                b.push('\n');
            }
        });
    }

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

    info!("docker: container {name} running");

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
                        info!("docker: KillSession — stopping container {name}");
                        let _ = Command::new("docker").args(["stop", &name]).status().await;
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
                warn!("docker: session {session_id} idle for 12h with no stdin — stopping container");
                let _ = Command::new("docker").args(["stop", &name]).status().await;
                let _ = child.kill().await;
                break;
            }
        }
    }

    let exit_code = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(s)) => s.code().unwrap_or(-1),
        _ => { let _ = child.kill().await; -1 }
    };

    // Brief yield so the stderr reader task can flush any buffered lines
    // before we read stderr_buf.
    tokio::task::yield_now().await;

    let stderr_content = stderr_buf.lock().await.clone();

    if !stderr_content.is_empty() {
        warn!("docker: container {name} stderr: {stderr_content}");
    }

    let error = if exit_code != 0 {
        if stderr_content.is_empty() {
            Some(format!("claude exited with code {exit_code}"))
        } else {
            Some(format!("claude exited with code {exit_code}:\n{stderr_content}"))
        }
    } else {
        None
    };

    ws_send(
        ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
            stats,
            error,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Container creation helpers
// ---------------------------------------------------------------------------

/// Build the `docker run -i` command for a brand-new container.
fn new_container_cmd(
    config: &crate::session_runner::SessionConfig,
    vm_cfg: &VmConfig,
    name: &str,
) -> Result<Command> {
    // Check the image exists locally.
    // (Synchronous check is fine here — we're in an async context but this is
    //  a quick one-shot status check that can block for a moment.)
    let image_exists = std::process::Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", &vm_cfg.image])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !image_exists {
        anyhow::bail!(
            "Docker image '{}' not found locally. \
             Run /vmrebuild from Telegram (or `docker build`) to build it first.",
            vm_cfg.image
        );
    }

    let is_resume = if config.is_resume { "1" } else { "0" };

    let mut cmd = Command::new("docker");
    cmd.args(["run", "-i", "--name", name]);

    // Networking.
    if vm_cfg.network_enabled {
        cmd.args(["--network", "bridge"]);
    } else {
        cmd.args(["--network", "none"]);
    }

    // Bind-mount host directories.
    for mount in &vm_cfg.mounts {
        cmd.args(["-v", &format!("{}:{}", mount.host_path, mount.guest_path)]);
    }

    // Working directory.
    cmd.args(["-w", &config.default_cwd]);

    // Environment variables for the entrypoint script.
    cmd.args([
        "--env", "ANTHROPIC_API_KEY",
        "--env", &format!("CLAUDE_SESSION_ID={}", config.claude_session_id),
        "--env", &format!("CLAUDE_IS_RESUME={is_resume}"),
    ]);

    // Run as the host user so bind-mounted directories have correct permissions
    // and claude sees a non-root UID (required for --dangerously-skip-permissions).
    cmd.args(["--user", &host_uid_gid()]);

    // Labels for lifecycle management.
    cmd.args([
        "--label", "claude.managed=true",
        "--label", &format!("claude.session_id={}", config.session_id),
        "--label", &format!("claude.claude_session_id={}", config.claude_session_id),
    ]);

    // Override entrypoint explicitly so the behaviour is independent of
    // whatever ENTRYPOINT the image was built with.
    cmd.args(["--entrypoint", "/entrypoint.sh"]);

    // Image — no CMD args; docker start -a -i replays this same invocation.
    cmd.arg(&vm_cfg.image);

    // Forward any extra args to the entrypoint / claude.
    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    Ok(cmd)
}

// ---------------------------------------------------------------------------
// Container state helpers
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns "uid:gid" for the current process, used to run containers as the
/// host user so bind mounts have correct permissions and Claude is non-root.
fn host_uid_gid() -> String {
    // Safety: getuid/getgid are always safe to call.
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    format!("{uid}:{gid}")
}

fn format_user_message(text: &str) -> String {
    let content_json = serde_json::to_string(text).unwrap_or_else(|_| format!("{text:?}"));
    format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{content_json}}}}}\n")
}
